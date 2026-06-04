use uuid::Uuid;

use super::anthropic_types::{
    AnthropicResponse, AnthropicResponseBlock, AnthropicUsage,
};
use super::openai_types::ChatCompletionResponse;

/// Translate a non-streaming OpenAI response into an Anthropic message.
pub fn translate_to_anthropic(resp: &ChatCompletionResponse) -> AnthropicResponse {
    let mut content: Vec<AnthropicResponseBlock> = Vec::new();
    let mut stop_reason = "end_turn".to_string();

    if let Some(choice) = resp.choices.first() {
        if let Some(text) = &choice.message.content {
            if !text.is_empty() {
                content.push(AnthropicResponseBlock::Text { text: text.clone() });
            }
        }

        if let Some(tool_calls) = &choice.message.tool_calls {
            for call in tool_calls {
                let input = serde_json::from_str(&call.function.arguments)
                    .unwrap_or_else(|_| serde_json::json!({}));
                content.push(AnthropicResponseBlock::ToolUse {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    input,
                });
            }
        }

        stop_reason = map_stop_reason(choice.finish_reason.as_deref());
    }

    let usage = resp
        .usage
        .as_ref()
        .map(|u| AnthropicUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        })
        .unwrap_or_default();

    AnthropicResponse {
        id: format!("msg_{}", Uuid::new_v4().simple()),
        kind: "message".into(),
        role: "assistant".into(),
        model: resp.model.clone(),
        content,
        stop_reason: Some(stop_reason),
        stop_sequence: None,
        usage,
    }
}

pub fn map_stop_reason(finish_reason: Option<&str>) -> String {
    match finish_reason {
        Some("length") => "max_tokens".into(),
        Some("tool_calls") => "tool_use".into(),
        Some("content_filter") => "end_turn".into(),
        _ => "end_turn".into(),
    }
}
