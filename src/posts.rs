//! The Post resource: model, validation, errors and the `/posts` routes.

use axum::{
    Extension, Json, Router,
    extract::{
        DefaultBodyLimit, Path,
        rejection::{JsonRejection, PathRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};
use sqlx::PgPool;
use std::fmt;

/// Maximum title length, in characters (Unicode scalar values).
pub const MAX_TITLE_CHARS: usize = 200;

/// A stored post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request body of `POST /posts` and `PUT /posts/{id}`.
///
/// Only a JSON object is accepted: unlike a derived `Deserialize`, positional arrays
/// such as `["title", "body"]` are rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostInput {
    pub title: String,
    pub body: String,
}

/// The fields of [`PostInput`], deserialized from the map handed over by its visitor.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PostInputFields {
    title: String,
    body: String,
}

impl<'de> Deserialize<'de> for PostInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;

        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = PostInput;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object with \"title\" and \"body\"")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<PostInput, A::Error> {
                let PostInputFields { title, body } =
                    PostInputFields::deserialize(MapAccessDeserializer::new(map))?;
                Ok(PostInput { title, body })
            }
        }

        deserializer.deserialize_map(ObjectVisitor)
    }
}

impl PostInput {
    /// Checks that the title is not blank and at most [`MAX_TITLE_CHARS`] characters.
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.title.trim().is_empty() {
            return Err(ApiError::Unprocessable("title must not be blank".into()));
        }
        if self.title.chars().count() > MAX_TITLE_CHARS {
            return Err(ApiError::Unprocessable(format!(
                "title must be at most {MAX_TITLE_CHARS} characters"
            )));
        }
        Ok(())
    }
}

/// Errors returned by the `/posts` handlers, rendered as `{"error": "<message>"}`.
#[derive(Debug)]
pub enum ApiError {
    /// 400: malformed body or path.
    BadRequest(String),
    /// 422: well-formed body that fails validation.
    Unprocessable(String),
    /// 404: no post with that id.
    NotFound,
    /// 503: the server runs without a database.
    DatabaseUnavailable,
    /// 500: unexpected database error (details are logged, not returned).
    Internal,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::Unprocessable(message) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "post not found".into()),
            ApiError::DatabaseUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "database not configured".into(),
            ),
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".into(),
            ),
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        eprintln!("Database error: {err}");
        ApiError::Internal
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        ApiError::BadRequest(rejection.body_text())
    }
}

fn pool(extension: Option<Extension<PgPool>>) -> Result<PgPool, ApiError> {
    extension
        .map(|Extension(pool)| pool)
        .ok_or(ApiError::DatabaseUnavailable)
}

fn post_id(path: Result<Path<i64>, PathRejection>) -> Result<i64, ApiError> {
    match path {
        Ok(Path(id)) if id > 0 => Ok(id),
        Ok(Path(_)) => Err(ApiError::BadRequest("id must be a positive integer".into())),
        Err(rejection) => Err(ApiError::BadRequest(rejection.body_text())),
    }
}

fn input(body: Result<Json<PostInput>, JsonRejection>) -> Result<PostInput, ApiError> {
    let Json(input) = body?;
    input.validate()?;
    Ok(input)
}

/// `POST /posts`
pub async fn create(
    db: Option<Extension<PgPool>>,
    body: Result<Json<PostInput>, JsonRejection>,
) -> Result<Response, ApiError> {
    let pool = pool(db)?;
    let input = input(body)?;
    let post = sqlx::query_as::<_, Post>("INSERT INTO posts (title, body) VALUES ($1, $2) RETURNING id, title, body, created_at, updated_at")
    .bind(&input.title)
    .bind(&input.body)
    .fetch_one(&pool)
    .await?;
    let location = format!("/posts/{}", post.id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(post),
    )
        .into_response())
}

/// `GET /posts`
pub async fn list(db: Option<Extension<PgPool>>) -> Result<Json<Vec<Post>>, ApiError> {
    let pool = pool(db)?;
    let posts = sqlx::query_as::<_, Post>(
        "SELECT id, title, body, created_at, updated_at FROM posts ORDER BY id",
    )
    .fetch_all(&pool)
    .await?;
    Ok(Json(posts))
}

/// `GET /posts/{id}`
pub async fn get_post(
    db: Option<Extension<PgPool>>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<Json<Post>, ApiError> {
    let pool = pool(db)?;
    let id = post_id(path)?;
    sqlx::query_as::<_, Post>(
        "SELECT id, title, body, created_at, updated_at FROM posts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}

/// `PUT /posts/{id}`
pub async fn update(
    db: Option<Extension<PgPool>>,
    path: Result<Path<i64>, PathRejection>,
    body: Result<Json<PostInput>, JsonRejection>,
) -> Result<Json<Post>, ApiError> {
    let pool = pool(db)?;
    let id = post_id(path)?;
    let input = input(body)?;
    sqlx::query_as::<_, Post>(
        "UPDATE posts SET title = $1, body = $2, updated_at = now() WHERE id = $3 \
         RETURNING id, title, body, created_at, updated_at",
    )
    .bind(&input.title)
    .bind(&input.body)
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}

/// `DELETE /posts/{id}`
pub async fn delete(
    db: Option<Extension<PgPool>>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let pool = pool(db)?;
    let id = post_id(path)?;
    let result = sqlx::query("DELETE FROM posts WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Routes for `/posts` and `/posts/{id}`. Handlers expect an `Extension<PgPool>` layer
/// (see [`crate::with_database`]) and answer 503 without it. Request bodies have no
/// size limit (Axum's default 2 MiB limit is disabled).
pub fn router() -> Router {
    Router::new()
        .route("/posts", get(list).post(create))
        .route("/posts/{id}", get(get_post).put(update).delete(delete))
        .layer(DefaultBodyLimit::disable())
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    fn input(title: &str) -> PostInput {
        PostInput {
            title: title.into(),
            body: String::new(),
        }
    }

    #[test]
    fn valid_input() {
        assert!(input("Hello").validate().is_ok());
    }

    #[test]
    fn blank_titles_rejected() {
        for title in ["", "   ", "\t\n"] {
            assert!(
                matches!(input(title).validate(), Err(ApiError::Unprocessable(_))),
                "{title:?}"
            );
        }
    }

    #[test]
    fn title_length_counts_chars() {
        assert!(input(&"é".repeat(200)).validate().is_ok());
        assert!(matches!(
            input(&"é".repeat(201)).validate(),
            Err(ApiError::Unprocessable(_))
        ));
    }

    #[test]
    fn unknown_field_rejected() {
        let err = serde_json::from_str::<PostInput>(r#"{"title":"a","body":"b","x":1}"#);
        assert!(err.is_err());
        let ok: PostInput = serde_json::from_str(r#"{"title":"a","body":""}"#).unwrap();
        assert_eq!(ok, input("a"));
    }

    #[test]
    fn only_json_objects_accepted() {
        for body in [
            r#"["Hello","World"]"#,
            r#"["Hello"]"#,
            "[]",
            r#""Hello""#,
            "1",
            "null",
        ] {
            assert!(serde_json::from_str::<PostInput>(body).is_err(), "{body}");
        }
        for body in [r#"{"title":"a"}"#, r#"{"title":1,"body":"b"}"#] {
            assert!(serde_json::from_str::<PostInput>(body).is_err(), "{body}");
        }
        let ok: PostInput = serde_json::from_str(r#"{"body":"","title":"a"}"#).unwrap();
        assert_eq!(ok, input("a"));
    }

    #[test]
    fn post_serializes_rfc3339() {
        let at = DateTime::parse_from_rfc3339("2026-11-12T10:15:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let post = Post {
            id: 7,
            title: "t".into(),
            body: "b".into(),
            created_at: at,
            updated_at: at,
        };
        let json = serde_json::to_value(&post).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": 7,
                "title": "t",
                "body": "b",
                "created_at": "2026-11-12T10:15:30Z",
                "updated_at": "2026-11-12T10:15:30Z",
            })
        );
    }

    async fn render(err: ApiError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn api_error_responses() {
        let cases = [
            (ApiError::BadRequest("bad".into()), 400, "bad"),
            (ApiError::Unprocessable("nope".into()), 422, "nope"),
            (ApiError::NotFound, 404, "post not found"),
            (
                ApiError::DatabaseUnavailable,
                503,
                "database not configured",
            ),
            (ApiError::Internal, 500, "internal server error"),
        ];
        for (err, status, message) in cases {
            let (actual, body) = render(err).await;
            assert_eq!(actual.as_u16(), status);
            assert_eq!(body, serde_json::json!({ "error": message }));
        }
    }

    #[tokio::test]
    async fn sqlx_error_is_generic_internal() {
        let err: ApiError = sqlx::Error::Protocol("secret detail".into()).into();
        let (status, body) = render(err).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!body.to_string().contains("secret detail"));
    }
}
