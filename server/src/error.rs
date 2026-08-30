use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use tracing::warn;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }
    pub(crate) fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "需要有效身份凭证".to_owned(),
        }
    }
    pub(crate) fn unauthorized_msg(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: message.into(),
        }
    }
    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: message.into(),
        }
    }
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.into(),
        }
    }
    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: message.into(),
        }
    }
    pub(crate) fn rate_limited() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "请求过于频繁，请稍后再试".to_owned(),
        }
    }
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: message.into(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        warn!(error = %error, "database operation failed");
        Self::internal("服务暂时无法处理请求")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": { "code": self.code, "message": self.message } })),
        )
            .into_response()
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[test]
    fn constructors_map_to_expected_status_codes() {
        assert_eq!(ApiError::bad_request("x").status, StatusCode::BAD_REQUEST);
        assert_eq!(ApiError::unauthorized().status, StatusCode::UNAUTHORIZED);
        assert_eq!(ApiError::forbidden("x").status, StatusCode::FORBIDDEN);
        assert_eq!(ApiError::not_found("x").status, StatusCode::NOT_FOUND);
        assert_eq!(ApiError::conflict("x").status, StatusCode::CONFLICT);
        assert_eq!(
            ApiError::rate_limited().status,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            ApiError::internal("x").status,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn response_body_wraps_code_and_message() {
        let response = ApiError::rate_limited().into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"]["code"], "rate_limited");
        assert_eq!(payload["error"]["message"], "请求过于频繁，请稍后再试");
    }

    #[tokio::test]
    async fn sqlx_errors_are_masked_as_internal() {
        let response = ApiError::from(sqlx::Error::RowNotFound).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"]["code"], "internal_error");
        assert_eq!(payload["error"]["message"], "服务暂时无法处理请求");
        assert!(
            payload["error"].get("source").is_none(),
            "内部错误细节不得外泄"
        );
    }
}
