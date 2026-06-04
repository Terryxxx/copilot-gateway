pub mod count_tokens;
pub mod messages;
pub mod models;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

/// Build the axum router with all Anthropic-compatible routes.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { "copilot-gateway running" }))
        .route("/v1/messages", post(messages::handle_messages))
        .route(
            "/v1/messages/count_tokens",
            post(count_tokens::handle_count_tokens),
        )
        .route("/v1/models", get(models::handle_models))
        .with_state(state)
}

/// Application error mapped to an Anthropic-style error response.
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        AppError {
            status,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        AppError::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": self.message,
            }
        });
        (self.status, Json(body)).into_response()
    }
}
