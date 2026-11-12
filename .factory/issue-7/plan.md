# Plan — Issue #7: Post resource

## Approach

Follow the existing layout: library code in `src/` exposed through `src/lib.rs`, the
binary in `src/main.rs` only wires things up, integration tests in `tests/` using
`tower::ServiceExt::oneshot`, and DB-dependent tests marked `#[ignore]`.

1. **Migration**: add `migrations/20261112000000_create_posts.sql` and embed it with
   `sqlx::migrate!("./migrations")`. Add `pub async fn migrate(pool: &PgPool) -> Result<(), DbError>`
   in `src/db.rs` (new `DbError::Migrate { source: sqlx::migrate::MigrateError }` variant;
   its message must not include the password — `MigrateError` does not contain connection
   options, but keep the message as `"could not apply database migrations: {source}"`).
2. **Posts module**: new `src/posts.rs` with
   - `Post` (`#[derive(Serialize, sqlx::FromRow)]`: `id: i64`, `title: String`, `body: String`,
     `created_at: DateTime<Utc>`, `updated_at: DateTime<Utc>`).
   - `PostInput` (`#[derive(Deserialize)] #[serde(deny_unknown_fields)]`: `title: String`, `body: String`)
     and `fn validate(&self) -> Result<(), ApiError>` (title trimmed non-empty, `chars().count() <= 200`).
     Store the title as sent (no trimming of the stored value) — keep it simple; validation
     only checks it is not blank.
   - `ApiError` enum (`BadRequest(String)`, `Unprocessable(String)`, `NotFound`,
     `DatabaseUnavailable`, `Internal`) implementing `IntoResponse` → status + `Json({"error": msg})`.
     `Internal` logs the underlying `sqlx::Error` with `eprintln!` (or `tracing::error!`) and
     returns the generic message. `From<sqlx::Error>` maps to `Internal`.
   - Handlers `create`, `list`, `get`, `update`, `delete`, using runtime queries
     (`sqlx::query_as::<_, Post>(...)`, not the compile-time `query!` macros — those need a live
     DB or offline data at build time):
     - create: `INSERT INTO posts (title, body) VALUES ($1, $2) RETURNING *`
     - list: `SELECT * FROM posts ORDER BY id`
     - get: `SELECT * FROM posts WHERE id = $1` → `fetch_optional`, `None` → 404
     - update: `UPDATE posts SET title = $1, body = $2, updated_at = now() WHERE id = $3 RETURNING *`
     - delete: `DELETE FROM posts WHERE id = $1` → `rows_affected() == 0` → 404
     (List columns explicitly instead of `*` to be safe against future column additions.)
   - Extractors: pool as `Option<Extension<PgPool>>` (Axum 0.8 implements
     `OptionalFromRequestParts` for `Extension`); `None` → 503. Body as
     `Result<Json<PostInput>, JsonRejection>` and path as `Result<Path<i64>, PathRejection>`, both
     rejections mapped to `ApiError::BadRequest(rejection.body_text())`. Ids `<= 0` → 400.
   - `pub fn router() -> Router` with `/posts` (`get(list).post(create)`) and `/posts/{id}`
     (`get(get).put(update).delete(delete)`) — Axum 0.8 `{id}` path syntax.
3. **Wire up**: `app()` in `src/lib.rs` merges `posts::router()`. `with_database` stays as-is
   (the `Extension` layer added after `app()` reaches the merged routes). `main.rs` calls
   `db::migrate(&pool)` right after a successful `connect`, prints `Database migrations applied`,
   and on error `eprintln!("Database error: {err}")` + exit 1 (same path as connect errors).
4. **Docs**: README section "Posts API" with the table of endpoints, error format, curl
   examples, and that migrations run at startup.

## Files

| File | Change | Why |
|------|--------|-----|
| `Cargo.toml` / `Cargo.lock` | sqlx features `+macros, migrate, chrono`; add `chrono = { version = "0.4", default-features = false, features = ["serde", "clock", "std"] }` | migrations macro and timestamp types |
| `migrations/20261112000000_create_posts.sql` | new | `posts` table |
| `src/db.rs` | `migrate()` + `DbError::Migrate` (Display/source) | apply embedded migrations |
| `src/posts.rs` | new | model, validation, errors, handlers, router |
| `src/lib.rs` | `pub mod posts;` and merge `posts::router()` into `app()` | expose routes |
| `src/main.rs` | call `db::migrate` after connect | create table at startup |
| `tests/posts.rs` | new | HTTP tests (no-DB and `#[ignore]` DB) |
| `README.md` | Posts API section; tests section mentions new ignored tests | docs |

## Data / API changes

- New table `posts` (see spec). sqlx also creates `_sqlx_migrations`.
- New public routes `/posts`, `/posts/{id}`. No change to `/health`.
- New public Rust items: `posts` module, `db::migrate`, `DbError::Migrate`.

## Risks

- **sqlx 0.9 feature names / APIs**: confirm `migrate`, `macros`, `chrono` features exist in the
  locked sqlx 0.9 version (`cargo add sqlx --features ...` will fail fast otherwise). If
  `chrono` is unavailable, use the `time` feature with `time::OffsetDateTime` and
  `#[serde(with = "time::serde::rfc3339")]`.
- **`migrate!` path**: relative to `CARGO_MANIFEST_DIR`; adding a new migration needs a rebuild
  (add a `build.rs` with `println!("cargo:rerun-if-changed=migrations")` if sqlx doesn't do it).
- **`Option<Extension<_>>` extractor**: if not supported in the locked axum, use
  `request.extensions().get::<PgPool>()` via a small custom extractor, or keep `Extension` and
  register a fallback. Must not 500 when no DB is configured.
- **Shared DB in ignored tests**: tests run in parallel against one database; each test must
  create its own posts and assert on its own ids, never on table emptiness or exact counts.
- **Leaking DB details**: 500 responses must not echo `sqlx::Error` text to clients.
- **Request logging**: bodies of posts are logged when logging is on (existing, documented
  behaviour; truncated to 1 KiB) — acceptable, no secrets in posts.

## Testing

- **Unit tests** (`src/posts.rs`): `PostInput::validate` — ok case, empty title, whitespace-only
  title, 200-char title ok, 201-char title rejected (use multibyte chars to check char counting);
  `ApiError` → status code mapping and JSON body; `Post` serializes timestamps as RFC 3339.
- **Integration, no DB** (`tests/posts.rs`, always run): with plain `app()` every endpoint returns
  503 + `{"error":"database not configured"}` and JSON content type; `/health` still 200.
  Handlers check the pool first, so without a DB every `/posts` request (even with a bad id or
  body) returns 503.
- **Integration, real DB** (`#[ignore]`, `with_database(app(), pool)` after `db::connect` +
  `db::migrate` with the example config):
  - full CRUD flow: create (201, Location, fields), get (200), list contains it, update (200,
    changed fields, `updated_at >= created_at`, `created_at` unchanged), delete (204, empty body),
    get after delete (404), delete again (404), put on missing id (404).
  - validation: blank title → 422; 201-char title → 422; missing `body` → 400; unknown field → 400;
    invalid JSON → 400; no content type → 400; `/posts/abc` → 400; `/posts/0` → 400.
  - `migrate` twice is OK.
- Manual: `docker compose up -d`, `cargo run`, curl examples from README; and without
  `config/database.toml`, `curl /posts` → 503.
- Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
  `cargo test -- --ignored` (with Postgres).
