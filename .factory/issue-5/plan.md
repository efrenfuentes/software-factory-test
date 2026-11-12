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
- tokio: add the `time` feature (required for `tokio::time::timeout`, which bounds the
  whole startup check — see `connect`). Tests also use `tokio::net::TcpListener` (`net`
  is already enabled).

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
  - `pub fn with_password_override(self, env: Option<String>) -> Self` — **every** `Some(v)`
    replaces the password, including `Some("")` (→ empty password); only `None` keeps the
    file value. Matches the spec ("if `DATABASE_PASSWORD` is set, it replaces the file password").
  - `pub fn load(path: &Path) -> Result<Self, DbError>` — reads the file, then `from_toml`; errors carry the path.
  - `pub fn connect_options(&self) -> PgConnectOptions` — host, port, username, password, database, ssl_mode.
  - `pub fn display_target(&self) -> String` — `"host:port/database"` for logs.
- `pub enum ConfigSource { Explicit(PathBuf), Default(PathBuf) }` and
  `pub const DEFAULT_CONFIG_PATH: &str = "config/database.toml";`
  `pub fn config_source(env: Option<&str>) -> ConfigSource` — pure, unit-tested
  (same style as `request_logging_enabled`).
- `pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, DbError>` — a single
  overall deadline `d = Duration::from_secs(config.connect_timeout_secs)` covering **both**
  connecting and verifying:
  ```rust
  let attempt = async {
      let pool = PgPoolOptions::new()
          .max_connections(config.max_connections)
          .acquire_timeout(d)
          .connect_with(config.connect_options())
          .await?;
      sqlx::query("SELECT 1").execute(&pool).await?;
      Ok::<_, sqlx::Error>(pool)
  };
  match tokio::time::timeout(d, attempt).await {
      Ok(Ok(pool)) => Ok(pool),
      Ok(Err(source)) => Err(DbError::Connect { target, source }),
      Err(_elapsed) => Err(DbError::Timeout { target, secs: config.connect_timeout_secs }),
  }
  ```
  `acquire_timeout` alone only bounds acquiring a connection; the outer
  `tokio::time::timeout` guarantees that a server which stops answering after the
  connection is established (e.g. during `SELECT 1`) cannot hang startup. Dropping the
  future on timeout drops the pool and its connection.
- `pub enum DbError { Read { path, source: io::Error }, Parse { path: Option<PathBuf>, message: String, line: Option<usize>, column: Option<usize> }, Connect { target: String, source: sqlx::Error }, Timeout { target: String, secs: u64 } }`
  with `Display` messages that never include the password:
  - sqlx errors don't include it when using `PgConnectOptions`; do not build a URL string.
  - **Parse errors**: `toml::de::Error`'s own `Display` renders a snippet of the offending
    source line, which may be the `password = "…"` line. So never store or format the
    `toml::de::Error` itself: in `from_toml` convert it to `err.message()` (bare
    description) plus line/column computed from `err.span()` against the input, and render
    `invalid database config <path>: <message> (line L, column C)`. No source snippet is
    ever printed. `load` fills in `path`.
  - `Timeout`: `could not connect to PostgreSQL at <target>: no response within <secs>s`.
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
- **Startup hang** if the host blackholes packets or stops answering after connecting:
  bounded by an outer `tokio::time::timeout(connect_timeout_secs)` around connect +
  `SELECT 1` (not only `acquire_timeout`), covered by an automated test against a silent
  TCP listener.
- **Password in parse diagnostics**: TOML error snippets could echo the password line;
  mitigated by rendering only `message()` + line/column, with a unit test.
- **CI without Postgres**: real-DB test is `#[ignore]`; normal `cargo test` must not need a DB.
- `config/database.toml` resolved relative to the working directory; documented in README
  (run from repo root, or set `DATABASE_CONFIG` to an absolute path).

## Testing
Unit tests in `src/db.rs`:
- full file parses into all fields; minimal file (host, user, database) gets defaults;
- missing `host` / `database` → parse error mentioning the key;
- `ssl_mode = "bogus"` → error; unknown key (e.g. `hots`) → error; invalid TOML → error;
- `with_password_override(Some("secret"))` replaces; `Some("")` replaces with `Some("")`;
  `None` keeps the file value;
- `format!("{cfg:?}")` does not contain the password;
- malformed file containing a password, e.g.
  `[postgres]\nhost = "localhost"\nuser = "u"\npassword = "s3cr3t-pw" oops\ndatabase = "d"`:
  `from_toml` returns `DbError::Parse`; its `Display` and `Debug` do not contain
  `s3cr3t-pw` and do mention line 4. Same check for a duplicate `password` key;
- `config_source(None)` → `Default("config/database.toml")`; `config_source(Some("/x.toml"))` → `Explicit`;
- `connect_options()` maps host/port/database/ssl_mode (check via `get_host()`, `get_port()`, `get_database()`).
- the example file `config/database.example.toml` parses (via `include_str!`).

Integration: `tests/postgres.rs` as above, plus a **bounded-timeout test** (not ignored,
no Postgres needed): bind `tokio::net::TcpListener` on `127.0.0.1:0`, spawn a task that
accepts connections and holds them open without ever writing; point a config at that port
with `connect_timeout_secs = 1`; measure with `Instant` and assert `db::connect` returns
`Err` (expected `DbError::Timeout`; accept `Connect` if sqlx's own pool timeout fires first)
in under ~3 s, and that the error `Display` does not contain the password. The test body is
wrapped in a test-side `tokio::time::timeout(10s)` so a regression fails instead of hanging CI. Existing `tests/health.rs` and
`tests/request_logging.rs` must pass unchanged.

Manual: the three startup scenarios (no file / DB up / DB down) from the spec.
Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
