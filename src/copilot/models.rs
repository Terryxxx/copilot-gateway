use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::github::headers::copilot_headers;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// Fetch the list of models the authenticated account has access to.
pub async fn fetch_models(state: &AppState) -> Result<ModelsResponse> {
    let token = state.current_copilot_token().await;
    let response = state
        .http
        .get(format!("{}/models", state.copilot_base_url()))
        .headers(copilot_headers(&token, false, false))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("failed to get models ({status}): {body}");
    }

    Ok(response.json::<ModelsResponse>().await?)
}
