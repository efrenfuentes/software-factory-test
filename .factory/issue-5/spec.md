# Issue #5 — PostgreSQL connection

## Problem
The API (Axum, `src/lib.rs` + `src/main.rs`) has no database. The issue asks to use
PostgreSQL as the API's database, with the connection settings kept in a **dedicated
configuration file** rather than hard-coded.

## Expected behaviour
- The connection settings live in a TOML file, by default `config/database.toml`. Another
  path can be given with the environment variable `DATABASE_CONFIG`.
- The repository ships `config/database.example.toml` with every field documented and safe
  local defaults. `config/database.toml` is listed in `.gitignore` so real credentials are
  never committed.
- File format (all keys under a `[postgres]` table):
  ```toml
  [postgres]
  host = "localhost"          # required
  port = 5432                 # optional, default 5432
  user = "postgres"           # required
  password = "postgres"       # optional; overridden by env DATABASE_PASSWORD
  database = "software_factory" # required
  ssl_mode = "prefer"         # optional: "disable" | "prefer" | "require", default "prefer"
  max_connections = 5         # optional, default 5
  connect_timeout_secs = 5    # optional, default 5
  ```
- If the environment variable `DATABASE_PASSWORD` is set, it replaces `password` from the
  file, so the password does not have to be written to disk. This applies to every set
  value, **including an empty string** (`DATABASE_PASSWORD=` means "empty password");
  only an unset variable keeps the file value.
- At startup the server:
  - **if the config file exists**: parses it, builds a PostgreSQL connection pool, checks
    the connection with `SELECT 1`, and prints
    `Connected to PostgreSQL at <host>:<port>/<database>` (never the password). The pool is
    made available to handlers (Axum `Extension<PgPool>`) for future endpoints.
  - **if the file is invalid** (bad TOML, missing required key, unknown `ssl_mode`, etc.)
    or **the database cannot be reached / does not answer** within `connect_timeout_secs`:
    prints a clear error that names the file / host and exits with a non-zero status. The
    timeout covers the whole startup check (opening the connection **and** the
    `SELECT 1` verification), so a server that accepts the TCP connection but never
    responds cannot hang startup. The password is never printed — including in TOML parse
    errors, which must not echo the offending source line.
  - **if `DATABASE_CONFIG` is set but the file does not exist**: error and non-zero exit.
  - **if `DATABASE_CONFIG` is not set and `config/database.toml` does not exist**: prints
    `No database configuration found at config/database.toml; running without a database`
    and starts as today.
- `GET /health`, request logging, `PORT`, and all existing behaviour stay the same.
- A `compose.yaml` starts a local PostgreSQL that matches the example config, so
  `docker compose up -d` + `cp config/database.example.toml config/database.toml` +
  `cargo run` works out of the box.

## Acceptance criteria
- [ ] `config/database.example.toml` exists, documents every key, and matches `compose.yaml`.
- [ ] `config/database.toml` is git-ignored.
- [ ] `DatabaseConfig` parses a full file and a minimal file (defaults applied: port 5432,
      ssl_mode prefer, max_connections 5, connect_timeout_secs 5) — unit tests.
- [ ] Missing required keys, an unknown `ssl_mode`, and invalid TOML give an error that
      names the problem — unit tests.
- [ ] `DATABASE_PASSWORD` overrides the file password whenever it is set, including
      `Some("")`; unset keeps the file value — unit tests (logic tested through a pure
      function that receives the env value, no global env mutation).
- [ ] The `Debug` output of the config does not contain the password — unit test.
- [ ] A malformed config file containing a password (syntax error on the password line)
      gives a parse error whose `Display`/`Debug` does not contain the password — unit test.
- [ ] Startup verification is bounded: connecting to a server that accepts TCP but never
      replies fails within about `connect_timeout_secs` — automated test, no Postgres needed.
- [ ] With Postgres running and a valid config, `cargo run` prints
      `Connected to PostgreSQL at localhost:5432/software_factory` and `/health` still returns 200.
- [ ] With a valid config but Postgres stopped, `cargo run` exits non-zero with a clear
      error within about `connect_timeout_secs`.
- [ ] Without any config file, `cargo run` starts as before and prints the "running without
      a database" line.
- [ ] An integration test connects to a real Postgres and runs `SELECT 1`; it is
      `#[ignore]` by default and runs with `cargo test -- --ignored` when Postgres is up
      (`compose.yaml`). Normal `cargo test` needs no database.
- [ ] README documents the config file, the env vars `DATABASE_CONFIG` and
      `DATABASE_PASSWORD`, and how to start Postgres locally.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass.

## Out of scope
- Tables, schema, migrations, models, or any endpoint that reads/writes data.
- Reporting database status in `/health` or a new readiness endpoint.
- Other databases, ORMs, or connection URLs (`DATABASE_URL`) as an alternative input.
- Secret managers, TLS client certificates, custom CA files.
- Reconnect/retry loops at startup (one attempt bounded by the timeout).
- Per-query timeouts for future handlers (only the startup check is bounded here).

## Assumptions
- "A file for Postgres configuration" means a configuration file with connection settings
  (not a SQL/init file). TOML is chosen because the project is Rust (Cargo, mise already use TOML).
- The database is optional when no config file is present, so `cargo run` and `cargo test`
  keep working on machines without Postgres. If a config file is present, the connection
  is mandatory (fail fast).
- Driver: `sqlx` (async, Tokio-native, works with Axum) with only the Postgres + Tokio
  runtime + rustls TLS features, no compile-time query macros (no `DATABASE_URL` needed to build).
- Default example credentials (`postgres`/`postgres`, database `software_factory`) are for
  local development only.
