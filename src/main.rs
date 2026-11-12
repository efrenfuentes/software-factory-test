use software_factory_test::app;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("failed to bind listener");
    println!(
        "Listening on {}",
        listener.local_addr().expect("failed to read local address")
    );
    axum::serve(listener, app()).await.expect("server error");
}
