use std::{path::Path, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use software_factory_test::{
    app,
    db::{self, DatabaseConfig, DbError, SslMode},
    with_database,
};
use tower::ServiceExt;

fn example_config() -> DatabaseConfig {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/database.example.toml");
    DatabaseConfig::load(&path).unwrap()
}

#[tokio::test]
#[ignore = "needs a running PostgreSQL (docker compose up -d)"]
async fn connects_to_real_postgres() {
    let config = example_config().with_password_override(std::env::var("DATABASE_PASSWORD").ok());
    let pool = db::connect(&config).await.unwrap();

    let one: i32 = sqlx::query_scalar("SELECT 1::int4")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(one, 1);

    let response = with_database(app(), pool)
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn unreachable_server_gives_connect_error_without_password() {
    // Bind and drop a listener to get a port nobody listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = DatabaseConfig {
        host: "127.0.0.1".into(),
        port,
        password: Some("s3cr3t-pw".into()),
        ssl_mode: SslMode::Disable,
        connect_timeout_secs: 2,
        ..example_config()
    };

    let err = tokio::time::timeout(Duration::from_secs(10), db::connect(&config))
        .await
        .expect("connect did not respect its timeout")
        .unwrap_err();

    // sqlx retries refused connections until its acquire timeout, which equals the outer
    // deadline, so either variant may win; both name the target.
    assert!(
        matches!(err, DbError::Connect { .. } | DbError::Timeout { .. }),
        "{err:?}"
    );
    let message = err.to_string();
    assert!(message.contains(&format!("127.0.0.1:{port}")), "{message}");
    assert!(!message.contains("s3cr3t-pw"), "{message}");
    assert!(!format!("{err:?}").contains("s3cr3t-pw"));
}
