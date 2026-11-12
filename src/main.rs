use software_factory_test::{app, app_with_request_logging, request_logging_enabled};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let log_requests = request_logging_enabled(
        std::env::args().skip(1),
        std::env::var("LOG_REQUESTS").ok().as_deref(),
    );
    let router = if log_requests {
        tracing_subscriber::fmt().with_target(false).init();
        println!("Request logging enabled");
        app_with_request_logging()
    } else {
        app()
    };
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("failed to bind listener");
    println!(
        "Listening on {}",
        listener.local_addr().expect("failed to read local address")
    );
    axum::serve(listener, router).await.expect("server error");
}
