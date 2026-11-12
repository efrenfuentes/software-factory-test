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

## Run the tests

```sh
cargo test
```

Quality checks:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
