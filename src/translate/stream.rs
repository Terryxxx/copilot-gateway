use std::collections::HashMap;

use serde_json::{json, Value};
use uuid::Uuid;

use super::openai_types::ChatCompletionChunk;
use super::response::map_stop_reason;

/// A single Anthropic SSE event to be written to the client.
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

impl SseEvent {
    fn new(event: &str, data: Value) -> Self {
        SseEvent {
            event: event.to_string(),
            data: data.to_string(),
        }
    }
}

struct ToolInfo {
    anthropic_index: usize,
}

/// Tracks the in-flight translation of an OpenAI stream into Anthropic events.
///
/// Mirrors the proven reference behaviour: only one content block is open at a
/// time, blocks are indexed sequentially, and a tool block is only started once
/// both its `id` and `name` are known.
pub struct StreamState {
    message_started: bool,
    finished: bool,
    content_block_index: usize,
    content_block_open: bool,
    /// Maps an OpenAI tool_call index to its assigned Anthropic block index.
    tool_calls: HashMap<usize, ToolInfo>,
    model: String,
    message_id: String,
    stop_reason: String,
    input_tokens: u64,
    output_tokens: u64,
}

impl StreamState {
    pub fn new(model: String) -> Self {
        StreamState {
            message_started: false,
            finished: false,
            content_block_index: 0,
            content_block_open: false,
            tool_calls: HashMap::new(),
            model,
            message_id: format!("msg_{}", Uuid::new_v4().simple()),
            stop_reason: "end_turn".into(),
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    fn capture_usage(&mut self, chunk: &ChatCompletionChunk) {
        if let Some(usage) = &chunk.usage {
            if usage.prompt_tokens > 0 {
                self.input_tokens = usage.prompt_tokens;
            }
            if usage.completion_tokens > 0 {
                self.output_tokens = usage.completion_tokens;
            }
        }
    }

    fn ensure_started(&mut self, events: &mut Vec<SseEvent>) {
        if self.message_started {
            return;
        }
        self.message_started = true;
        events.push(SseEvent::new(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "model": self.model,
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": self.input_tokens,
                        "output_tokens": 0
                    }
                }
            }),
        ));
    }

    fn is_tool_block_open(&self) -> bool {
        self.content_block_open
            && self
                .tool_calls
                .values()
                .any(|t| t.anthropic_index == self.content_block_index)
    }

    fn close_current_block(&mut self, events: &mut Vec<SseEvent>) {
        events.push(SseEvent::new(
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": self.content_block_index }),
        ));
    }

    /// Process one OpenAI chunk, returning the Anthropic events it produces.
    pub fn process_chunk(&mut self, chunk: &ChatCompletionChunk) -> Vec<SseEvent> {
        let mut events = Vec::new();
        // Capture usage before message_start so input_tokens can be reported.
        self.capture_usage(chunk);
        self.ensure_started(&mut events);

        let Some(choice) = chunk.choices.first() else {
            return events;
        };

        if let Some(text) = &choice.delta.content {
            if !text.is_empty() {
                self.handle_text_delta(text, &mut events);
            }
        }

        if let Some(tool_calls) = &choice.delta.tool_calls {
            for tc in tool_calls {
                self.handle_tool_delta(tc, &mut events);
            }
        }

        if let Some(reason) = &choice.finish_reason {
            self.stop_reason = map_stop_reason(Some(reason.as_str()));
        }

        events
    }

    fn handle_text_delta(&mut self, text: &str, events: &mut Vec<SseEvent>) {
        // A tool block was open: close it before starting a text block.
        if self.is_tool_block_open() {
            self.close_current_block(events);
            self.content_block_index += 1;
            self.content_block_open = false;
        }

        if !self.content_block_open {
            events.push(SseEvent::new(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": self.content_block_index,
                    "content_block": { "type": "text", "text": "" }
                }),
            ));
            self.content_block_open = true;
        }

        events.push(SseEvent::new(
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": self.content_block_index,
                "delta": { "type": "text_delta", "text": text }
            }),
        ));
    }

    fn handle_tool_delta(
        &mut self,
        tc: &super::openai_types::DeltaToolCall,
        events: &mut Vec<SseEvent>,
    ) {
        let name = tc.function.as_ref().and_then(|f| f.name.clone());

        // A new tool call only starts once both id and name are known.
        if let (Some(id), Some(name)) = (tc.id.clone(), name) {
            if self.content_block_open {
                self.close_current_block(events);
                self.content_block_index += 1;
                self.content_block_open = false;
            }

            let anthropic_index = self.content_block_index;
            self.tool_calls.insert(tc.index, ToolInfo { anthropic_index });

            events.push(SseEvent::new(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": anthropic_index,
                    "content_block": {
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": {}
                    }
                }),
            ));
            self.content_block_open = true;
        }

        // Argument fragments are appended to the matching tool block.
        if let Some(args) = tc.function.as_ref().and_then(|f| f.arguments.clone()) {
            if !args.is_empty() {
                if let Some(info) = self.tool_calls.get(&tc.index) {
                    events.push(SseEvent::new(
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": info.anthropic_index,
                            "delta": {
                                "type": "input_json_delta",
                                "partial_json": args
                            }
                        }),
                    ));
                }
            }
        }
    }

    /// Emit the closing events for the stream.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if self.finished {
            return events;
        }
        self.finished = true;
        self.ensure_started(&mut events);

        if self.content_block_open {
            self.close_current_block(&mut events);
            self.content_block_open = false;
        }

        events.push(SseEvent::new(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": self.stop_reason,
                    "stop_sequence": null
                },
                "usage": {
                    "input_tokens": self.input_tokens,
                    "output_tokens": self.output_tokens
                }
            }),
        ));
        events.push(SseEvent::new(
            "message_stop",
            json!({ "type": "message_stop" }),
        ));
        events
    }

    /// Build a terminal Anthropic error event for fatal stream failures.
    pub fn error_event(message: &str) -> SseEvent {
        SseEvent::new(
            "error",
            json!({
                "type": "error",
                "error": { "type": "api_error", "message": message }
            }),
        )
    }
}
