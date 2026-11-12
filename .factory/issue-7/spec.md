# Spec — Issue #7: Post resource

## Problem

The API only has `/health` and an optional PostgreSQL connection (issue #5). It has no
domain resource yet. We need a **Post** resource: a `posts` table in PostgreSQL and REST
endpoints that create, read, update and delete posts with JSON.

## Expected behaviour

### Data model

A post has:

| Field        | Type (JSON)          | Notes                                                   |
|--------------|----------------------|---------------------------------------------------------|
| `id`         | integer              | Assigned by the database (`BIGSERIAL`), read-only       |
| `title`      | string               | Required, not blank after trimming, at most 200 chars   |
| `body`       | string               | Required, may be empty                                  |
| `created_at` | string (RFC 3339 UTC)| Set by the database on insert, read-only                |
| `updated_at` | string (RFC 3339 UTC)| Set on insert, refreshed on every update, read-only     |

The table is created by a SQL migration in `migrations/`, applied automatically at startup
after the database connection succeeds.

### Endpoints

All request and response bodies are JSON (`Content-Type: application/json`).

| Method   | Path          | Request body          | Success                          |
|----------|---------------|-----------------------|----------------------------------|
| `POST`   | `/posts`      | `{"title","body"}`    | `201 Created`, the post, `Location: /posts/{id}` |
| `GET`    | `/posts`      | —                     | `200 OK`, JSON array of posts ordered by `id` ascending |
| `GET`    | `/posts/{id}` | —                     | `200 OK`, the post               |
| `PUT`    | `/posts/{id}` | `{"title","body"}`    | `200 OK`, the updated post       |
| `DELETE` | `/posts/{id}` | —                     | `204 No Content`, empty body     |

Errors always have a JSON body `{"error": "<message>"}`:

- `400 Bad Request` — body is not valid JSON, has the wrong shape, is missing `title` or
  `body`, has unknown fields, or `Content-Type` is not `application/json`; or `{id}` is not
  a positive 64-bit integer.
- `422 Unprocessable Entity` — `title` is blank or longer than 200 characters.
- `404 Not Found` — no post with that `id` (GET/PUT/DELETE).
- `503 Service Unavailable` — the server is running without a database
  (`{"error":"database not configured"}`).
- `500 Internal Server Error` — unexpected database error; the message is generic
  (`"internal server error"`), details go to stderr only.

Other methods on `/posts` or `/posts/{id}` return `405` (Axum default). `/health` is unchanged.

## Acceptance criteria

- [ ] A migration `migrations/<timestamp>_create_posts.sql` creates table `posts` with
      `id BIGSERIAL PRIMARY KEY`, `title TEXT NOT NULL`, `body TEXT NOT NULL`,
      `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`, `updated_at TIMESTAMPTZ NOT NULL DEFAULT now()`.
- [ ] Migrations run automatically at startup after a successful connection; failure prints
      an error and exits with status 1; startup prints `Database migrations applied`.
- [ ] Running the migrations twice is a no-op (sqlx migration tracking).
- [ ] `POST /posts` with a valid body returns 201, the created post and a `Location` header.
- [ ] `GET /posts` returns all posts as a JSON array ordered by `id`.
- [ ] `GET /posts/{id}` returns the post, or 404 if it does not exist.
- [ ] `PUT /posts/{id}` replaces `title` and `body`, bumps `updated_at`, keeps `created_at`;
      returns 404 if it does not exist.
- [ ] `DELETE /posts/{id}` returns 204, then `GET` on the same id returns 404; deleting a
      missing id returns 404.
- [ ] Validation and malformed-input errors return the status codes above with a JSON
      `{"error": ...}` body.
- [ ] Without a database configured, every `/posts` endpoint returns 503 with JSON error and
      the server does not crash; `/health` still returns 200.
- [ ] Request logging (`--log-requests`) works with the new endpoints.
- [ ] `cargo test` passes without PostgreSQL; `cargo test -- --ignored` passes with
      `docker compose up -d`.
- [ ] `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` pass.
- [ ] README documents the Post endpoints, the migration behaviour, and example `curl` calls.

## Out of scope

- Authentication, authorization, post ownership/authors.
- Pagination, filtering, sorting options, search on `GET /posts`.
- `PATCH` (partial update).
- Soft deletes, versioning, optimistic concurrency (ETags).
- A migration CLI or down migrations.
- OpenAPI/Swagger documentation.

## Assumptions

- Fields are `title` and `body` plus `id` and timestamps; the issue does not list fields, and
  these are the minimal useful set for a "post".
- `PUT` is a full replacement (both `title` and `body` required); `PATCH` is not provided.
- IDs are database-generated `BIGSERIAL` integers (no UUID dependency needed).
- Title limit is 200 characters (Unicode scalar values); body has no length limit.
- Migrations are embedded in the binary with `sqlx::migrate!` and applied on startup, since
  the repository has no migration tooling and the server already owns the DB lifecycle.
- Timestamps are serialized as RFC 3339 strings using `chrono` (sqlx `chrono` feature).
- Without a database the server keeps starting as today (issue #5 behaviour) and `/posts`
  answers 503 rather than being unregistered.
