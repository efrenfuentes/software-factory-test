# Plan — Issue #3: Add logs by requests

## Approach
Add a custom Axum middleware (`axum::middleware::from_fn`). It:
1. records the start time (`std::time::Instant`), the method and the URI (path + query);
2. splits the request into `Parts` + body, buffers the body with
   `axum::body::to_bytes(body, usize::MAX)`, and rebuilds the request with
   `Request::from_parts(parts, Body::from(bytes.clone()))`;
3. calls `next.run(req).await`;
4. splits the response into parts + body, buffers the body the same way, and rebuilds the
   response with the original parts (status and headers such as `content-type` stay the same);
5. emits one `tracing::info!` event with fields `method`, `uri`, `status`, `latency_ms`,
   `request_body` and `response_body`, and returns the rebuilt response.

`tower_http::trace::TraceLayer` is not used because it does not log bodies. A small
hand-written middleware is simpler and easy to test.

Output goes through `tracing-subscriber`'s `fmt` subscriber. It prints a UTC RFC 3339
timestamp, the level and the fields, which covers the "date" requirement with no extra time
crate. Only the binary (`main.rs`) installs the subscriber. The library only emits events, so
tests can install their own capturing subscriber.

Buffering errors (stream failures, which the current handlers never produce):
- request body: return `500 Internal Server Error` without calling `next`, and still log the entry
  with `request_body="<unreadable body>"`;
- response body: log `response_body="<unreadable body>"` and return the original parts with an empty body.

### Body formatting
A pure helper `pub fn format_body(bytes: &[u8]) -> String` in the logging module, with
`pub const MAX_LOGGED_BODY_BYTES: usize = 1024;`:
- empty → empty string;
- valid UTF-8 and ≤ 1024 bytes → the text;
- valid UTF-8 and > 1024 bytes → the first 1024 bytes, moved back to the nearest char boundary
  (loop with `str::is_char_boundary`), followed by `…(truncated, N bytes total)`;
- invalid UTF-8 → `<N bytes of binary data>`.

Log the body fields Debug-formatted (`request_body = ?text`) so quotes and newlines are escaped
and each entry stays on one line. An empty body then prints as `""`.

### Enabling the option
In `src/lib.rs`, add `pub fn request_logging_enabled<I: IntoIterator<Item = String>>(args: I, env: Option<&str>) -> bool`.
It returns true if any arg equals `--log-requests`, or the env value, trimmed and lower-cased,
is `1`, `true` or `yes`. Because it is pure, it can be unit-tested without touching the process env.

`main.rs`:
```rust
let log = request_logging_enabled(std::env::args().skip(1), std::env::var("LOG_REQUESTS").ok().as_deref());
let router = if log {
    tracing_subscriber::fmt().with_target(false).init();
    println!("Request logging enabled");
    app_with_request_logging()
} else { app() };
```
Unknown CLI args are ignored, as they are today.

## Files to create or change
| File | Why |
|------|-----|
| `Cargo.toml` / `Cargo.lock` | Add `tracing` and `tracing-subscriber` (default features include `fmt`) with `cargo add`. Tests reuse them, so no extra dev-deps are needed. |
| `src/logging.rs` (new) | `log_requests` middleware, `format_body`, `MAX_LOGGED_BODY_BYTES`, plus `#[cfg(test)]` unit tests for `format_body`. |
| `src/lib.rs` | `pub mod logging;`. `app()` stays unchanged. New `pub fn app_with_request_logging() -> Router { app().layer(axum::middleware::from_fn(logging::log_requests)) }`. New `request_logging_enabled` with unit tests. |
| `src/main.rs` | Read the option, set up the subscriber, choose the router, print the notice. |
| `tests/request_logging.rs` (new) | Integration tests with captured log output (see Testing). |
| `README.md` | New "Request logging" section: how to turn it on (flag and env var), a sample line, and notes that it is off by default and bodies are truncated to 1 KiB. |

## API changes
HTTP endpoints and responses do not change. New public library items:
`app_with_request_logging`, `request_logging_enabled`, `logging::log_requests`,
`logging::format_body` and `logging::MAX_LOGGED_BODY_BYTES`. New runtime options: the CLI flag
`--log-requests` and the env var `LOG_REQUESTS`.

## Risks
- **Fallback coverage**: `Router::layer` should also wrap the default 404 fallback and the 405
  responses in Axum 0.8, and the 404/405 tests confirm it. If they don't, wrap the router as a
  whole service instead: `tower::Layer::layer(&from_fn(log_requests), app())` served via
  `ServiceExt::into_make_service`, or add an explicit `.fallback(...)` before `.layer`.
- **Memory**: whole bodies are buffered. That is acceptable because logging is opt-in and the API is tiny.
- **Streaming responses**: buffering would break streaming or SSE. None exist today; noted for future work.
- **Sensitive data in logs**: bodies are logged as-is. There is no sensitive data today, and redaction is out of scope.
- **Test log capture**: `tracing::subscriber::set_default` is thread-local. Use the default
  current-thread `#[tokio::test]` so events are emitted on the thread that holds the guard.
- **Global subscriber init**: `init()` panics if called twice. Only `main` calls it, never the library or tests.

## Testing
Unit tests (`src/logging.rs`, `src/lib.rs`):
- `format_body(b"")` → `""`; `format_body(b"hello")` → `"hello"`.
- A 2000-byte ASCII body → starts with 1024 `a`s and ends with `…(truncated, 2000 bytes total)`.
- A body of multibyte chars (e.g. `"é".repeat(600)`, 1200 bytes) does not panic, and the output is cut at a char boundary.
- `format_body(&[0xff, 0xfe])` → `<2 bytes of binary data>`.
- `request_logging_enabled`: the flag is present → true; env `1` / `true` / `YES` → true; none set, `0` or `false` → false.

Integration tests (`tests/request_logging.rs`). The capturing writer is a
`#[derive(Clone)] struct Capture(Arc<Mutex<Vec<u8>>>)` that implements `std::io::Write` and
`tracing_subscriber::fmt::MakeWriter`. Install it with
`let _g = tracing::subscriber::set_default(tracing_subscriber::fmt().with_writer(cap.clone()).with_ansi(false).finish());`.
1. `logs_get_health`: GET `/health?x=1` on `app_with_request_logging()` → the output contains
   `method=GET`, `uri=/health?x=1` and `status=200`, plus the escaped response body `{\"status\":\"ok\"}`.
   The line starts with a timestamp: the first 4 chars are digits and char 10 is `T`.
2. `logs_request_body_and_405`: POST `/health` with body `hello` → output contains `request_body="hello"` and `status=405`.
3. `logs_unknown_route_404`: GET `/nope` → output contains `uri=/nope` and `status=404`.
4. `response_unchanged_by_logging`: GET `/health` on `app_with_request_logging()` → status 200,
   `content-type: application/json`, body `{"status":"ok"}`.

`tests/health.rs` must keep passing untouched.
Quality gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
Manual check: `cargo run -- --log-requests`, then `curl localhost:3000/health` and
`curl -X POST -d hello localhost:3000/health`, and read the console output.
