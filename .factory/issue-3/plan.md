# Plan — Issue #3: Add logs by requests

## Approach
Add a custom Axum middleware (`axum::middleware::from_fn`). It:
1. records the start time (`std::time::Instant`), the method and the URI (path + query);
2. splits the request into `Parts` + body and reads the body chunk by chunk with
   `Body::into_data_stream()` and `futures_util::StreamExt::next`, keeping the chunks in a
   `Vec<Bytes>`:
   - if every chunk is read, it rebuilds the request with
     `Request::from_parts(parts, Body::from(concatenated_bytes))`;
   - if a chunk read returns `Err(e)`, it stops reading and rebuilds the request with a
     **replay body**. That body yields the chunks already read and then the same error:
     `Body::from_stream(futures_util::stream::iter(chunks.into_iter().map(Ok).chain(std::iter::once(Err(e)))))`;
3. **always** calls `next.run(req).await`, even after a request-body read error. It never
   returns early. Handlers that ignore the body (all current ones, plus the 404/405
   fallbacks) return exactly what they return without logging. Handlers that read the body
   get the same error at the same point, so extractors such as `Bytes` reject it the same way;
4. splits the response into parts + body, reads the body the same way (replaying on error), and rebuilds the
   response with the original parts (status and headers such as `content-type` stay the same);
5. emits one `tracing::info!` event with fields `method`, `uri`, `status`, `latency_ms`,
   `request_body` and `response_body`, and returns the rebuilt response.

`tower_http::trace::TraceLayer` is not used because it does not log bodies. A small
hand-written middleware is simpler and easy to test.

Output goes through `tracing-subscriber`'s `fmt` subscriber. It prints a UTC RFC 3339
timestamp, the level and the fields, which covers the "date" requirement with no extra time
crate. Only the binary (`main.rs`) installs the subscriber. The library only emits events, so
tests can install their own capturing subscriber.

Body read errors (stream failures, which the current clients and handlers never produce).
The rule is that logging must never change the outcome:
- request body: never short-circuit, and never answer 500 on its own. Forward the replay
  body from step 2, call `next` as usual, and log `request_body="<unreadable body>"` together
  with the real status and response body that the handler produced;
- response body: same idea. On `Err(e)`, log `response_body="<unreadable body>"` and return the
  original parts with a replay body (the chunks read so far, then the same error), so the
  client sees the same failure it would see without logging.

One private helper serves both directions:
`async fn read_body(body: Body) -> (Body, Option<Bytes>)`. It returns the body to forward
(rebuilt or replayed), plus `Some(bytes)` on success or `None` on a read error. The logged
value is `format_body(&bytes)`, or `"<unreadable body>"` for `None`.

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
| `Cargo.toml` / `Cargo.lock` | Add `tracing`, `tracing-subscriber` (default features include `fmt`) and `futures-util` with `cargo add`. `futures-util` is already in `Cargo.lock` through axum; it is needed for `StreamExt::next` and `stream::iter` in the replay body. Tests reuse them, so no extra dev-deps are needed. |
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
- **Trailers**: reading through `into_data_stream()` drops HTTP trailers, as `to_bytes` would.
  No current client or handler uses them, so this is a known limitation.
- **Replay fidelity**: the replayed error is the original `axum::Error`, boxed again by
  `Body::from_stream`. Body extractors such as `Bytes` map any body error to the same
  rejection (`FailedToBufferBody`, 400). The regression test with a body-reading handler
  confirms that status and body match with logging on and off.
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
- Limit falls **inside** a 2-byte char: `"a"` + `"é".repeat(600)` (1201 bytes). Byte 1024 is the
  second byte of an `é`, so the text part must be `"a"` + 511 `é` (1023 bytes), followed by
  `…(truncated, 1201 bytes total)`, with no panic.
- Limit falls inside a 3-byte char: `"€".repeat(400)` (1200 bytes) → 341 `€` (1023 bytes) + marker.
- Limit falls exactly on a boundary: `"é".repeat(600)` (1200 bytes) → 512 `é` (1024 bytes) + marker.
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

Failing request-body regression tests (same file). Helper:
`fn failing_body() -> Body { Body::from_stream(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from_static(b"par")), Err(std::io::Error::other("boom"))])) }`.
Each test sends the same request, with a fresh `failing_body()`, to the plain router and to
the logging router, and asserts that the status and the response body bytes are equal:
5. `failing_body_get_health_unchanged`: GET `/health` → 200 and `{"status":"ok"}` both ways; the
   log contains `request_body="<unreadable body>"` and `status=200`.
6. `failing_body_unknown_route_unchanged`: GET `/nope` → 404 both ways; log has `status=404`.
7. `failing_body_405_unchanged`: POST `/health` → 405 both ways.
8. `failing_body_reading_handler_unchanged`: test-only router
   `Router::new().route("/echo", post(|b: Bytes| async move { b }))`, with and without
   `.layer(from_fn(log_requests))` → same status (expected 400) and same body both ways.

`tests/health.rs` must keep passing untouched.
Quality gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
Manual check: `cargo run -- --log-requests`, then `curl localhost:3000/health` and
`curl -X POST -d hello localhost:3000/health`, and read the console output.
