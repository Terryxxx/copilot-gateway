use serde_json::{json, Value};

use super::anthropic_types::{
    AnthropicMessagesPayload, ContentBlock, ImageSource, MessageContent,
};
use super::openai_types::{
    ChatCompletionsPayload, FunctionCall, Message, Tool, ToolCall, ToolFunction,
};

/// Result of translating an Anthropic request, including detected flags.
pub struct TranslatedRequest {
    pub payload: ChatCompletionsPayload,
    pub vision: bool,
    pub is_agent_call: bool,
}

pub fn translate_to_openai(p: &AnthropicMessagesPayload, stream: bool) -> TranslatedRequest {
    let mut messages: Vec<Message> = Vec::new();
    let mut vision = false;

    // Anthropic carries the system prompt separately; OpenAI uses a system message.
    if let Some(system) = &p.system {
        let text = system.to_text();
        if !text.is_empty() {
            messages.push(Message {
                role: "system".into(),
                content: Some(Value::String(text)),
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    for msg in &p.messages {
        match &msg.content {
            MessageContent::Text(text) => {
                messages.push(Message {
                    role: msg.role.clone(),
                    content: Some(Value::String(text.clone())),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            MessageContent::Blocks(blocks) => {
                translate_blocks(&msg.role, blocks, &mut messages, &mut vision);
            }
        }
    }

    // Mark as an agent call when the conversation already contains assistant/tool turns.
    let is_agent_call = messages
        .iter()
        .any(|m| m.role == "assistant" || m.role == "tool");

    let tools = p.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| Tool {
                kind: "function".into(),
                function: ToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: if t.input_schema.is_null() {
                        json!({ "type": "object", "properties": {} })
                    } else {
                        t.input_schema.clone()
                    },
                },
            })
            .collect::<Vec<_>>()
    });

    let payload = ChatCompletionsPayload {
        messages,
        model: p.model.clone(),
        temperature: p.temperature,
        top_p: p.top_p,
        max_tokens: Some(p.max_tokens),
        stop: p.stop_sequences.clone(),
        stream: Some(stream),
        tools,
        tool_choice: translate_tool_choice(p.tool_choice.as_ref()),
    };

    TranslatedRequest {
        payload,
        vision,
        is_agent_call,
    }
}

fn translate_blocks(
    role: &str,
    blocks: &[ContentBlock],
    messages: &mut Vec<Message>,
    vision: &mut bool,
) {
    let mut content_parts: Vec<Value> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                content_parts.push(json!({ "type": "text", "text": text }));
            }
            ContentBlock::Image { source } => {
                *vision = true;
                content_parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": image_url(source) }
                }));
            }
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    kind: "function".into(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_else(|_| "{}".into()),
                    },
                });
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                // Each tool_result becomes a standalone OpenAI tool message.
                messages.push(Message {
                    role: "tool".into(),
                    content: Some(Value::String(tool_result_to_string(content))),
                    tool_calls: None,
                    tool_call_id: Some(tool_use_id.clone()),
                });
            }
            ContentBlock::Thinking { thinking } => {
                // Fold reasoning text into the textual content for OpenAI.
                if !thinking.is_empty() {
                    content_parts.push(json!({ "type": "text", "text": thinking }));
                }
            }
            ContentBlock::Unknown => {}
        }
    }

    let has_content = !content_parts.is_empty();
    let has_tool_calls = !tool_calls.is_empty();

    if has_content || has_tool_calls {
        messages.push(Message {
            role: role.to_string(),
            content: if has_content {
                Some(simplify_content(content_parts))
            } else {
                None
            },
            tool_calls: if has_tool_calls {
                Some(tool_calls)
            } else {
                None
            },
            tool_call_id: None,
        });
    }
}

/// Collapse an all-text part list into a plain string; otherwise keep the array.
fn simplify_content(parts: Vec<Value>) -> Value {
    let all_text = parts
        .iter()
        .all(|p| p.get("type").and_then(|t| t.as_str()) == Some("text"));
    if all_text {
        let joined = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("");
        Value::String(joined)
    } else {
        Value::Array(parts)
    }
}

fn image_url(source: &ImageSource) -> String {
    match source.kind.as_str() {
        "url" => source.url.clone().unwrap_or_default(),
        _ => {
            // base64 image -> data URL
            let media = source
                .media_type
                .clone()
                .unwrap_or_else(|| "image/png".into());
            let data = source.data.clone().unwrap_or_default();
            format!("data:{media};base64,{data}")
        }
    }
}

fn tool_result_to_string(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    text.to_string()
                } else {
                    item.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn translate_tool_choice(choice: Option<&Value>) -> Option<Value> {
    let choice = choice?;
    let kind = choice.get("type").and_then(|t| t.as_str())?;
    match kind {
        "auto" => Some(json!("auto")),
        "any" => Some(json!("required")),
        "tool" => {
            let name = choice.get("name").and_then(|n| n.as_str()).unwrap_or("");
            Some(json!({ "type": "function", "function": { "name": name } }))
        }
        "none" => Some(json!("none")),
        _ => None,
    }
}
