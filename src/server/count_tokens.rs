use axum::extract::State;
use axum::Json;
use serde_json::json;

use crate::state::AppState;
use crate::translate::anthropic_types::{
    AnthropicMessagesPayload, ContentBlock, MessageContent,
};

use super::AppError;

/// Estimate the number of input tokens for an Anthropic request.
pub async fn handle_count_tokens(
    State(_state): State<AppState>,
    Json(payload): Json<AnthropicMessagesPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bpe = tiktoken_rs::cl100k_base()
        .map_err(|e| AppError::internal(format!("tokenizer init failed: {e}")))?;

    let text = collect_text(&payload);
    let count = bpe.encode_with_special_tokens(&text).len();

    Ok(Json(json!({ "input_tokens": count })))
}

fn collect_text(payload: &AnthropicMessagesPayload) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(system) = &payload.system {
        parts.push(system.to_text());
    }

    for msg in &payload.messages {
        match &msg.content {
            MessageContent::Text(text) => parts.push(text.clone()),
            MessageContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => parts.push(text.clone()),
                        ContentBlock::ToolUse { name, input, .. } => {
                            parts.push(name.clone());
                            parts.push(input.to_string());
                        }
                        ContentBlock::ToolResult { content, .. } => {
                            parts.push(content.to_string());
                        }
                        ContentBlock::Thinking { thinking } => parts.push(thinking.clone()),
                        ContentBlock::Image { .. } | ContentBlock::Unknown => {}
                    }
                }
            }
        }
    }

    if let Some(tools) = &payload.tools {
        for tool in tools {
            parts.push(tool.name.clone());
            if let Some(desc) = &tool.description {
                parts.push(desc.clone());
            }
            parts.push(tool.input_schema.to_string());
        }
    }

    parts.join("\n")
}
