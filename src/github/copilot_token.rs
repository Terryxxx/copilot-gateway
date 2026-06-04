use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::config::GITHUB_API_BASE_URL;
use crate::github::headers::github_headers;

#[derive(Debug, Deserialize)]
pub struct CopilotTokenResponse {
    pub token: String,
    /// Seconds until the token should be refreshed.
    pub refresh_in: u64,
}

/// Exchange the GitHub OAuth token for a short-lived Copilot token.
pub async fn fetch_copilot_token(
    http: &reqwest::Client,
    github_token: &str,
) -> Result<CopilotTokenResponse> {
    let response = http
        .get(format!("{GITHUB_API_BASE_URL}/copilot_internal/v2/token"))
        .headers(github_headers(github_token))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            tracing::error!("{}", crate::config::REAUTH_HINT);
        }
        let body = response.text().await.unwrap_or_default();
        bail!("failed to get Copilot token ({status}): {body}");
    }

    Ok(response.json::<CopilotTokenResponse>().await?)
}

/// Spawn a background task that keeps the Copilot token fresh.
pub fn spawn_refresh_task(
    http: reqwest::Client,
    github_token: String,
    copilot_token: Arc<RwLock<String>>,
    initial_refresh_in: u64,
) {
    tokio::spawn(async move {
        let mut refresh_in = initial_refresh_in;
        loop {
            // Refresh a bit early to avoid using an expired token.
            let wait = refresh_in.saturating_sub(60).max(60);
            tokio::time::sleep(Duration::from_secs(wait)).await;

            match fetch_copilot_token(&http, &github_token).await {
                Ok(resp) => {
                    *copilot_token.write().await = resp.token;
                    refresh_in = resp.refresh_in;
                    tracing::info!("Refreshed Copilot token (next in {refresh_in}s)");
                }
                Err(err) => {
                    tracing::error!("Failed to refresh Copilot token: {err}");
                    refresh_in = 300;
                }
            }
        }
    });
}
