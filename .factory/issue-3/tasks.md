# Tasks — Issue #3: Add logs by requests

- [ ] T1 Add dependencies with `cargo add tracing tracing-subscriber`. Verify: `cargo build` succeeds and `Cargo.lock` is updated.
- [ ] T2 Create `src/logging.rs` with `pub const MAX_LOGGED_BODY_BYTES: usize = 1024` and `pub fn format_body(&[u8]) -> String`. It covers four cases: empty, text, truncation at a char boundary with `…(truncated, N bytes total)`, and `<N bytes of binary data>`. Add `#[cfg(test)]` unit tests for each case, including multibyte chars at the boundary. Register the module with `pub mod logging;` in `src/lib.rs`. Verify: `cargo test format_body` passes.
- [ ] T3 Implement `pub async fn log_requests(req: Request, next: Next) -> Response` in `src/logging.rs`. It buffers and rebuilds the request body, runs `next`, buffers and rebuilds the response body with the original parts, and emits one `tracing::info!` event with `method`, `uri` (path + query), `status`, `latency_ms`, `request_body` and `response_body` (via `format_body`, Debug-formatted). Handle buffering errors as the plan describes. Verify: `cargo build`.
- [ ] T4 In `src/lib.rs`, add `pub fn app_with_request_logging() -> Router` (`app().layer(axum::middleware::from_fn(logging::log_requests))`) and `pub fn request_logging_enabled(args, env: Option<&str>) -> bool`. The function returns true for the `--log-requests` flag, or for `LOG_REQUESTS` equal to `1`/`true`/`yes`, ignoring case. Add unit tests for it. Keep `app()` unchanged. Verify: `cargo test` passes, including the existing `tests/health.rs`.
- [ ] T5 Update `src/main.rs` to call `request_logging_enabled(std::env::args().skip(1), std::env::var("LOG_REQUESTS").ok().as_deref())`. If it returns true, init `tracing_subscriber::fmt().with_target(false)`, print `Request logging enabled`, and serve `app_with_request_logging()`; otherwise serve `app()`. Verify manually: with `cargo run -- --log-requests`, `curl localhost:3000/health` prints a timestamped line with `status=200`; plain `cargo run` prints nothing per request.
- [ ] T6 Add `tests/request_logging.rs` with a capturing `MakeWriter` and four tests:
  - `logs_get_health`: timestamp prefix, `method=GET`, `uri=/health?x=1`, `status=200`, and the response body;
  - `logs_request_body_and_405`: POST `hello` → `request_body="hello"` and `status=405`;
  - `logs_unknown_route_404`;
  - `response_unchanged_by_logging`: status, content-type and body are the same as with `app()`.

  Verify: `cargo test` passes. If the 404/405 tests fail because the layer does not wrap the fallback, apply the mitigation from the plan's Risks section.
- [ ] T7 Update `README.md` with a "Request logging" section: `cargo run -- --log-requests`, `LOG_REQUESTS=1 cargo run`, a sample log line, and notes that logging is off by default and bodies are truncated to 1 KiB. Verify: the README matches the real behaviour.
- [ ] T8 Quality gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` all pass.
