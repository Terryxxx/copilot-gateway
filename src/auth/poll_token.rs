use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

use crate::config::{GITHUB_BASE_URL, GITHUB_CLIENT_ID};

use super::device_code::DeviceCodeResponse;

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
}

/// Poll GitHub for the OAuth access token until the user authorizes the device.
pub async fn poll_access_token(
    http: &reqwest::Client,
    device: &DeviceCodeResponse,
) -> Result<String> {
    // Add a second of slack to the polling interval to avoid slow_down errors.
    let sleep_duration = Duration::from_secs(device.interval + 1);

    loop {
        let response = http
            .post(format!("{GITHUB_BASE_URL}/login/oauth/access_token"))
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "client_id": GITHUB_CLIENT_ID,
                "device_code": device.device_code,
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            }))
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let json = resp.json::<AccessTokenResponse>().await?;
                if let Some(token) = json.access_token {
                    if !token.is_empty() {
                        return Ok(token);
                    }
                }
            }
            Ok(_) | Err(_) => {
                // Not authorized yet or transient error; keep polling.
            }
        }

        tokio::time::sleep(sleep_duration).await;
    }
}
