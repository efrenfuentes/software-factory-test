use std::io;
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::{Body, Bytes},
    http::{Request, StatusCode, header},
    routing::post,
};
use http_body_util::BodyExt;
use software_factory_test::{app, app_with_request_logging, logging::log_requests};
use tower::ServiceExt;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn output(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn capture() -> (Capture, tracing::subscriber::DefaultGuard) {
    let cap = Capture::default();
    let guard = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_writer(cap.clone())
            .with_ansi(false)
            .finish(),
    );
    (cap, guard)
}

async fn send(router: Router, req: Request<Body>) -> (StatusCode, Option<String>, Bytes) {
    let response = router.oneshot(req).await.unwrap();
    let status = response.status();
    let ct = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, ct, bytes)
}

#[tokio::test]
async fn logs_get_health() {
    let (cap, _g) = capture();
    let (status, _, _) = send(
        app_with_request_logging(),
        Request::get("/health?x=1").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let out = cap.output();
    let first = out.chars().take(4).collect::<String>();
    assert!(first.chars().all(|c| c.is_ascii_digit()), "{out}");
    assert_eq!(out.chars().nth(10), Some('T'), "{out}");
    assert!(out.contains("method=GET"), "{out}");
    assert!(out.contains("uri=/health?x=1"), "{out}");
    assert!(out.contains("status=200"), "{out}");
    assert!(
        out.contains(r#"response_body="{\"status\":\"ok\"}""#),
        "{out}"
    );
    assert_eq!(out.lines().count(), 1, "{out}");
}

#[tokio::test]
async fn logs_request_body_and_405() {
    let (cap, _g) = capture();
    let (status, _, _) = send(
        app_with_request_logging(),
        Request::post("/health").body(Body::from("hello")).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    let out = cap.output();
    assert!(out.contains(r#"request_body="hello""#), "{out}");
    assert!(out.contains("status=405"), "{out}");
}

#[tokio::test]
async fn logs_unknown_route_404() {
    let (cap, _g) = capture();
    let (status, _, _) = send(
        app_with_request_logging(),
        Request::get("/nope").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let out = cap.output();
    assert!(out.contains("uri=/nope"), "{out}");
    assert!(out.contains("status=404"), "{out}");
}

#[tokio::test]
async fn response_unchanged_by_logging() {
    let (_cap, _g) = capture();
    let req = || Request::get("/health").body(Body::empty()).unwrap();
    let plain = send(app(), req()).await;
    let logged = send(app_with_request_logging(), req()).await;
    assert_eq!(plain, logged);
    assert_eq!(logged.0, StatusCode::OK);
    assert_eq!(logged.1.as_deref(), Some("application/json"));
    assert_eq!(logged.2, Bytes::from_static(br#"{"status":"ok"}"#));
}

fn failing_body() -> Body {
    Body::from_stream(futures_util::stream::iter(vec![
        Ok::<_, io::Error>(Bytes::from_static(b"par")),
        Err(io::Error::other("boom")),
    ]))
}

async fn assert_same_both_ways(
    plain: Router,
    logged: Router,
    method: &str,
    uri: &str,
    expected: StatusCode,
) -> (StatusCode, Option<String>, Bytes) {
    let req = || {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(failing_body())
            .unwrap()
    };
    let a = send(plain, req()).await;
    let b = send(logged, req()).await;
    assert_eq!(a, b);
    assert_eq!(a.0, expected);
    a
}

#[tokio::test]
async fn failing_body_get_health_unchanged() {
    let (cap, _g) = capture();
    let (_, _, body) = assert_same_both_ways(
        app(),
        app_with_request_logging(),
        "GET",
        "/health",
        StatusCode::OK,
    )
    .await;
    assert_eq!(body, Bytes::from_static(br#"{"status":"ok"}"#));
    let out = cap.output();
    assert!(out.contains(r#"request_body="<unreadable body>""#), "{out}");
    assert!(out.contains("status=200"), "{out}");
}

#[tokio::test]
async fn failing_body_unknown_route_unchanged() {
    let (cap, _g) = capture();
    assert_same_both_ways(
        app(),
        app_with_request_logging(),
        "GET",
        "/nope",
        StatusCode::NOT_FOUND,
    )
    .await;
    let out = cap.output();
    assert!(out.contains(r#"request_body="<unreadable body>""#), "{out}");
    assert!(out.contains("status=404"), "{out}");
}

#[tokio::test]
async fn failing_body_405_unchanged() {
    let (cap, _g) = capture();
    assert_same_both_ways(
        app(),
        app_with_request_logging(),
        "POST",
        "/health",
        StatusCode::METHOD_NOT_ALLOWED,
    )
    .await;
    let out = cap.output();
    assert!(out.contains(r#"request_body="<unreadable body>""#), "{out}");
    assert!(out.contains("status=405"), "{out}");
}

#[tokio::test]
async fn failing_body_reading_handler_unchanged() {
    let (cap, _g) = capture();
    let echo = || Router::new().route("/echo", post(|b: Bytes| async move { b }));
    let logged = echo().layer(axum::middleware::from_fn(log_requests));
    assert_same_both_ways(echo(), logged, "POST", "/echo", StatusCode::BAD_REQUEST).await;
    let out = cap.output();
    assert!(out.contains(r#"request_body="<unreadable body>""#), "{out}");
    assert!(out.contains("status=400"), "{out}");
}
