// SPDX-License-Identifier: GPL-3.0-or-later

//! Streamed replies put back together: each provider's server-sent events gathered into the
//! answer its non-streamed request gives, so the same code reads both.

use qs_agent::ModelError;
use serde_json::{Map, Value, json};

/// Anthropic's events (Messages API, streaming): `message_start`, then for each content block
/// `content_block_start`, its `content_block_delta`s and `content_block_stop`, then
/// `message_delta` with the `stop_reason`, and `error` on a failure.
#[derive(Default)]
pub(crate) struct AnthropicStream {
    content: Vec<Value>,
    /// Each block's `input_json_delta` fragments, joined at its `content_block_stop`.
    json: Vec<String>,
    stop: Value,
    error: Option<String>,
    /// `message_start`'s `usage`, updated by each `message_delta`'s.
    usage: serde_json::Map<String, Value>,
}

impl AnthropicStream {
    /// Takes one event's data, calling `on_text` with the reply's text as it comes.
    pub(crate) fn event(&mut self, data: &str, on_text: &mut dyn FnMut(&str)) {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            return;
        };
        let index = event
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|i| usize::try_from(i).ok());
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                if let (Some(index), Some(block)) = (index, event.get("content_block")) {
                    while self.content.len() <= index {
                        self.content.push(Value::Null);
                        self.json.push(String::new());
                    }
                    if let Some(slot) = self.content.get_mut(index) {
                        *slot = block.clone();
                    }
                }
            }
            Some("content_block_delta") => {
                if let (Some(index), Some(delta)) = (index, event.get("delta")) {
                    self.delta(index, delta, on_text);
                }
            }
            Some("content_block_stop") => {
                if let Some(index) = index
                    && let Some(json) = self.json.get(index)
                    && !json.is_empty()
                    && let Some(Value::Object(block)) = self.content.get_mut(index)
                {
                    let input = serde_json::from_str(json).unwrap_or_else(|_| json!({}));
                    block.insert("input".to_owned(), input);
                }
            }
            Some("message_start") => {
                if let Some(Value::Object(usage)) = event.pointer("/message/usage") {
                    self.usage.extend(usage.clone());
                }
            }
            Some("message_delta") => {
                if let Some(stop) = event.pointer("/delta/stop_reason") {
                    self.stop = stop.clone();
                }
                if let Some(Value::Object(usage)) = event.get("usage") {
                    self.usage.extend(usage.clone());
                }
            }
            Some("error") => {
                self.error = Some(
                    event
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("the stream failed")
                        .to_owned(),
                );
            }
            _ => {}
        }
    }

    /// One delta: `text_delta` text is shown and kept, `input_json_delta` fragments are kept
    /// for the block's input, and every other delta (`thinking_delta`, `signature_delta`)
    /// appends each of its text fields to the block's field of that name, so a thinking block
    /// goes back whole.
    fn delta(&mut self, index: usize, delta: &Value, on_text: &mut dyn FnMut(&str)) {
        if delta.get("type").and_then(Value::as_str) == Some("input_json_delta") {
            if let (Some(json), Some(part)) = (
                self.json.get_mut(index),
                delta.get("partial_json").and_then(Value::as_str),
            ) {
                json.push_str(part);
            }
            return;
        }
        let Some(Value::Object(block)) = self.content.get_mut(index) else {
            return;
        };
        let Some(fields) = delta.as_object() else {
            return;
        };
        for (name, value) in fields {
            let Some(part) = value.as_str() else { continue };
            if name == "type" {
                continue;
            }
            if name == "text" {
                on_text(part);
            }
            let held = block
                .entry(name.clone())
                .or_insert_with(|| Value::String(String::new()));
            if let Value::String(text) = held {
                text.push_str(part);
            }
        }
    }

    /// The answer a non-streamed request gives, or the stream's error.
    pub(crate) fn answer(self) -> Result<Value, ModelError> {
        if let Some(error) = self.error {
            return Err(ModelError::Stream(error));
        }
        let content: Vec<Value> = self.content.into_iter().filter(|b| !b.is_null()).collect();
        Ok(json!({ "content": content, "stop_reason": self.stop, "usage": self.usage }))
    }
}

/// OpenAI-compatible chunks (`CreateChatCompletionStreamResponse`): each holds
/// `choices[0].delta`, whose `content` is the next piece of text and whose `tool_calls` are
/// `ChatCompletionMessageToolCallChunk`s, by `index`, with the call's `id`, name and pieces of
/// its arguments; the last one has the `finish_reason`.
#[derive(Default)]
pub(crate) struct OpenAiStream {
    content: String,
    calls: Vec<Map<String, Value>>,
    finish: Value,
}

impl OpenAiStream {
    /// Takes one chunk, calling `on_text` with the reply's text as it comes.
    pub(crate) fn event(&mut self, data: &str, on_text: &mut dyn FnMut(&str)) {
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            return;
        };
        let Some(choice) = chunk.pointer("/choices/0") else {
            return;
        };
        if let Some(finish) = choice.get("finish_reason")
            && !finish.is_null()
        {
            self.finish = finish.clone();
        }
        let Some(delta) = choice.get("delta") else {
            return;
        };
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            on_text(text);
            self.content.push_str(text);
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let index = call
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|i| usize::try_from(i).ok())
                .unwrap_or_default();
            while self.calls.len() <= index {
                let mut fresh = Map::new();
                fresh.insert("type".to_owned(), json!("function"));
                fresh.insert(
                    "function".to_owned(),
                    json!({ "name": "", "arguments": "" }),
                );
                self.calls.push(fresh);
            }
            let Some(held) = self.calls.get_mut(index) else {
                continue;
            };
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                held.insert("id".to_owned(), json!(id));
            }
            for field in ["name", "arguments"] {
                if let Some(part) = call
                    .pointer(&format!("/function/{field}"))
                    .and_then(Value::as_str)
                    && let Some(Value::String(text)) =
                        held.get_mut("function").and_then(|f| f.get_mut(field))
                {
                    text.push_str(part);
                }
            }
        }
    }

    /// The answer a non-streamed request gives.
    pub(crate) fn answer(self) -> Value {
        let mut message = json!({ "role": "assistant", "content": self.content });
        if !self.calls.is_empty()
            && let Some(object) = message.as_object_mut()
        {
            let calls: Vec<Value> = self.calls.into_iter().map(Value::Object).collect();
            object.insert("tool_calls".to_owned(), Value::Array(calls));
        }
        json!({ "choices": [{ "finish_reason": self.finish, "message": message }] })
    }
}
