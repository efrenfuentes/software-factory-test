# software-factory-test

A minimal HTTP API built with [Axum](https://github.com/tokio-rs/axum). It exposes a single
health-check endpoint.

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

## Run the tests

```sh
cargo test
```

Quality checks:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
