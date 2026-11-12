use std::path::Path;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use software_factory_test::{
    app,
    db::{self, DatabaseConfig},
    with_database,
};
use tower::ServiceExt;

struct Reply {
    status: StatusCode,
    content_type: Option<String>,
    location: Option<String>,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", String::from_utf8_lossy(&self.body)))
    }

    /// Asserts an error status with a JSON `{"error": "..."}` body.
    fn assert_error(&self, status: StatusCode) {
        assert_eq!(
            self.status,
            status,
            "{:?}",
            String::from_utf8_lossy(&self.body)
        );
        assert_eq!(self.content_type.as_deref(), Some("application/json"));
        assert!(self.json()["error"].is_string(), "{}", self.json());
    }
}

async fn send(router: &Router, method: Method, uri: &str, body: Option<&str>) -> Reply {
    send_with_type(router, method, uri, body, Some("application/json")).await
}

async fn send_with_type(
    router: &Router,
    method: Method,
    uri: &str,
    body: Option<&str>,
    content_type: Option<&str>,
) -> Reply {
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some()
        && let Some(content_type) = content_type
    {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    let request = request
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let header_value = |name| {
        response
            .headers()
            .get(name)
            .map(|v: &header::HeaderValue| v.to_str().unwrap().to_string())
    };
    Reply {
        status: response.status(),
        content_type: header_value(header::CONTENT_TYPE),
        location: header_value(header::LOCATION),
        body: response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    }
}

const VALID: &str = r#"{"title":"Hello","body":"World"}"#;

#[tokio::test]
async fn without_database_every_endpoint_is_503() {
    let router = app();
    let cases = [
        (Method::POST, "/posts", Some(VALID)),
        (Method::GET, "/posts", None),
        (Method::GET, "/posts/1", None),
        (Method::PUT, "/posts/1", Some(VALID)),
        (Method::DELETE, "/posts/1", None),
        // The pool is checked first, so bad input still gives 503.
        (Method::GET, "/posts/abc", None),
        (Method::POST, "/posts", Some("not json")),
    ];
    for (method, uri, body) in cases {
        let reply = send(&router, method.clone(), uri, body).await;
        assert_eq!(
            reply.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{method} {uri}"
        );
        assert_eq!(reply.content_type.as_deref(), Some("application/json"));
        assert_eq!(reply.json(), json!({"error": "database not configured"}));
    }
}

#[tokio::test]
async fn health_still_ok_and_other_methods_405() {
    let router = app();
    assert_eq!(
        send(&router, Method::GET, "/health", None).await.status,
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::PATCH, "/posts/1", Some(VALID))
            .await
            .status,
        StatusCode::METHOD_NOT_ALLOWED
    );
}

async fn db_router() -> Router {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/database.example.toml");
    let config = DatabaseConfig::load(&path)
        .unwrap()
        .with_password_override(std::env::var("DATABASE_PASSWORD").ok());
    let pool = db::connect(&config).await.unwrap();
    db::migrate(&pool).await.unwrap();
    with_database(app(), pool)
}

#[tokio::test]
#[ignore = "needs a running PostgreSQL (docker compose up -d)"]
async fn migrate_twice_is_ok() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/database.example.toml");
    let config = DatabaseConfig::load(&path)
        .unwrap()
        .with_password_override(std::env::var("DATABASE_PASSWORD").ok());
    let pool = db::connect(&config).await.unwrap();
    db::migrate(&pool).await.unwrap();
    db::migrate(&pool).await.unwrap();
}

#[tokio::test]
#[ignore = "needs a running PostgreSQL (docker compose up -d)"]
async fn crud_flow() {
    let router = db_router().await;

    let created = send(&router, Method::POST, "/posts", Some(VALID)).await;
    assert_eq!(created.status, StatusCode::CREATED);
    let post = created.json();
    let id = post["id"].as_i64().unwrap();
    let uri = format!("/posts/{id}");
    assert_eq!(created.location.as_deref(), Some(uri.as_str()));
    assert_eq!(post["title"], "Hello");
    assert_eq!(post["body"], "World");
    assert!(post["created_at"].is_string());
    assert!(post["updated_at"].is_string());

    let fetched = send(&router, Method::GET, &uri, None).await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.json(), post);

    let listed = send(&router, Method::GET, "/posts", None).await;
    assert_eq!(listed.status, StatusCode::OK);
    let ids: Vec<i64> = listed
        .json()
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    assert!(ids.contains(&id));
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");

    let updated = send(
        &router,
        Method::PUT,
        &uri,
        Some(r#"{"title":"Changed","body":""}"#),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK);
    let updated = updated.json();
    assert_eq!(updated["id"], id);
    assert_eq!(updated["title"], "Changed");
    assert_eq!(updated["body"], "");
    assert_eq!(updated["created_at"], post["created_at"]);
    let ts = |v: &Value| chrono::DateTime::parse_from_rfc3339(v.as_str().unwrap()).unwrap();
    assert!(ts(&updated["updated_at"]) >= ts(&updated["created_at"]));
    assert!(ts(&updated["updated_at"]) >= ts(&post["updated_at"]));

    let deleted = send(&router, Method::DELETE, &uri, None).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert!(deleted.body.is_empty());

    send(&router, Method::GET, &uri, None)
        .await
        .assert_error(StatusCode::NOT_FOUND);
    send(&router, Method::DELETE, &uri, None)
        .await
        .assert_error(StatusCode::NOT_FOUND);
    send(&router, Method::PUT, &uri, Some(VALID))
        .await
        .assert_error(StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs a running PostgreSQL (docker compose up -d)"]
async fn invalid_input() {
    let router = db_router().await;
    let long_title = format!(r#"{{"title":"{}","body":""}}"#, "a".repeat(201));

    for body in [r#"{"title":"  ","body":"x"}"#, long_title.as_str()] {
        send(&router, Method::POST, "/posts", Some(body))
            .await
            .assert_error(StatusCode::UNPROCESSABLE_ENTITY);
    }

    for body in [
        r#"{"title":"t"}"#,
        r#"{"title":"t","body":"b","extra":1}"#,
        r#"{"title":1,"body":"b"}"#,
        "not json",
    ] {
        send(&router, Method::POST, "/posts", Some(body))
            .await
            .assert_error(StatusCode::BAD_REQUEST);
    }
    send_with_type(&router, Method::POST, "/posts", Some(VALID), None)
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    send_with_type(
        &router,
        Method::POST,
        "/posts",
        Some(VALID),
        Some("text/plain"),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST);

    for uri in [
        "/posts/abc",
        "/posts/0",
        "/posts/-1",
        "/posts/99999999999999999999",
    ] {
        send(&router, Method::GET, uri, None)
            .await
            .assert_error(StatusCode::BAD_REQUEST);
    }

    // Validation on PUT, against a post this test owns.
    let created = send(&router, Method::POST, "/posts", Some(VALID)).await;
    assert_eq!(created.status, StatusCode::CREATED);
    let uri = created.location.clone().unwrap();
    send(
        &router,
        Method::PUT,
        &uri,
        Some(r#"{"title":"","body":""}"#),
    )
    .await
    .assert_error(StatusCode::UNPROCESSABLE_ENTITY);
    send(&router, Method::PUT, &uri, Some("{"))
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    send(&router, Method::DELETE, &uri, None).await;
}
