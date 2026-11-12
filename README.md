# software-factory-test

A minimal HTTP API built with [Axum](https://github.com/tokio-rs/axum). It exposes a
health-check endpoint and a Post resource stored in PostgreSQL.

## Prerequisites

- [mise](https://mise.jdx.dev/) — run `mise install` to get the pinned Rust toolchain (1.99.1).
  Any recent stable Rust that supports edition 2024 also works.

## Run the server

```sh
cargo run
```

The server listens on `0.0.0.0:3000` by default. Set the `PORT` environment variable to use a
different port:

```sh
PORT=8080 cargo run
```

## Try it

```sh
curl -i http://localhost:3000/health
```

Expected response: `200 OK` with `Content-Type: application/json` and body `{"status":"ok"}`.
Any other path returns `404 Not Found`.

## Request logging

Request logging is off by default. Turn it on with a flag or an environment variable:

```sh
cargo run -- --log-requests
LOG_REQUESTS=1 cargo run
```

`LOG_REQUESTS` accepts `1`, `true` or `yes` (case-insensitive). When on, the server prints
`Request logging enabled` at startup and one line per request, including 404 and 405 responses:

```
2026-11-12T10:15:30.123456Z  INFO method=GET uri=/health status=200 latency_ms=0 request_body="" response_body="{\"status\":\"ok\"}"
```

Each line has a UTC timestamp, the method, the path and query string, the status code, the
duration, and the request and response bodies. Bodies are truncated to 1 KiB
(`…(truncated, N bytes total)`), and bodies that are not UTF-8 show as `<N bytes of binary data>`.
Logging never changes the response the client receives.

## Database (PostgreSQL)

The server can connect to PostgreSQL. The connection settings live in a TOML file,
`config/database.toml` by default. This file is git-ignored, so real credentials are never
committed. `config/database.example.toml` documents every key.

Start a local PostgreSQL (requires Docker) and use the example settings:

```sh
docker compose up -d
cp config/database.example.toml config/database.toml
cargo run
```

The server prints `Connected to PostgreSQL at localhost:5432/software_factory`, then
`Database migrations applied`.

All keys go under a `[postgres]` table:

| Key                    | Required | Default  | Description                                           |
|------------------------|----------|----------|-------------------------------------------------------|
| `host`                 | yes      |          | Server host name or IP address                        |
| `port`                 | no       | `5432`   | Server port                                           |
| `user`                 | yes      |          | User name                                             |
| `password`             | no       | none     | Password (see `DATABASE_PASSWORD`)                    |
| `database`             | yes      |          | Database name                                         |
| `ssl_mode`             | no       | `prefer` | `disable`, `prefer` or `require`                      |
| `max_connections`      | no       | `5`      | Maximum connections in the pool                       |
| `connect_timeout_secs` | no       | `5`      | Time limit for the startup check (connect + `SELECT 1`) |

Unknown keys are reported as errors.

Environment variables:

- `DATABASE_CONFIG` — path to a different config file, for example
  `DATABASE_CONFIG=/etc/app/database.toml cargo run`.
- `DATABASE_PASSWORD` — if set, it replaces `password` from the file, so the password does
  not have to be written to disk. An empty value (`DATABASE_PASSWORD=`) means an empty
  password.

At startup:

- If the config file exists, the server parses it, connects, and runs `SELECT 1`. Then it
  prints `Connected to PostgreSQL at <host>:<port>/<database>`.
- After connecting, it applies the SQL migrations in `migrations/` (embedded in the binary at
  build time) and prints `Database migrations applied`. Migrations that were already applied
  are skipped (sqlx records them in the `_sqlx_migrations` table), so restarting is safe. If
  a migration fails, the server prints `Database error: ...` and exits with status 1.
- If the file is invalid, or the database cannot be reached or does not answer within
  `connect_timeout_secs`, the server prints an error that names the file or host and exits
  with status 1. The password is never printed.
- If `DATABASE_CONFIG` is set and the file does not exist, the server exits with an error.
- If `DATABASE_CONFIG` is not set and `config/database.toml` does not exist, the server
  prints `No database configuration found at config/database.toml; running without a database`
  and starts without a database.

`config/database.toml` is resolved relative to the working directory. Run the server from the
repository root, or set `DATABASE_CONFIG` to an absolute path.

## Posts API

A post has these fields:

| Field        | Type                 | Notes                                               |
|--------------|----------------------|-----------------------------------------------------|
| `id`         | integer              | Assigned by the database, read-only                 |
| `title`      | string               | Required, not blank, at most 200 characters         |
| `body`       | string               | Required, may be empty                              |
| `created_at` | string (RFC 3339 UTC)| Set on insert, read-only                            |
| `updated_at` | string (RFC 3339 UTC)| Set on insert and on every update, read-only        |

Request and response bodies are JSON. Send `Content-Type: application/json` with a body.

| Method   | Path          | Body                 | Success                                              |
|----------|---------------|----------------------|------------------------------------------------------|
| `POST`   | `/posts`      | `{"title","body"}`   | `201 Created`, the post, `Location: /posts/{id}`     |
| `GET`    | `/posts`      |                      | `200 OK`, all posts as an array, ordered by `id`     |
| `GET`    | `/posts/{id}` |                      | `200 OK`, the post                                   |
| `PUT`    | `/posts/{id}` | `{"title","body"}`   | `200 OK`, the updated post (both fields replaced)    |
| `DELETE` | `/posts/{id}` |                      | `204 No Content`, empty body                         |

Errors have the body `{"error": "<message>"}`:

- `400 Bad Request` — the body is not valid JSON, is missing `title` or `body`, has unknown
  fields or wrong types, or `Content-Type` is not `application/json`; or `{id}` is not a
  positive integer.
- `422 Unprocessable Entity` — `title` is blank or longer than 200 characters.
- `404 Not Found` — no post with that id.
- `503 Service Unavailable` — the server runs without a database
  (`{"error":"database not configured"}`).
- `500 Internal Server Error` — unexpected database error. The response says only
  `internal server error`; the details go to stderr.

Other methods return `405 Method Not Allowed`.

Examples:

```sh
curl -i -X POST http://localhost:3000/posts \
  -H 'Content-Type: application/json' \
  -d '{"title":"Hello","body":"First post"}'
curl -i http://localhost:3000/posts
curl -i http://localhost:3000/posts/1
curl -i -X PUT http://localhost:3000/posts/1 \
  -H 'Content-Type: application/json' \
  -d '{"title":"Hello again","body":"Edited"}'
curl -i -X DELETE http://localhost:3000/posts/1
```

## Run the tests

```sh
cargo test
```

`cargo test` needs no database. The tests that use a real PostgreSQL (the connection check
and the Posts API tests, using `config/database.example.toml`) are ignored by default. They
create and delete their own posts, so they can run against a database that already has data.
Run them with PostgreSQL running:

```sh
docker compose up -d
cargo test -- --ignored
```

Quality checks:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
