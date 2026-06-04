use serde_json::json;

use copilot_gateway::translate::anthropic_types::AnthropicMessagesPayload;
use copilot_gateway::translate::openai_types::{
    ChatCompletionChunk, ChatCompletionResponse,
};
use copilot_gateway::translate::request::translate_to_openai;
use copilot_gateway::translate::response::translate_to_anthropic;
use copilot_gateway::translate::stream::StreamState;

fn payload(value: serde_json::Value) -> AnthropicMessagesPayload {
    serde_json::from_value(value).unwrap()
}

#[test]
fn translates_system_and_text_message() {
    let p = payload(json!({
        "model": "gpt-4o",
        "max_tokens": 100,
        "system": "You are helpful.",
        "messages": [{ "role": "user", "content": "Hello" }]
    }));

    let t = translate_to_openai(&p, false);
    assert_eq!(t.payload.messages.len(), 2);
    assert_eq!(t.payload.messages[0].role, "system");
    assert_eq!(
        t.payload.messages[0].content.as_ref().unwrap(),
        &json!("You are helpful.")
    );
    assert_eq!(t.payload.messages[1].role, "user");
    assert_eq!(t.payload.max_tokens, Some(100));
    assert!(!t.is_agent_call);
}

#[test]
fn translates_tool_use_and_tool_result() {
    let p = payload(json!({
        "model": "gpt-4o",
        "max_tokens": 100,
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "Let me check." },
                    { "type": "tool_use", "id": "tu_1", "name": "get_weather",
                      "input": { "city": "Paris" } }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "tu_1",
                      "content": "Sunny" }
                ]
            }
        ]
    }));

    let t = translate_to_openai(&p, false);
    let assistant = &t.payload.messages[0];
    assert_eq!(assistant.role, "assistant");
    let tool_calls = assistant.tool_calls.as_ref().unwrap();
    assert_eq!(tool_calls[0].function.name, "get_weather");
    assert!(tool_calls[0].function.arguments.contains("Paris"));

    let tool_msg = &t.payload.messages[1];
    assert_eq!(tool_msg.role, "tool");
    assert_eq!(tool_msg.tool_call_id.as_deref(), Some("tu_1"));
    assert_eq!(tool_msg.content.as_ref().unwrap(), &json!("Sunny"));
    assert!(t.is_agent_call);
}

#[test]
fn maps_tools_and_tool_choice() {
    let p = payload(json!({
        "model": "gpt-4o",
        "max_tokens": 50,
        "messages": [{ "role": "user", "content": "hi" }],
        "tools": [{
            "name": "search",
            "description": "search the web",
            "input_schema": { "type": "object", "properties": { "q": { "type": "string" } } }
        }],
        "tool_choice": { "type": "any" }
    }));

    let t = translate_to_openai(&p, false);
    let tools = t.payload.tools.as_ref().unwrap();
    assert_eq!(tools[0].function.name, "search");
    assert_eq!(t.payload.tool_choice, Some(json!("required")));
}

#[test]
fn translates_non_stream_response_with_tool_call() {
    let resp: ChatCompletionResponse = serde_json::from_value(json!({
        "id": "cmpl_1",
        "model": "gpt-4o",
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "content": "Working on it",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": { "name": "do_thing", "arguments": "{\"x\":1}" }
                }]
            }
        }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
    }))
    .unwrap();

    let a = translate_to_anthropic(&resp);
    assert_eq!(a.role, "assistant");
    assert_eq!(a.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(a.usage.input_tokens, 10);
    assert_eq!(a.usage.output_tokens, 5);
    assert_eq!(a.content.len(), 2);
}

fn chunk(value: serde_json::Value) -> ChatCompletionChunk {
    serde_json::from_value(value).unwrap()
}

#[test]
fn streams_text_events_in_order() {
    let mut s = StreamState::new("gpt-4o".into());

    let mut events = Vec::new();
    events.extend(s.process_chunk(&chunk(json!({
        "choices": [{ "delta": { "content": "Hel" }, "finish_reason": null }]
    }))));
    events.extend(s.process_chunk(&chunk(json!({
        "choices": [{ "delta": { "content": "lo" }, "finish_reason": "stop" }]
    }))));
    events.extend(s.finish());

    let types: Vec<&str> = events.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(
        types,
        vec![
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop",
        ]
    );
}

#[test]
fn streams_tool_call_with_input_json_delta() {
    let mut s = StreamState::new("gpt-4o".into());

    let mut events = Vec::new();
    events.extend(s.process_chunk(&chunk(json!({
        "choices": [{ "delta": { "tool_calls": [{
            "index": 0, "id": "call_1",
            "function": { "name": "lookup", "arguments": "" }
        }] }, "finish_reason": null }]
    }))));
    events.extend(s.process_chunk(&chunk(json!({
        "choices": [{ "delta": { "tool_calls": [{
            "index": 0, "function": { "arguments": "{\"q\":1}" }
        }] }, "finish_reason": "tool_calls" }]
    }))));
    events.extend(s.finish());

    let types: Vec<&str> = events.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(
        types,
        vec![
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop",
        ]
    );

    assert!(events[1].data.contains("lookup"));
    assert!(events[2].data.contains("input_json_delta"));
    assert!(events[4].data.contains("tool_use"));
}

#[test]
fn tolerates_thinking_and_unknown_blocks() {
    // Anthropic clients replay prior assistant turns that may contain thinking
    // and other block types; these must not break deserialization.
    let p = payload(json!({
        "model": "gpt-4o",
        "max_tokens": 100,
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "thinking", "thinking": "pondering" },
                    { "type": "redacted_thinking", "data": "xyz" },
                    { "type": "text", "text": "answer" }
                ]
            }
        ]
    }));

    let t = translate_to_openai(&p, false);
    let assistant = &t.payload.messages[0];
    let content = assistant.content.as_ref().unwrap().to_string();
    assert!(content.contains("pondering"));
    assert!(content.contains("answer"));
}

#[test]
fn streams_two_sequential_tool_calls() {
    let mut s = StreamState::new("gpt-4o".into());

    let mut events = Vec::new();
    events.extend(s.process_chunk(&chunk(json!({
        "choices": [{ "delta": { "tool_calls": [{
            "index": 0, "id": "call_a",
            "function": { "name": "first", "arguments": "{}" }
        }] }, "finish_reason": null }]
    }))));
    events.extend(s.process_chunk(&chunk(json!({
        "choices": [{ "delta": { "tool_calls": [{
            "index": 1, "id": "call_b",
            "function": { "name": "second", "arguments": "{}" }
        }] }, "finish_reason": "tool_calls" }]
    }))));
    events.extend(s.finish());

    let types: Vec<&str> = events.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(
        types,
        vec![
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_stop",
            "content_block_start",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop",
        ]
    );

    // Second tool block must use index 1.
    assert!(events[4].data.contains("\"index\":1"));
    assert!(events[4].data.contains("second"));
}
