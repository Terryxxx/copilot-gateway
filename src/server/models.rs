use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::copilot::models::fetch_models;
use crate::state::AppState;

use super::AppError;

/// Return available models in an Anthropic-style list.
pub async fn handle_models(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    // Refresh from upstream and cache.
    let models = match fetch_models(&state).await {
        Ok(m) => {
            *state.models.write().await = Some(m.clone());
            m
        }
        Err(_) => state
            .models
            .read()
            .await
            .clone()
            .ok_or_else(|| AppError::internal("models unavailable"))?,
    };

    let data: Vec<Value> = models
        .data
        .iter()
        .map(|m| {
            json!({
                "type": "model",
                "id": m.id,
                "display_name": m.name.clone().unwrap_or_else(|| m.id.clone()),
            })
        })
        .collect();

    Ok(Json(json!({
        "data": data,
        "has_more": false,
        "object": "list"
    })))
}
