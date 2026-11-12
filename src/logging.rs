use std::time::Instant;

use axum::{
    body::{Body, Bytes},
    extract::Request,
    middleware::Next,
    response::Response,
};
use futures_util::StreamExt;

/// Maximum number of body bytes shown in a log entry.
pub const MAX_LOGGED_BODY_BYTES: usize = 1024;

const UNREADABLE_BODY: &str = "<unreadable body>";

/// Formats a body for a log entry: text (truncated at a char boundary if too long),
/// or a placeholder for binary data.
pub fn format_body(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return format!("<{} bytes of binary data>", bytes.len());
    };
    if bytes.len() <= MAX_LOGGED_BODY_BYTES {
        return text.to_string();
    }
    let mut end = MAX_LOGGED_BODY_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…(truncated, {} bytes total)", &text[..end], bytes.len())
}

/// Reads a whole body. Returns the body to forward and the bytes read, or `None` if a read
/// error happened. On error the forwarded body replays the chunks read so far and then the
/// same error, so downstream code sees the failure it would have seen without logging.
async fn read_body(body: Body) -> (Body, Option<Bytes>) {
    let mut stream = body.into_data_stream();
    let mut chunks: Vec<Bytes> = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => chunks.push(chunk),
            Err(err) => {
                let replay = futures_util::stream::iter(
                    chunks.into_iter().map(Ok).chain(std::iter::once(Err(err))),
                );
                return (Body::from_stream(replay), None);
            }
        }
    }
    let bytes = Bytes::from(chunks.concat());
    (Body::from(bytes.clone()), Some(bytes))
}

fn describe(bytes: &Option<Bytes>) -> String {
    match bytes {
        Some(b) => format_body(b),
        None => UNREADABLE_BODY.to_string(),
    }
}

/// Middleware that logs one `tracing` info event per request.
pub async fn log_requests(req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let uri = req
        .uri()
        .path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_else(|| req.uri().path().to_string());

    let (parts, body) = req.into_parts();
    let (body, request_bytes) = read_body(body).await;
    let req = Request::from_parts(parts, body);

    let response = next.run(req).await;

    let (parts, body) = response.into_parts();
    let (body, response_bytes) = read_body(body).await;
    let response = Response::from_parts(parts, body);

    let request_body = describe(&request_bytes);
    let response_body = describe(&response_bytes);
    tracing::info!(
        method = %method,
        uri = %uri,
        status = response.status().as_u16(),
        latency_ms = start.elapsed().as_millis() as u64,
        request_body = ?request_body,
        response_body = ?response_body,
    );

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_body() {
        assert_eq!(format_body(b""), "");
    }

    #[test]
    fn short_text() {
        assert_eq!(format_body(b"hello"), "hello");
    }

    #[test]
    fn exactly_at_limit_is_not_truncated() {
        let s = "a".repeat(MAX_LOGGED_BODY_BYTES);
        assert_eq!(format_body(s.as_bytes()), s);
    }

    #[test]
    fn long_ascii_is_truncated() {
        let s = "a".repeat(2000);
        assert_eq!(
            format_body(s.as_bytes()),
            format!("{}…(truncated, 2000 bytes total)", "a".repeat(1024))
        );
    }

    #[test]
    fn limit_inside_two_byte_char() {
        let s = format!("a{}", "é".repeat(600));
        assert_eq!(s.len(), 1201);
        assert_eq!(
            format_body(s.as_bytes()),
            format!("a{}…(truncated, 1201 bytes total)", "é".repeat(511))
        );
    }

    #[test]
    fn limit_inside_three_byte_char() {
        let s = "€".repeat(400);
        assert_eq!(s.len(), 1200);
        assert_eq!(
            format_body(s.as_bytes()),
            format!("{}…(truncated, 1200 bytes total)", "€".repeat(341))
        );
    }

    #[test]
    fn limit_on_char_boundary() {
        let s = "é".repeat(600);
        assert_eq!(
            format_body(s.as_bytes()),
            format!("{}…(truncated, 1200 bytes total)", "é".repeat(512))
        );
    }

    #[test]
    fn binary_body() {
        assert_eq!(format_body(&[0xff, 0xfe]), "<2 bytes of binary data>");
    }
}
