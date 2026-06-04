use anyhow::{bail, Result};
use serde::Deserialize;

use crate::config::{GITHUB_APP_SCOPES, GITHUB_BASE_URL, GITHUB_CLIENT_ID};

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
}

/// Request a device + user code pair from GitHub.
pub async fn request_device_code(http: &reqwest::Client) -> Result<DeviceCodeResponse> {
    let response = http
        .post(format!("{GITHUB_BASE_URL}/login/device/code"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "client_id": GITHUB_CLIENT_ID,
            "scope": GITHUB_APP_SCOPES,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("failed to get device code ({status}): {body}");
    }

    Ok(response.json::<DeviceCodeResponse>().await?)
}
