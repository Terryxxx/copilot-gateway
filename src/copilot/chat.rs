use anyhow::Result;
use reqwest::Response;

use crate::github::headers::copilot_headers;
use crate::state::AppState;
use crate::translate::openai_types::ChatCompletionsPayload;

/// Send a chat completion request to Copilot. The caller decides how to consume
/// the response based on whether streaming was requested.
pub async fn send_chat(
    state: &AppState,
    payload: &ChatCompletionsPayload,
    vision: bool,
    is_agent_call: bool,
) -> Result<Response> {
    let token = state.current_copilot_token().await;
    let response = state
        .http
        .post(format!("{}/chat/completions", state.copilot_base_url()))
        .headers(copilot_headers(&token, vision, is_agent_call))
        .json(payload)
        .send()
        .await?;

    Ok(response)
}
