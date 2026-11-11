// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::unwrap_in_result,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, match or index is a failed test"
)]

use std::cell::RefCell;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use qs_agent::{Message, Model as _, Reply, ToolCall, ToolSpec};
use serde_json::{Value, json};

use crate::{
    Header, Http, KeyError, KeyStore as _, Keychain, MemoryKeys, ProviderConfig, ProviderError,
    ProviderKind, ProviderModel, Secret, Transport, TransportError,
};

const KEY: &str = "sk-test-0123456789-very-secret";

/// A transport that records each request and answers from a script.
struct Scripted {
    answers: RefCell<Vec<Result<Value, TransportError>>>,
    sent: RefCell<Vec<(String, Vec<Header>, Value)>>,
}

impl Scripted {
    fn new(answers: Vec<Result<Value, TransportError>>) -> Self {
        Self {
            answers: RefCell::new(answers.into_iter().rev().collect()),
            sent: RefCell::new(Vec::new()),
        }
    }
}

impl Transport for &Scripted {
    fn post_json(
        &self,
        url: &str,
        headers: &[Header],
        body: &Value,
    ) -> Result<Value, TransportError> {
        self.sent
            .borrow_mut()
            .push((url.to_owned(), headers.to_vec(), body.clone()));
        self.answers.borrow_mut().pop().unwrap()
    }

    /// The next scripted answer is the stream: an array of event data, each given in turn.
    fn post_stream(
        &self,
        url: &str,
        headers: &[Header],
        body: &Value,
        on_data: &mut dyn FnMut(&str),
    ) -> Result<(), TransportError> {
        let events = self.post_json(url, headers, body)?;
        for event in events.as_array().unwrap() {
            on_data(&event.to_string());
        }
        Ok(())
    }
}

fn tools() -> Vec<ToolSpec> {
    vec![ToolSpec {
        name: "edit.apply".to_owned(),
        description: "Stages a change.".to_owned(),
        input_schema: json!({ "type": "object" }),
    }]
}

fn header<'a>(headers: &'a [Header], name: &str) -> Option<&'a str> {
    headers.iter().find(|h| h.name() == name).map(Header::value)
}

// No spec statement: AGT-6, Anthropic's Messages API: tools by name, description and
// input_schema with the dots of a command id made `_`, the key in x-api-key with the API
// version, a tool_use block back as the command's call, its content (thinking included) sent
// back unchanged and the result as a tool_result in a user message, then the text as the finish
#[test]
fn anthropic_speaks_the_messages_api() {
    let thinking = json!({ "type": "thinking", "thinking": "", "signature": "sig" });
    let first = json!({
        "stop_reason": "tool_use",
        "content": [
            thinking,
            { "type": "text", "text": "Staging it." },
            { "type": "tool_use", "id": "toolu_1", "name": "edit_apply", "input": { "files": [] } },
        ],
    });
    let second = json!({
        "stop_reason": "end_turn",
        "content": [{ "type": "text", "text": "Done." }],
    });
    let transport = Scripted::new(vec![Ok(first), Ok(second)]);
    let mut model = ProviderModel::new(
        ProviderConfig::anthropic(),
        Some(Secret::new(KEY.to_owned())),
        &transport,
    )
    .unwrap();
    let mut transcript = vec![Message::User {
        text: "Make the axe stronger.".to_owned(),
    }];
    let reply = model.reply("system", &tools(), &transcript).unwrap();
    let Reply::Calls { text, calls } = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(text, "Staging it.");
    assert_eq!(
        calls[0].name, "edit.apply",
        "the command, not the wire name"
    );
    assert_eq!(calls[0].arguments, json!({ "files": [] }));
    transcript.push(Message::Assistant {
        text,
        calls: calls.clone(),
    });
    transcript.push(Message::Tool {
        call_id: "toolu_1".to_owned(),
        content: json!([{ "id": 0 }]),
        is_error: false,
    });
    let finished = model.reply("system", &tools(), &transcript).unwrap();
    assert_eq!(
        finished,
        Reply::Finish {
            summary: "Done.".to_owned()
        }
    );
    let sent = transport.sent.borrow();
    let (url, headers, body) = &sent[0];
    assert_eq!(url, "https://api.anthropic.com/v1/messages");
    assert_eq!(header(headers, "x-api-key"), Some(KEY));
    assert_eq!(header(headers, "anthropic-version"), Some("2026-10-21"));
    assert_eq!(body["model"], "claude-opus-5-5");
    assert_eq!(body["system"], "system");
    assert_eq!(body["tools"][0]["name"], "edit_apply");
    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
    assert_eq!(body["fallbacks"], "default");
    assert_eq!(body["output_config"]["effort"], "high");
    let messages = sent[1].2["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(
        messages[1]["content"][0], thinking,
        "thinking goes back unchanged"
    );
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(messages[2]["content"][0]["tool_use_id"], "toolu_1");
    assert_eq!(messages[2]["content"][0]["content"], "[{\"id\":0}]");
}

// No spec statement: AGT-6, Anthropic's refusal, a cut-off reply and an answer with no content
// end the run with a message, and results in a row go back in one user message; a model that
// takes no fallbacks is sent none
#[test]
fn anthropic_failures_and_joined_results() {
    let transport = Scripted::new(vec![
        Ok(json!({ "stop_reason": "refusal", "content": [] })),
        Ok(json!({ "stop_reason": "max_tokens", "content": [{ "type": "text", "text": "x" }] })),
        Ok(json!({ "type": "error" })),
    ]);
    let config = ProviderConfig::new(ProviderKind::Anthropic, "claude-haiku-5-5");
    let mut model =
        ProviderModel::new(config, Some(Secret::new(KEY.to_owned())), &transport).unwrap();
    let call = |id: &str| ToolCall {
        id: id.to_owned(),
        name: "edit.apply".to_owned(),
        arguments: json!({}),
    };
    let transcript = vec![
        Message::User {
            text: "go".to_owned(),
        },
        Message::Assistant {
            text: String::new(),
            calls: vec![call("a"), call("b")],
        },
        Message::Tool {
            call_id: "a".to_owned(),
            content: json!("ok"),
            is_error: false,
        },
        Message::Tool {
            call_id: "b".to_owned(),
            content: json!("no"),
            is_error: true,
        },
    ];
    let refused = model.reply("s", &[], &transcript).unwrap_err();
    assert!(refused.contains("declined"), "{refused}");
    let cut = model.reply("s", &[], &transcript).unwrap_err();
    assert!(cut.contains("max_tokens"), "{cut}");
    assert!(model.reply("s", &[], &transcript).is_err());
    let sent = transport.sent.borrow();
    let body = &sent[0].2;
    assert!(body.get("fallbacks").is_none() && body.get("tools").is_none());
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert_eq!(messages[1]["content"][0]["name"], "edit_apply");
    let results = messages[2]["content"].as_array().unwrap();
    assert_eq!(results.len(), 2, "both results in one message");
    assert_eq!(results[1]["is_error"], true);
}

// No spec statement: AGT-6, an OpenAI-compatible endpoint: function tools, the key as a Bearer
// token, a call's JSON arguments parsed and text that is not JSON passed on as text, the
// provider's message sent back and the result as a tool message; content_filter and length
// end the run
#[test]
fn openai_speaks_chat_completions() {
    let first = json!({ "choices": [{ "finish_reason": "tool_calls", "message": {
        "role": "assistant", "content": null, "tool_calls": [
            { "id": "c1", "type": "function",
              "function": { "name": "edit_apply", "arguments": "{\"files\":[]}" } },
            { "id": "c2", "type": "function",
              "function": { "name": "edit_apply", "arguments": "{not json" } },
        ] } }] });
    let second = json!({ "choices": [{ "finish_reason": "stop",
        "message": { "role": "assistant", "content": "Done." } }] });
    let transport = Scripted::new(vec![
        Ok(first),
        Ok(second),
        Ok(json!({ "choices": [{ "finish_reason": "content_filter", "message": {} }] })),
        Ok(json!({ "choices": [{ "finish_reason": "length", "message": { "content": "x" } }] })),
    ]);
    let mut config = ProviderConfig::new(ProviderKind::OpenAiCompatible, "gpt-test");
    config.base_url = Some("https://example.test/v1/".to_owned());
    let mut model =
        ProviderModel::new(config, Some(Secret::new(KEY.to_owned())), &transport).unwrap();
    let mut transcript = vec![Message::User {
        text: "go".to_owned(),
    }];
    let Reply::Calls { calls, text } = model.reply("s", &tools(), &transcript).unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].name, "edit.apply");
    assert_eq!(calls[0].arguments, json!({ "files": [] }));
    assert_eq!(calls[1].arguments, json!("{not json"));
    transcript.push(Message::Assistant { text, calls });
    transcript.push(Message::Tool {
        call_id: "c1".to_owned(),
        content: json!({ "ok": true }),
        is_error: false,
    });
    assert_eq!(
        model.reply("s", &tools(), &transcript).unwrap(),
        Reply::Finish {
            summary: "Done.".to_owned()
        }
    );
    assert!(model.reply("s", &[], &transcript).is_err());
    assert!(model.reply("s", &[], &transcript).is_err());
    let sent = transport.sent.borrow();
    let (url, headers, body) = &sent[0];
    assert_eq!(url, "https://example.test/v1/chat/completions");
    assert_eq!(
        header(headers, "authorization"),
        Some(&*format!("Bearer {KEY}"))
    );
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "edit_apply");
    let messages = sent[1].2["messages"].as_array().unwrap();
    assert_eq!(
        messages[2]["tool_calls"][0]["id"], "c1",
        "the provider's message"
    );
    assert_eq!(messages[3]["role"], "tool");
    assert_eq!(messages[3]["tool_call_id"], "c1");
    assert_eq!(messages[3]["content"], "{\"ok\":true}");
}

/// A local HTTP server answering each request with the next of `answers`; it returns the
/// requests it read.
fn serve(answers: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in answers {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut content = vec![0; length];
            reader.read_exact(&mut content).unwrap();
            head.push_str(&String::from_utf8(content).unwrap());
            requests.push(head);
            let answer = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
                 connection: close\r\n\r\n{body}",
                body.len()
            );
            reader.get_mut().write_all(answer.as_bytes()).unwrap();
        }
        requests
    });
    (base, handle)
}

// No spec statement: AGT-6, PRIN-9, P4-07 "Works fully offline with a local Ollama model": the
// Ollama provider needs no key, sends none, and talks only to the local server it names, over
// the real HTTP client
#[test]
fn ollama_runs_offline_with_no_key() {
    let answer = json!({ "choices": [{ "finish_reason": "stop",
        "message": { "role": "assistant", "content": "Hello from llama." } }] });
    let (base, server) = serve(vec![(200, answer.to_string())]);
    let mut config = ProviderConfig::new(ProviderKind::Ollama, "qwen3-coder");
    config.base_url = Some(format!("{base}/v1"));
    let keys = MemoryKeys::new();
    let mut model = ProviderModel::connect(config, &keys).unwrap();
    let reply = model
        .reply(
            "s",
            &tools(),
            &[Message::User {
                text: "hi".to_owned(),
            }],
        )
        .unwrap();
    assert_eq!(
        reply,
        Reply::Finish {
            summary: "Hello from llama.".to_owned()
        }
    );
    let requests = server.join().unwrap();
    assert!(
        requests[0].starts_with("POST /v1/chat/completions "),
        "{}",
        requests[0]
    );
    assert!(!requests[0].to_ascii_lowercase().contains("authorization"));
    assert!(requests[0].contains("\"model\":\"qwen3-coder\""));
    assert_eq!(
        ProviderConfig::new(ProviderKind::Ollama, "m").base_url(),
        "http://localhost:11434/v1"
    );
}

/// A writer the log goes to, kept for reading.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// No spec statement: P4-07 "Keys never appear in logs, settings files or crash records": a key
// prints as redacted wherever it is formatted with Debug (a log field or a panic message), the
// settings form has no key, a failed request's error has none, and a run logged at every level
// writes none
#[test]
fn keys_never_appear_in_logs_settings_or_errors() {
    let secret = Secret::new(KEY.to_owned());
    assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
    let header = Header::Secret("x-api-key", secret.clone());
    assert!(!format!("{header:?}").contains(KEY));
    let config = ProviderConfig::anthropic();
    let settings = serde_json::to_string(&config).unwrap();
    assert!(!settings.contains(KEY));
    assert_eq!(
        serde_json::from_str::<ProviderConfig>(&settings).unwrap(),
        config
    );
    let (base, server) = serve(vec![(
        401,
        json!({ "type": "error", "error": { "message": "invalid x-api-key" } }).to_string(),
    )]);
    let mut config = ProviderConfig::anthropic();
    config.base_url = Some(base);
    let keys = MemoryKeys::new();
    keys.set(&config.key_name(), &secret).unwrap();
    let log = Log::default();
    let writer = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    let error = tracing::subscriber::with_default(subscriber, || {
        let mut model = ProviderModel::connect(config, &keys).unwrap();
        tracing::debug!(?secret, "the key as a log field");
        model
            .reply(
                "s",
                &[],
                &[Message::User {
                    text: "hi".to_owned(),
                }],
            )
            .unwrap_err()
    });
    let requests = server.join().unwrap();
    assert!(
        requests[0].contains(KEY),
        "the request itself carries the key"
    );
    assert!(error.contains("401") && !error.contains(KEY), "{error}");
    let written = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(
        written.contains("Secret(<redacted>)"),
        "the log was captured: {written}"
    );
    assert!(!written.contains(KEY), "{written}");
}

// No spec statement: AGT-6, a provider that needs a key and has none is refused with the
// keychain entry it looked for; keys are kept per provider and URL, set, read and deleted
#[test]
fn keys_are_kept_by_provider() {
    let keys = MemoryKeys::new();
    let anthropic = ProviderConfig::anthropic();
    let missing = ProviderModel::connect(anthropic.clone(), &keys)
        .err()
        .unwrap();
    assert!(matches!(&missing, ProviderError::NoKey(name) if name == "anthropic"));
    assert_eq!(missing.to_string(), "no API key is stored for anthropic");
    let mut other = ProviderConfig::new(ProviderKind::OpenAiCompatible, "m");
    other.base_url = Some("https://a.test/v1".to_owned());
    assert_eq!(other.key_name(), "openai-compatible https://a.test/v1");
    keys.set("anthropic", &Secret::new("a".to_owned())).unwrap();
    assert_eq!(
        keys.get("anthropic").unwrap(),
        Some(Secret::new("a".to_owned()))
    );
    assert_eq!(keys.get(&other.key_name()).unwrap(), None);
    keys.delete("anthropic").unwrap();
    keys.delete("anthropic").unwrap();
    assert_eq!(keys.get("anthropic").unwrap(), None);
    assert!(ProviderModel::connect(anthropic, &keys).is_err());
    assert_eq!(
        KeyError::Unavailable("locked".to_owned()).to_string(),
        "the keychain is not available: locked"
    );
}

// No spec statement: AGT-6, the OS keychain answers a missing entry with none, or says it is
// unavailable (a CI machine with no Secret Service), and never fails another way
#[test]
fn the_keychain_answers_or_says_why_not() {
    match Keychain.get("qs-models-test-entry-that-does-not-exist") {
        Ok(found) => assert_eq!(found, None),
        Err(KeyError::Unavailable(_) | KeyError::Store(_)) => {}
    }
    let _http = Http::default();
}

// No spec statement: P4-09, an Anthropic reply streams: its text comes piece by piece as it is
// written, a thinking block's thinking and signature deltas are joined and sent back whole, a
// tool call's input_json_delta fragments become its input, and the request asks for a stream;
// an error event ends the reply with its message
#[test]
fn anthropic_streams_text_thinking_and_calls() {
    let events = json!([
        { "type": "message_start", "message": { "content": [], "usage": { "input_tokens": 50, "output_tokens": 1 } } },
        { "type": "content_block_start", "index": 0,
          "content_block": { "type": "thinking", "thinking": "" } },
        { "type": "content_block_delta", "index": 0,
          "delta": { "type": "thinking_delta", "thinking": "plan" } },
        { "type": "content_block_delta", "index": 0,
          "delta": { "type": "signature_delta", "signature": "sig" } },
        { "type": "content_block_stop", "index": 0 },
        { "type": "content_block_start", "index": 1,
          "content_block": { "type": "text", "text": "" } },
        { "type": "content_block_delta", "index": 1,
          "delta": { "type": "text_delta", "text": "Stag" } },
        { "type": "content_block_delta", "index": 1,
          "delta": { "type": "text_delta", "text": "ing it." } },
        { "type": "content_block_stop", "index": 1 },
        { "type": "content_block_start", "index": 2,
          "content_block": { "type": "tool_use", "id": "toolu_1", "name": "edit_apply", "input": {} } },
        { "type": "content_block_delta", "index": 2,
          "delta": { "type": "input_json_delta", "partial_json": "{\"files\":" } },
        { "type": "content_block_delta", "index": 2,
          "delta": { "type": "input_json_delta", "partial_json": " []}" } },
        { "type": "content_block_stop", "index": 2 },
        { "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 9 } },
        { "type": "message_stop" },
    ]);
    let failed = json!([{ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } }]);
    let finished = json!([]);
    let transport = Scripted::new(vec![Ok(events), Ok(failed), Ok(finished)]);
    let mut model = ProviderModel::new(
        ProviderConfig::anthropic(),
        Some(Secret::new(KEY.to_owned())),
        &transport,
    )
    .unwrap();
    let mut transcript = vec![Message::User {
        text: "go".to_owned(),
    }];
    let mut pieces = Vec::new();
    let reply = model
        .reply_streaming("s", &tools(), &transcript, &mut |p| {
            pieces.push(p.to_owned())
        })
        .unwrap();
    assert_eq!(pieces, ["Stag", "ing it."]);
    let Reply::Calls { text, calls } = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(text, "Staging it.");
    assert_eq!(calls[0].name, "edit.apply");
    assert_eq!(calls[0].arguments, json!({ "files": [] }));
    assert_eq!(model.usage().input_tokens, 50);
    assert_eq!(model.usage().output_tokens, 9, "the last delta's count");
    transcript.push(Message::Assistant { text, calls });
    let error = model
        .reply_streaming("s", &tools(), &transcript, &mut |_| {})
        .unwrap_err();
    assert_eq!(error, "Overloaded");
    let sent = transport.sent.borrow();
    assert_eq!(sent[0].2["stream"], true);
    let back = &sent[1].2["messages"][1]["content"][0];
    assert_eq!(back["thinking"], "plan");
    assert_eq!(back["signature"], "sig");
}

// No spec statement: P4-09, an OpenAI-compatible reply streams: its content comes piece by
// piece, and a tool call's id, name and arguments, spread over chunks by index, come back
// whole with the finish_reason
#[test]
fn openai_streams_text_and_calls() {
    let chunks = json!([
        { "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "On " } }] },
        { "choices": [{ "index": 0, "delta": { "content": "it." } }] },
        { "choices": [{ "index": 0, "delta": { "tool_calls": [
            { "index": 0, "id": "c1", "type": "function",
              "function": { "name": "edit_", "arguments": "{\"fi" } }] } }] },
        { "choices": [{ "index": 0, "delta": { "tool_calls": [
            { "index": 0, "function": { "name": "apply", "arguments": "les\":[]}" } }] } }] },
        { "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] },
    ]);
    let transport = Scripted::new(vec![Ok(chunks)]);
    let config = ProviderConfig::new(ProviderKind::OpenAiCompatible, "m");
    let mut model =
        ProviderModel::new(config, Some(Secret::new(KEY.to_owned())), &transport).unwrap();
    let mut pieces = Vec::new();
    let reply = model
        .reply_streaming(
            "s",
            &tools(),
            &[Message::User {
                text: "go".to_owned(),
            }],
            &mut |p| pieces.push(p.to_owned()),
        )
        .unwrap();
    assert_eq!(pieces, ["On ", "it."]);
    let Reply::Calls { text, calls } = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(text, "On it.");
    assert_eq!(calls[0].id, "c1");
    assert_eq!(calls[0].name, "edit.apply");
    assert_eq!(calls[0].arguments, json!({ "files": [] }));
}

// No spec statement: P4-09, PRIN-9, a local Ollama streams over the real HTTP client: the
// server-sent events are read line by line until [DONE]
#[test]
fn ollama_streams_over_http() {
    let first = json!({ "choices": [{ "index": 0, "delta": { "content": "Hel" } }] });
    let last = json!({ "choices": [{ "index": 0, "delta": { "content": "lo" }, "finish_reason": "stop" }] });
    let body = format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n");
    let (base, server) = serve(vec![(200, body)]);
    let mut config = ProviderConfig::new(ProviderKind::Ollama, "qwen3-coder");
    config.base_url = Some(format!("{base}/v1"));
    let mut model = ProviderModel::connect(config, &MemoryKeys::new()).unwrap();
    let mut pieces = Vec::new();
    let reply = model
        .reply_streaming(
            "s",
            &[],
            &[Message::User {
                text: "hi".to_owned(),
            }],
            &mut |p| {
                pieces.push(p.to_owned());
            },
        )
        .unwrap();
    assert_eq!(pieces, ["Hel", "lo"]);
    assert_eq!(
        reply,
        Reply::Finish {
            summary: "Hello".to_owned()
        }
    );
    let requests = server.join().unwrap();
    assert!(requests[0].contains("\"stream\":true"), "{}", requests[0]);
}

// No spec statement: P4-15, a model's replies add up the tokens each provider reports, and
// their cost is the tokens at the prices given per million
#[test]
fn usage_adds_up_what_the_providers_report() {
    let transport = Scripted::new(vec![
        Ok(
            json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": "a" }],
                   "usage": { "input_tokens": 1000, "output_tokens": 200 } }),
        ),
        Ok(json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": "b" }] })),
    ]);
    let mut model = ProviderModel::new(
        ProviderConfig::anthropic(),
        Some(Secret::new(KEY.to_owned())),
        &transport,
    )
    .unwrap();
    let transcript = [Message::User {
        text: "go".to_owned(),
    }];
    model.reply("s", &[], &transcript).unwrap();
    model.reply("s", &[], &transcript).unwrap();
    let usage = model.usage();
    assert_eq!((usage.input_tokens, usage.output_tokens), (1000, 200));
    assert!((usage.cost(4.0, 20.0) - 0.008).abs() < 1e-12);
    let chunks = json!([{ "choices": [{ "index": 0, "delta": { "content": "x" }, "finish_reason": "stop" }] }]);
    let open = Scripted::new(vec![
        Ok(json!({ "choices": [{ "finish_reason": "stop",
        "message": { "content": "x" } }], "usage": { "prompt_tokens": 7, "completion_tokens": 3 } })),
        Ok(chunks),
    ]);
    let mut other = ProviderModel::new(
        ProviderConfig::new(ProviderKind::OpenAiCompatible, "m"),
        Some(Secret::new(KEY.to_owned())),
        &open,
    )
    .unwrap();
    other.reply("s", &[], &transcript).unwrap();
    assert_eq!(other.usage().input_tokens, 7);
    assert_eq!(other.usage().output_tokens, 3);
}
