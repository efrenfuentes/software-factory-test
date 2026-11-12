# Plan — Issue #5: PostgreSQL connection

## Approach
Add a `db` module (like the existing `logging` module) that owns the PostgreSQL config
file and the connection pool. Keep the pure parts (parsing, defaults, password override,
"which file to load") as plain functions with unit tests, and keep I/O (reading the file,
connecting) in thin wrappers. `main.rs` decides at startup whether to connect, and attaches
the pool to the router as an `Extension<PgPool>`. `app()` stays unchanged, so existing
tests are untouched.

## Dependencies (`Cargo.toml`)
Add with `cargo add` so the latest compatible versions are pinned in `Cargo.lock`:
- `sqlx` with `default-features = false` and features: `runtime-tokio`, a rustls TLS
  feature (`tls-rustls` / `tls-rustls-ring`, whichever the current version names), `postgres`.
  Do **not** enable `macros`/`migrate` (no compile-time DB needed).
- `toml` (parsing the config file; `serde` with `derive` is already present).
- `thiserror` (optional, for the error enum; a hand-written `Display` impl is fine too).
- tokio: add the `time` feature only if needed for timeouts (sqlx's `acquire_timeout` should cover it).

## Files

### `src/db.rs` (new)
- `pub enum SslMode { Disable, Prefer, Require }` — `#[derive(Deserialize)]`,
  `#[serde(rename_all = "lowercase")]`, default `Prefer`; maps to `sqlx::postgres::PgSslMode`.
- `pub struct DatabaseConfig { host, port: u16 (default 5432), user, password: Option<String>, database, ssl_mode (default Prefer), max_connections: u32 (default 5), connect_timeout_secs: u64 (default 5) }`
  - `#[serde(deny_unknown_fields)]` so typos are reported.
  - Manual `impl Debug` that prints `password: "***"` (or `None`).
- `#[derive(Deserialize)] struct ConfigFile { postgres: DatabaseConfig }` (private).
- `impl DatabaseConfig`:
  - `pub fn from_toml(s: &str) -> Result<Self, DbError>` — parses `ConfigFile`, returns `postgres`.
  - `pub fn with_password_override(self, env: Option<String>) -> Self` — non-empty env replaces password.
  - `pub fn load(path: &Path) -> Result<Self, DbError>` — reads the file, then `from_toml`; errors carry the path.
  - `pub fn connect_options(&self) -> PgConnectOptions` — host, port, username, password, database, ssl_mode.
  - `pub fn display_target(&self) -> String` — `"host:port/database"` for logs.
- `pub enum ConfigSource { Explicit(PathBuf), Default(PathBuf) }` and
  `pub const DEFAULT_CONFIG_PATH: &str = "config/database.toml";`
  `pub fn config_source(env: Option<&str>) -> ConfigSource` — pure, unit-tested
  (same style as `request_logging_enabled`).
- `pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, DbError>` —
  `PgPoolOptions::new().max_connections(..).acquire_timeout(Duration::from_secs(..)).connect_with(config.connect_options())`,
  then `sqlx::query("SELECT 1").execute(&pool)` to verify.
- `pub enum DbError { Read { path, source: io::Error }, Parse { path?, source: toml::de::Error }, Connect { target: String, source: sqlx::Error } }`
  with `Display` messages that never include the password (sqlx errors don't include it
  when using `PgConnectOptions`; do not build a URL string).
- `#[cfg(test)] mod tests` — see Testing.

### `src/lib.rs`
- `pub mod db;`
- `pub fn with_database(router: Router, pool: sqlx::PgPool) -> Router` — `router.layer(Extension(pool))`.
  Keep `app()` and `app_with_request_logging()` as they are.

### `src/main.rs`
After building `router` (logging on/off as today):
1. `let source = db::config_source(std::env::var("DATABASE_CONFIG").ok().as_deref());`
2. If `Default(path)` and `!path.exists()` → print the "running without a database" line, keep router.
3. Otherwise `DatabaseConfig::load(&path)` → `.with_password_override(std::env::var("DATABASE_PASSWORD").ok())`
   → `db::connect(&cfg).await`. On error: `eprintln!("Database error: {err}")` and
   `std::process::exit(1)`. On success print `Connected to PostgreSQL at {target}` and
   `router = with_database(router, pool)`.
   (Note the `Explicit` path that does not exist naturally fails in `load` with a Read error.)

### `config/database.example.toml` (new)
Commented example of every key, values matching `compose.yaml`.

### `compose.yaml` (new)
`postgres:17` (or current major) service, port `5432:5432`, `POSTGRES_USER=postgres`,
`POSTGRES_PASSWORD=postgres`, `POSTGRES_DB=software_factory`, a healthcheck with `pg_isready`.

### `.gitignore`
Add `/config/database.toml`.

### `tests/postgres.rs` (new)
`#[tokio::test] #[ignore = "needs a running PostgreSQL (docker compose up -d)"]`:
load `config/database.example.toml` (path via `env!("CARGO_MANIFEST_DIR")`), apply
`DATABASE_PASSWORD` override, `db::connect`, run `SELECT 1::int4` and assert `1`; also
build `with_database(app(), pool)` and check `GET /health` is still 200.
A second ignored test: config pointing at an unused port (e.g. 1) with
`connect_timeout_secs = 2` → `connect` returns `Err(DbError::Connect { .. })` and its
`Display` does not contain the password. (This one needs no running DB but is quick; it can
be a normal, non-ignored test if it reliably fails fast with connection refused — prefer
non-ignored.)

### `README.md`
New "Database (PostgreSQL)" section: start Postgres with `docker compose up -d`, copy the
example config, list keys and defaults, `DATABASE_CONFIG`, `DATABASE_PASSWORD`, behaviour
when no file exists / connection fails, and `cargo test -- --ignored` for the DB tests.

## Data / API changes
- No schema, no new endpoints, `/health` unchanged.
- New public library items: `db` module, `with_database`.
- New runtime inputs: `config/database.toml`, `DATABASE_CONFIG`, `DATABASE_PASSWORD`.

## Risks
- **Secrets leakage**: password in logs/errors. Mitigated by manual `Debug`, using
  `PgConnectOptions` instead of a URL, and a unit test on `Debug` + a test on the connect error message.
- **Committing credentials**: `.gitignore` entry; only the example file is committed.
- **sqlx feature names / versions** change between releases; check `cargo add sqlx --features …`
  output and the docs for the TLS feature name. Build time increases.
- **Startup hang** if the host blackholes packets: bounded by `acquire_timeout(connect_timeout_secs)`.
- **CI without Postgres**: real-DB test is `#[ignore]`; normal `cargo test` must not need a DB.
- `config/database.toml` resolved relative to the working directory; documented in README
  (run from repo root, or set `DATABASE_CONFIG` to an absolute path).

## Testing
Unit tests in `src/db.rs`:
- full file parses into all fields; minimal file (host, user, database) gets defaults;
- missing `host` / `database` → parse error mentioning the key;
- `ssl_mode = "bogus"` → error; unknown key (e.g. `hots`) → error; invalid TOML → error;
- `with_password_override(Some("secret"))` replaces; `None` and `Some("")` keep the file value;
- `format!("{cfg:?}")` does not contain the password;
- `config_source(None)` → `Default("config/database.toml")`; `config_source(Some("/x.toml"))` → `Explicit`;
- `connect_options()` maps host/port/database/ssl_mode (check via `get_host()`, `get_port()`, `get_database()`).
- the example file `config/database.example.toml` parses (via `include_str!`).

Integration: `tests/postgres.rs` as above. Existing `tests/health.rs` and
`tests/request_logging.rs` must pass unchanged.

Manual: the three startup scenarios (no file / DB up / DB down) from the spec.
Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
