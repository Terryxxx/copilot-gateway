use std::convert::Infallible;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::copilot::chat::send_chat;
use crate::state::AppState;
use crate::translate::anthropic_types::AnthropicMessagesPayload;
use crate::translate::openai_types::{ChatCompletionChunk, ChatCompletionResponse};
use crate::translate::request::translate_to_openai;
use crate::translate::response::translate_to_anthropic;
use crate::translate::stream::StreamState;

use super::AppError;

pub async fn handle_messages(
    State(state): State<AppState>,
    Json(payload): Json<AnthropicMessagesPayload>,
) -> Result<Response, AppError> {
    let stream = payload.stream.unwrap_or(false);
    let model = payload.model.clone();

    let mut translated = translate_to_openai(&payload, stream);

    // Map the client-requested model (e.g. an Anthropic id from the /model
    // picker) to an id GitHub Copilot accepts.
    let available = state.available_model_ids().await;
    let resolved = state.model_map.resolve(&translated.payload.model, &available);
    if resolved != translated.payload.model {
        tracing::info!("Mapped model '{}' -> '{}'", translated.payload.model, resolved);
        translated.payload.model = resolved;
    }

    let response = send_chat(
        &state,
        &translated.payload,
        translated.vision,
        translated.is_agent_call,
    )
    .await
    .map_err(|e| AppError::internal(format!("upstream request failed: {e}")))?;

    if !response.status().is_success() {
        let status =
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        if status == StatusCode::UNAUTHORIZED {
            tracing::error!("{}", crate::config::REAUTH_HINT);
        }
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::new(
            status,
            format!("Copilot API error: {body}"),
        ));
    }

    if !stream {
        let parsed = response
            .json::<ChatCompletionResponse>()
            .await
            .map_err(|e| AppError::internal(format!("failed to parse Copilot response: {e}")))?;
        let anthropic = translate_to_anthropic(&parsed);
        return Ok(Json(anthropic).into_response());
    }

    Ok(stream_response(response, model))
}

/// Bridge the upstream OpenAI SSE stream to an Anthropic SSE stream.
fn stream_response(response: reqwest::Response, model: String) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        let mut machine = StreamState::new(model);
        let mut es = response.bytes_stream().eventsource();

        while let Some(item) = es.next().await {
            match item {
                Ok(event) => {
                    if event.data == "[DONE]" {
                        break;
                    }
                    if event.data.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<ChatCompletionChunk>(&event.data) {
                        Ok(chunk) => {
                            for se in machine.process_chunk(&chunk) {
                                let ev = Event::default().event(se.event).data(se.data);
                                if tx.send(Ok(ev)).await.is_err() {
                                    return;
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!("failed to parse chunk: {err}; data={}", event.data);
                        }
                    }
                }
                Err(err) => {
                    tracing::error!("upstream stream error: {err}");
                    let ev = StreamState::error_event(&format!("upstream stream error: {err}"));
                    let _ = tx
                        .send(Ok(Event::default().event(ev.event).data(ev.data)))
                        .await;
                    return;
                }
            }
        }

        for se in machine.finish() {
            let ev = Event::default().event(se.event).data(se.data);
            let _ = tx.send(Ok(ev)).await;
        }
    });

    Sse::new(ReceiverStream::new(rx)).into_response()
}

use eventsource_stream::Eventsource;
