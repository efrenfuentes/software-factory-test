# Tasks — Issue #1: Initialize an Axum API

- [x] T1 Create the Cargo crate at the repo root: `Cargo.toml` (name `software-factory-test`, edition 2024, `rust-version = "1.99"`), a placeholder `src/main.rs`, and a `.gitignore` containing `/target`. Verify: `cargo build` succeeds and `target/` does not appear in `git status`.
- [x] T2 Add dependencies with `cargo add`: `axum`, `tokio --features macros,rt-multi-thread,net`, `serde --features derive`. Add dev-deps: `tower --features util`, `http-body-util`, `serde_json`. Verify: `cargo build` succeeds and `Cargo.lock` is created.
- [x] T3 Write `src/lib.rs` with a `HealthResponse` struct (Serialize), an async `health` handler that returns `Json(HealthResponse { status: "ok" })`, and `pub fn app() -> Router` that routes `GET /health`. Verify: `cargo build`.
- [x] T4 Write `src/main.rs`: a Tokio main that reads `PORT` (default 3000), binds a `TcpListener` on `0.0.0.0`, prints the address and serves `app()`. Verify: `cargo run`, then `curl -i localhost:3000/health` shows `200` and `{"status":"ok"}`.
- [x] T5 Add `tests/health.rs` with `health_returns_ok_json` (checks status 200, `content-type: application/json`, and that the body equals `{"status":"ok"}`) and `unknown_route_returns_404`. Verify: `cargo test` passes.
- [x] T6 Update `README.md` with prerequisites (mise / Rust 1.99.1), how to run the server, the `PORT` variable, how to run the tests, and a sample `curl`. Verify: the README is accurate.
- [x] T7 Quality gates: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean, and `cargo test` passes.
