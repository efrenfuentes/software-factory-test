use axum::{Json, Router, routing::get};
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

pub fn app() -> Router {
    Router::new().route("/health", get(health))
}
