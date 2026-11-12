use software_factory_test::{
    app, app_with_request_logging,
    db::{self, ConfigSource, DatabaseConfig},
    request_logging_enabled, with_database,
};
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
    let mut router = if log_requests {
        tracing_subscriber::fmt().with_target(false).init();
        println!("Request logging enabled");
        app_with_request_logging()
    } else {
        app()
    };
    if let Some(pool) = connect_database().await {
        router = with_database(router, pool);
    }
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("failed to bind listener");
    println!(
        "Listening on {}",
        listener.local_addr().expect("failed to read local address")
    );
    axum::serve(listener, router).await.expect("server error");
}

/// Connects to PostgreSQL if a config file is present. Exits the process on any error.
async fn connect_database() -> Option<sqlx::PgPool> {
    let path = match db::config_source(std::env::var("DATABASE_CONFIG").ok().as_deref()) {
        ConfigSource::Default(path) if !path.exists() => {
            println!(
                "No database configuration found at {}; running without a database",
                path.display()
            );
            return None;
        }
        ConfigSource::Default(path) | ConfigSource::Explicit(path) => path,
    };
    let result = async {
        let config = DatabaseConfig::load(&path)?
            .with_password_override(std::env::var("DATABASE_PASSWORD").ok());
        let pool = db::connect(&config).await?;
        Ok::<_, db::DbError>((pool, config.display_target()))
    }
    .await;
    match result {
        Ok((pool, target)) => {
            println!("Connected to PostgreSQL at {target}");
            Some(pool)
        }
        Err(err) => {
            eprintln!("Database error: {err}");
            std::process::exit(1);
        }
    }
}
