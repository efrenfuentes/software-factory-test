# Issue #1 — Initialize an Axum API

## Problem
The repository is empty apart from `README.md` and a `mise.toml` that pins Rust `1.99.1`.
It needs a Rust project set up, plus an HTTP API built with Axum that has one health-check endpoint.

## Expected behaviour
- `cargo build` and `cargo run` work at the repository root.
- Once running, the server listens for HTTP requests (default `0.0.0.0:3000`).
- `GET /health` returns HTTP `200 OK` with header `Content-Type: application/json` and body:
  ```json
  {"status":"ok"}
  ```
- Any other path returns `404 Not Found` (Axum's default).

## Acceptance criteria
- [ ] A Cargo binary crate exists at the repository root (`Cargo.toml`, `Cargo.lock`, `src/main.rs`).
- [ ] `.gitignore` ignores `/target`.
- [ ] Dependencies: `axum`, `tokio` (with `full` or at least `macros` + `rt-multi-thread` + `net`), and `serde` / `serde_json` if needed for the JSON body.
- [ ] `GET /health` returns status 200, `Content-Type: application/json`, and a body that parses to `{"status":"ok"}`.
- [ ] The router is built by a function (`app()`) that tests can call without opening a network socket.
- [ ] An automated test checks the `/health` status, content type and body.
- [ ] An automated test checks that an unknown route returns 404.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` all pass.
- [ ] The README explains how to run the server and the tests, and shows a sample `curl` for `/health`.

## Out of scope
- Any endpoint other than `/health`.
- Databases, authentication, configuration files, structured logging/tracing setup, Docker, CI pipelines.
- Readiness/liveness checks of dependencies (the endpoint always answers `ok`).

## Assumptions
- Use the Rust toolchain already pinned in `mise.toml` (`1.99.1`), edition 2024.
- Use the latest stable Axum release (0.8.x or newer) with Tokio as the runtime.
- Only `GET` needs to work on `/health`; other methods get Axum's default `405 Method Not Allowed`.
- Bind address defaults to `0.0.0.0:3000`; the optional `PORT` environment variable overrides the port. This is a small convenience, not a requirement.
- The crate name is `software-factory-test`, matching the repository.
- Body serialization is compact (`{"status":"ok"}`); whitespace is not significant.
