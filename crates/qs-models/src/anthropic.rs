// SPDX-License-Identifier: GPL-3.0-or-later

//! Anthropic's Messages API: `POST /v1/messages` with `x-api-key` and `anthropic-version`,
//! the tools as `name`, `description` and `input_schema`, a call as a `tool_use` block and its
//! result as a `tool_result` block in the next user message (platform.claude.com, Messages
//! API; the request and response shapes as the Claude API reference gives them for raw HTTP).

use serde_json::{Value, json};

use qs_agent::{Message, Reply, ToolCall, ToolSpec};

use crate::{
    Credential, Effort, Header, ProviderConfig, Secret, command_of, result_text, tool_name,
};

/// The API version sent with every request.
const VERSION: &str = "2026-10-21";

/// The beta that lets a declined request run again on a fallback model, which the Claude API
/// reference asks for by default on the models in [`FALLBACK_MODELS`].
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// The models that take `fallbacks: "default"`.
const FALLBACK_MODELS: [&str; 5] = [
    "claude-fable-5-2",
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];

/// The request for the next reply: its URL, headers and body.
pub(crate) fn request(
    config: &ProviderConfig,
    credential: Option<&Credential>,
    system: &str,
    tools: &[ToolSpec],
    transcript: &[Message],
    sent: &[Value],
) -> (String, Vec<Header>, Value) {
    let mut headers = vec![Header::Plain("anthropic-version", VERSION.to_owned())];
    match credential {
        Some(Credential::Key(key)) => headers.push(Header::Secret("x-api-key", key.clone())),
        // P4-38: workload identity's access token, as Anthropic's example sends it
        // ("authorization: Bearer $ACCESS_TOKEN").
        Some(Credential::Bearer(token)) => headers.push(Header::Secret(
            "authorization",
            Secret::new(format!("Bearer {}", token.expose())),
        )),
        None => {}
    }
    let mut body = json!({
        "model": config.model,
        "max_tokens": config.max_tokens,
        "system": system,
        "messages": messages(transcript, sent),
        "output_config": { "effort": effort(config.effort) },
        // The system prompt, the tools and the history grow append-only, so each turn reads
        // the last one's prefix from the cache.
        "cache_control": { "type": "ephemeral" },
    });
    if !tools.is_empty()
        && let Some(object) = body.as_object_mut()
    {
        let tools: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool_name(&tool.name),
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                })
            })
            .collect();
        object.insert("tools".to_owned(), Value::Array(tools));
    }
    if FALLBACK_MODELS.contains(&config.model.as_str())
        && let Some(object) = body.as_object_mut()
    {
        headers.push(Header::Plain("anthropic-beta", FALLBACK_BETA.to_owned()));
        object.insert("fallbacks".to_owned(), json!("default"));
    }
    let url = format!("{}/v1/messages", config.base_url());
    (url, headers, body)
}

/// The wire name of `effort`.
const fn effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "xhigh",
        Effort::Max => "max",
    }
}

/// The transcript as the API's messages: each assistant message the content the API sent for
/// it when there is one, results in user messages, and messages of one role in a row joined.
fn messages(transcript: &[Message], sent: &[Value]) -> Vec<Value> {
    let mut out: Vec<(&'static str, Vec<Value>)> = Vec::new();
    let mut replies = sent.iter();
    for message in transcript {
        let (role, blocks) = match message {
            Message::User { text } => ("user", text_block(text)),
            Message::Assistant { text, calls } => {
                let blocks = match replies.next().and_then(Value::as_array) {
                    Some(content) => content.clone(),
                    None => {
                        let mut blocks = text_block(text);
                        blocks.extend(calls.iter().map(|call| {
                            json!({
                                "type": "tool_use",
                                "id": call.id,
                                "name": tool_name(&call.name),
                                "input": call.arguments,
                            })
                        }));
                        blocks
                    }
                };
                ("assistant", blocks)
            }
            Message::Tool {
                call_id,
                content,
                is_error,
            } => (
                "user",
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": result_text(content),
                    "is_error": is_error,
                })],
            ),
        };
        if blocks.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some((last, content)) if *last == role => content.extend(blocks),
            _ => out.push((role, blocks)),
        }
    }
    out.into_iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect()
}

/// A text block, or none for empty text, which the API refuses.
fn text_block(text: &str) -> Vec<Value> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![json!({ "type": "text", "text": text })]
    }
}

/// The reply in `answer`, and the content to send back for it.
pub(crate) fn reply(answer: &Value, tools: &[ToolSpec]) -> Result<(Reply, Value), String> {
    let stop = answer.get("stop_reason").and_then(Value::as_str);
    if stop == Some("refusal") {
        return Err("the model declined the request".to_owned());
    }
    let content = answer
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("the answer has no content: {answer}"))?;
    let mut text = String::new();
    let mut calls = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(part) = block.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(part);
                }
            }
            Some("tool_use") => {
                let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                calls.push(ToolCall {
                    id: id.to_owned(),
                    name: command_of(name, tools),
                    arguments: block.get("input").cloned().unwrap_or(Value::Null),
                });
            }
            // Thinking and other blocks are kept in what is sent back, not shown.
            _ => {}
        }
    }
    let raw = Value::Array(content.clone());
    if !calls.is_empty() {
        return Ok((Reply::Calls { text, calls }, raw));
    }
    if stop == Some("max_tokens") {
        return Err("the reply was cut off at max_tokens".to_owned());
    }
    Ok((Reply::Finish { summary: text }, raw))
}
