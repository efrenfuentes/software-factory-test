//! PostgreSQL configuration (read from a TOML file) and connection pool.

use std::{
    fmt, io,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Deserialize;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};

/// Config file used when `DATABASE_CONFIG` is not set (relative to the working directory).
pub const DEFAULT_CONFIG_PATH: &str = "config/database.toml";

/// TLS mode for the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

impl From<SslMode> for PgSslMode {
    fn from(mode: SslMode) -> Self {
        match mode {
            SslMode::Disable => PgSslMode::Disable,
            SslMode::Prefer => PgSslMode::Prefer,
            SslMode::Require => PgSslMode::Require,
        }
    }
}

/// Connection settings, the `[postgres]` table of the config file.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    #[serde(default, deserialize_with = "deserialize_password")]
    pub password: Option<String>,
    pub database: String,
    #[serde(default)]
    pub ssl_mode: SslMode,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
}

/// Deserializes `password`, replacing any error with a fixed message so a malformed value
/// (for example an unquoted number) is never echoed in diagnostics.
fn deserialize_password<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
        .map_err(|_| serde::de::Error::custom("invalid value for `password`: expected a string"))
}

fn default_port() -> u16 {
    5432
}

fn default_max_connections() -> u32 {
    5
}

fn default_connect_timeout_secs() -> u64 {
    5
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("database", &self.database)
            .field("ssl_mode", &self.ssl_mode)
            .field("max_connections", &self.max_connections)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    postgres: DatabaseConfig,
}

/// Errors from loading the config or connecting. Messages never contain the password.
#[derive(Debug)]
pub enum DbError {
    /// The config file could not be read.
    Read { path: PathBuf, source: io::Error },
    /// The config file is not valid. Only the bare TOML message is kept (no source snippet),
    /// so the password can never be echoed.
    Parse {
        path: Option<PathBuf>,
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    /// Connecting or the `SELECT 1` check failed.
    Connect { target: String, source: sqlx::Error },
    /// The server did not answer within `connect_timeout_secs`.
    Timeout { target: String, secs: u64 },
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Read { path, source } => {
                write!(
                    f,
                    "could not read database config {}: {source}",
                    path.display()
                )
            }
            DbError::Parse {
                path,
                message,
                line,
                column,
            } => {
                write!(f, "invalid database config")?;
                if let Some(path) = path {
                    write!(f, " {}", path.display())?;
                }
                write!(f, ": {}", message.trim())?;
                match (line, column) {
                    (Some(l), Some(c)) => write!(f, " (line {l}, column {c})"),
                    (Some(l), None) => write!(f, " (line {l})"),
                    _ => Ok(()),
                }
            }
            DbError::Connect { target, source } => {
                write!(f, "could not connect to PostgreSQL at {target}: {source}")
            }
            DbError::Timeout { target, secs } => write!(
                f,
                "could not connect to PostgreSQL at {target}: no response within {secs}s"
            ),
        }
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DbError::Read { source, .. } => Some(source),
            DbError::Connect { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// 1-based line and column of byte offset `pos` in `input`.
fn line_column(input: &str, pos: usize) -> (usize, usize) {
    let mut pos = pos.min(input.len());
    while !input.is_char_boundary(pos) {
        pos -= 1;
    }
    let before = &input[..pos];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = before[line_start..].chars().count() + 1;
    (line, column)
}

impl DatabaseConfig {
    /// Parses the contents of a config file.
    pub fn from_toml(s: &str) -> Result<Self, DbError> {
        toml::from_str::<ConfigFile>(s)
            .map(|file| file.postgres)
            .map_err(|err| {
                let (line, column) = match err.span() {
                    Some(span) => {
                        let (l, c) = line_column(s, span.start);
                        (Some(l), Some(c))
                    }
                    None => (None, None),
                };
                DbError::Parse {
                    path: None,
                    message: err.message().to_string(),
                    line,
                    column,
                }
            })
    }

    /// Reads and parses a config file.
    pub fn load(path: &Path) -> Result<Self, DbError> {
        let contents = std::fs::read_to_string(path).map_err(|source| DbError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml(&contents).map_err(|err| match err {
            DbError::Parse {
                message,
                line,
                column,
                ..
            } => DbError::Parse {
                path: Some(path.to_path_buf()),
                message,
                line,
                column,
            },
            other => other,
        })
    }

    /// Applies `DATABASE_PASSWORD`: any set value (even empty) replaces the file password.
    pub fn with_password_override(mut self, env: Option<String>) -> Self {
        if let Some(password) = env {
            self.password = Some(password);
        }
        self
    }

    /// `host:port/database`, for log and error messages.
    pub fn display_target(&self) -> String {
        format!("{}:{}/{}", self.host, self.port, self.database)
    }

    /// sqlx connection options for this config.
    pub fn connect_options(&self) -> PgConnectOptions {
        let options = PgConnectOptions::new()
            .host(&self.host)
            .port(self.port)
            .username(&self.user)
            .database(&self.database)
            .ssl_mode(self.ssl_mode.into());
        match &self.password {
            Some(password) => options.password(password),
            None => options,
        }
    }
}

/// Where the config file comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Given with `DATABASE_CONFIG`; it must exist.
    Explicit(PathBuf),
    /// The default path; if missing, the server runs without a database.
    Default(PathBuf),
}

/// Picks the config file from the `DATABASE_CONFIG` value.
pub fn config_source(env: Option<&str>) -> ConfigSource {
    match env {
        Some(path) => ConfigSource::Explicit(PathBuf::from(path)),
        None => ConfigSource::Default(PathBuf::from(DEFAULT_CONFIG_PATH)),
    }
}

/// Builds a pool and checks it with `SELECT 1`. The whole check is bounded by
/// `connect_timeout_secs`.
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, DbError> {
    let deadline = Duration::from_secs(config.connect_timeout_secs);
    let target = config.display_target();
    let attempt = async {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(deadline)
            .connect_with(config.connect_options())
            .await?;
        sqlx::query("SELECT 1").execute(&pool).await?;
        Ok::<_, sqlx::Error>(pool)
    };
    match tokio::time::timeout(deadline, attempt).await {
        Ok(Ok(pool)) => Ok(pool),
        Ok(Err(source)) => Err(DbError::Connect { target, source }),
        Err(_elapsed) => Err(DbError::Timeout {
            target,
            secs: config.connect_timeout_secs,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const FULL: &str = r#"
[postgres]
host = "db.example.com"
port = 6543
user = "app"
password = "hunter2"
database = "appdb"
ssl_mode = "require"
max_connections = 10
connect_timeout_secs = 3
"#;

    const MINIMAL: &str = r#"
[postgres]
host = "localhost"
user = "postgres"
database = "software_factory"
"#;

    fn parse_err(s: &str) -> String {
        let err = DatabaseConfig::from_toml(s).unwrap_err();
        assert!(matches!(err, DbError::Parse { .. }), "{err:?}");
        err.to_string()
    }

    #[test]
    fn parses_full_file() {
        let cfg = DatabaseConfig::from_toml(FULL).unwrap();
        assert_eq!(
            cfg,
            DatabaseConfig {
                host: "db.example.com".into(),
                port: 6543,
                user: "app".into(),
                password: Some("hunter2".into()),
                database: "appdb".into(),
                ssl_mode: SslMode::Require,
                max_connections: 10,
                connect_timeout_secs: 3,
            }
        );
    }

    #[test]
    fn minimal_file_gets_defaults() {
        let cfg = DatabaseConfig::from_toml(MINIMAL).unwrap();
        assert_eq!(cfg.port, 5432);
        assert_eq!(cfg.password, None);
        assert_eq!(cfg.ssl_mode, SslMode::Prefer);
        assert_eq!(cfg.max_connections, 5);
        assert_eq!(cfg.connect_timeout_secs, 5);
    }

    #[test]
    fn missing_required_key() {
        let msg = parse_err("[postgres]\nhost = \"h\"\nuser = \"u\"\n");
        assert!(msg.contains("database"), "{msg}");
        let msg = parse_err("[postgres]\nuser = \"u\"\ndatabase = \"d\"\n");
        assert!(msg.contains("host"), "{msg}");
    }

    #[test]
    fn bad_ssl_mode() {
        let msg = parse_err(&format!("{MINIMAL}ssl_mode = \"bogus\"\n"));
        assert!(msg.contains("bogus"), "{msg}");
    }

    #[test]
    fn unknown_key() {
        let msg = parse_err(&format!("{MINIMAL}hots = \"x\"\n"));
        assert!(msg.contains("hots"), "{msg}");
    }

    #[test]
    fn invalid_toml() {
        parse_err("[postgres\nhost = ");
    }

    #[test]
    fn debug_hides_password() {
        let cfg = DatabaseConfig::from_toml(FULL).unwrap();
        let debug = format!("{cfg:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("***"), "{debug}");
    }

    fn assert_parse_error_hides_password(input: &str) {
        let err = DatabaseConfig::from_toml(input).unwrap_err();
        let display = err.to_string();
        let debug = format!("{err:?}");
        assert!(matches!(err, DbError::Parse { .. }), "{debug}");
        assert!(!display.contains("s3cr3t-pw"), "{display}");
        assert!(!debug.contains("s3cr3t-pw"), "{debug}");
        assert!(display.contains("line 4"), "{display}");
    }

    #[test]
    fn parse_error_on_password_line_hides_password() {
        assert_parse_error_hides_password(
            "[postgres]\nhost = \"localhost\"\nuser = \"u\"\npassword = \"s3cr3t-pw\" oops\ndatabase = \"d\"\n",
        );
    }

    #[test]
    fn duplicate_password_hides_password() {
        assert_parse_error_hides_password(
            "[postgres]\nhost = \"localhost\"\npassword = \"a\"\npassword = \"s3cr3t-pw\"\nuser = \"u\"\ndatabase = \"d\"\n",
        );
    }

    #[test]
    fn unquoted_numeric_password_hides_value() {
        let input = "[postgres]\nhost = \"localhost\"\nuser = \"u\"\npassword = 987654321\ndatabase = \"d\"\n";
        let err = DatabaseConfig::from_toml(input).unwrap_err();
        let display = err.to_string();
        let debug = format!("{err:?}");
        assert!(matches!(err, DbError::Parse { .. }), "{debug}");
        assert!(!display.contains("987654321"), "{display}");
        assert!(!debug.contains("987654321"), "{debug}");
        assert!(display.contains("password"), "{display}");
        assert!(display.contains("line 4"), "{display}");
    }

    #[test]
    fn password_override() {
        let cfg = DatabaseConfig::from_toml(FULL).unwrap();
        assert_eq!(
            cfg.clone()
                .with_password_override(Some("secret".into()))
                .password
                .as_deref(),
            Some("secret")
        );
        assert_eq!(
            cfg.clone()
                .with_password_override(Some(String::new()))
                .password
                .as_deref(),
            Some("")
        );
        assert_eq!(
            cfg.with_password_override(None).password.as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn config_source_from_env() {
        assert_eq!(
            config_source(None),
            ConfigSource::Default(PathBuf::from("config/database.toml"))
        );
        assert_eq!(
            config_source(Some("/x.toml")),
            ConfigSource::Explicit(PathBuf::from("/x.toml"))
        );
    }

    #[test]
    fn load_missing_file_is_read_error() {
        let path = Path::new("/definitely/not/here/database.toml");
        let err = DatabaseConfig::load(path).unwrap_err();
        assert!(matches!(err, DbError::Read { .. }), "{err:?}");
        assert!(err.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn display_target_format() {
        let cfg = DatabaseConfig::from_toml(MINIMAL).unwrap();
        assert_eq!(cfg.display_target(), "localhost:5432/software_factory");
    }

    #[test]
    fn connect_options_mapping() {
        let opts = DatabaseConfig::from_toml(FULL).unwrap().connect_options();
        assert_eq!(opts.get_host(), "db.example.com");
        assert_eq!(opts.get_port(), 6543);
        assert_eq!(opts.get_username(), "app");
        assert_eq!(opts.get_database(), Some("appdb"));
        assert!(matches!(opts.get_ssl_mode(), PgSslMode::Require));

        for (mode, expected) in [
            (SslMode::Disable, PgSslMode::Disable),
            (SslMode::Prefer, PgSslMode::Prefer),
            (SslMode::Require, PgSslMode::Require),
        ] {
            let mapped: PgSslMode = mode.into();
            assert_eq!(format!("{mapped:?}"), format!("{expected:?}"));
        }
    }

    #[test]
    fn example_file_parses() {
        let cfg =
            DatabaseConfig::from_toml(include_str!("../config/database.example.toml")).unwrap();
        assert_eq!(cfg.display_target(), "localhost:5432/software_factory");
        assert_eq!(cfg.user, "postgres");
        assert_eq!(cfg.password.as_deref(), Some("postgres"));
    }

    #[tokio::test]
    async fn connect_to_silent_server_times_out() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let cfg = DatabaseConfig {
            port,
            host: "127.0.0.1".into(),
            password: Some("s3cr3t-pw".into()),
            ssl_mode: SslMode::Disable,
            connect_timeout_secs: 1,
            ..DatabaseConfig::from_toml(MINIMAL).unwrap()
        };

        let start = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(10), connect(&cfg))
            .await
            .expect("connect did not respect its timeout");
        let elapsed = start.elapsed();

        let err = result.unwrap_err();
        assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
        assert!(
            matches!(err, DbError::Timeout { .. } | DbError::Connect { .. }),
            "{err:?}"
        );
        assert!(!err.to_string().contains("s3cr3t-pw"), "{err}");
        assert!(!format!("{err:?}").contains("s3cr3t-pw"), "{err:?}");
    }
}
