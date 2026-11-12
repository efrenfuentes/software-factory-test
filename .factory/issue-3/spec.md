# Issue #3 — Add logs by requests

## Problem
When the API runs from the console (`cargo run`), it prints only the `Listening on …` line.
There is no way to see which requests arrive, what data they carry, or what the server
answers. The issue asks for an **option** that prints one log entry per request. Each
entry must show the date, the endpoint, the data sent to the endpoint, and the response
with its HTTP status code.

## Expected behaviour
- Request logging is **off by default**. The server behaves exactly as it does today.
- It is turned on by either of these:
  - the command-line flag `--log-requests` (e.g. `cargo run -- --log-requests`), or
  - the environment variable `LOG_REQUESTS` set to `1`, `true` or `yes` (case-insensitive),
    e.g. `LOG_REQUESTS=1 cargo run`.
- When it is on, the startup output says so: `Request logging enabled`.
- When it is on, the server writes **one line per handled request** to stdout once the
  response is produced. The line contains:
  - **date/time**: a UTC timestamp in RFC 3339 / ISO 8601 format, e.g. `2026-11-12T10:15:30.123456Z`;
  - **endpoint**: the HTTP method and the path, plus the query string if there is one (`method=GET uri=/health?x=1`);
  - **data passed**: the request body (`request_body=…`). The query string is already part of `uri`;
  - **response**: the HTTP status code (`status=200`) and the response body (`response_body=…`);
  - the request duration in milliseconds (`latency_ms=…`), as a small extra.
- Example (format is indicative):
  ```
  2026-11-12T10:15:30.123456Z  INFO method=GET uri=/health status=200 latency_ms=0 request_body="" response_body="{\"status\":\"ok\"}"
  ```
- Requests that match no route (404) and wrong methods (405) are logged too.
- Logging never changes what the client receives: status, headers and body are the same
  whether logging is on or off. This also holds when the request body stream fails while
  it is read. The handler is still called and returns what it would return without
  logging: `GET /health` still gives 200, and an unknown route still gives 404. A handler
  that reads the body sees the same error it would see without logging. The log line then
  shows `request_body="<unreadable body>"`.
- Body display rules in the log:
  - UTF-8 bodies are printed as text (quoted/escaped, so each entry stays on one line);
  - bodies longer than 1024 bytes are cut to 1024 bytes and marked `…(truncated, N bytes total)`;
  - bodies that are not UTF-8 are shown as `<N bytes of binary data>`;
  - an empty body is shown as `""`.

## Acceptance criteria
- [ ] Without `--log-requests` or `LOG_REQUESTS`, nothing new is printed per request, and the existing tests pass unchanged.
- [ ] `cargo run -- --log-requests` and `LOG_REQUESTS=1 cargo run` both turn on request logging.
- [ ] With logging on, `curl localhost:3000/health` prints one line with a UTC timestamp, `method=GET`, `uri=/health`, `status=200`, and the response body `{"status":"ok"}`.
- [ ] With logging on, `curl -X POST -d 'hello' localhost:3000/health` logs `request_body="hello"` and `status=405`.
- [ ] Unknown routes are logged with `status=404`.
- [ ] The query string appears in the logged `uri`.
- [ ] An automated test shows that the response the client gets (status, `content-type`, body) is identical with and without the logging middleware.
- [ ] An automated test captures the log output and checks the method, URI, status, request body and response body.
- [ ] Unit tests cover truncation of long bodies, including a limit that falls inside a multibyte character, and the binary-body placeholder.
- [ ] Regression tests send a failing request-body stream with logging on and with logging off, and get the same status and body both ways: 200 for `GET /health`, 404 for an unknown route, 405 for `POST /health`, and the same result for a handler that reads the body. The log shows `request_body="<unreadable body>"`.
- [ ] README documents the option and shows a sample log line.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass.

## Out of scope
- Logging request/response headers, client IP, or user agent.
- Writing logs to files, log rotation, JSON log format, or sending logs to external systems.
- Configurable log levels or filters beyond on/off (no `RUST_LOG` support needed).
- Redacting sensitive fields (the API has no auth or sensitive data today).
- Rejecting large bodies or other API behaviour changes.

## Assumptions
- "Option" means opt-in and off by default. Both a CLI flag and an env var are offered: the
  flag suits console use, and the env var matches the existing `PORT` convention.
- "Date" means a full UTC timestamp, which the `tracing-subscriber` formatter prints by default.
- "Data passed to the endpoint" means the request body plus the query string. Headers are left out.
- "Response" means the status code plus the response body (truncated as described above).
- Logs go to stdout via `tracing` / `tracing-subscriber`, the standard logging stack in the Axum ecosystem.
- The middleware buffers bodies fully in memory so it can log them. That is acceptable for
  this small API and only happens when logging is on.
