use axum::{Json, Router, routing::get};
use serde::Serialize;

pub mod logging;

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

/// Same as [`app`], with one log entry per request (see [`logging::log_requests`]).
pub fn app_with_request_logging() -> Router {
    app().layer(axum::middleware::from_fn(logging::log_requests))
}

/// Request logging is on if `--log-requests` is among the args, or the env value
/// (`LOG_REQUESTS`) is `1`, `true` or `yes`, ignoring case.
pub fn request_logging_enabled<I: IntoIterator<Item = String>>(args: I, env: Option<&str>) -> bool {
    args.into_iter().any(|a| a == "--log-requests")
        || env.is_some_and(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::request_logging_enabled;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flag_enables() {
        assert!(request_logging_enabled(args(&["--log-requests"]), None));
    }

    #[test]
    fn env_values_enable() {
        for v in ["1", "true", "YES", " True "] {
            assert!(request_logging_enabled(args(&[]), Some(v)), "{v}");
        }
    }

    #[test]
    fn disabled_by_default() {
        assert!(!request_logging_enabled(args(&[]), None));
        assert!(!request_logging_enabled(args(&["--other"]), Some("0")));
        assert!(!request_logging_enabled(args(&[]), Some("false")));
        assert!(!request_logging_enabled(args(&[]), Some("")));
    }
}
