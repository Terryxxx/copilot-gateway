use std::sync::Arc;

use reqwest::Client;
use tokio::sync::RwLock;

use crate::copilot::models::ModelsResponse;
use crate::model_map::ModelMap;

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub http: Client,
    pub account_type: String,
    pub copilot_token: Arc<RwLock<String>>,
    pub models: Arc<RwLock<Option<ModelsResponse>>>,
    pub model_map: Arc<ModelMap>,
}

impl AppState {
    pub fn new(http: Client, account_type: String) -> Self {
        Self {
            http,
            account_type,
            copilot_token: Arc::new(RwLock::new(String::new())),
            models: Arc::new(RwLock::new(None)),
            model_map: Arc::new(ModelMap::load()),
        }
    }

    /// Base URL for the Copilot API, depending on account type.
    pub fn copilot_base_url(&self) -> String {
        if self.account_type == "individual" {
            "https://api.githubcopilot.com".to_string()
        } else {
            format!("https://api.{}.githubcopilot.com", self.account_type)
        }
    }

    pub async fn current_copilot_token(&self) -> String {
        self.copilot_token.read().await.clone()
    }

    /// Snapshot of model ids the account can access (empty if not yet loaded).
    pub async fn available_model_ids(&self) -> Vec<String> {
        match self.models.read().await.as_ref() {
            Some(m) => m.data.iter().map(|model| model.id.clone()).collect(),
            None => Vec::new(),
        }
    }
}
