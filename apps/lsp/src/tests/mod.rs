// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests of the language server: each drives a whole session through the base
//! protocol's framing, as a client would, and reads the answers back.
//!
//! The protocol is a public specification, cached in `specs/lsp/3.18/`; a test of protocol
//! behaviour quotes the sentence it pins in a comment above it. The analysis behind each
//! answer is `qc-ide`'s, whose tests cite the engine statements.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::string_slice,
    reason = "test code: a failed unwrap, expect, index, overflow or panic is a failed test"
)]

mod features;
mod lifecycle;
mod lines;
mod sync;
mod transport;
mod uri;

use std::path::Path;

use qc_project::MemoryLoader;
use serde_json::{Value, json};

use crate::transport::read_message;
use crate::uri::from_path;
use crate::{Exit, run};

/// A finished session: how it ended and every message the server wrote.
struct Session {
    exit: Exit,
    messages: Vec<Value>,
}

impl Session {
    /// The answer to request `id`.
    fn response(&self, id: i64) -> &Value {
        self.messages
            .iter()
            .find(|m| m.get("id") == Some(&json!(id)) && m.get("method").is_none())
            .unwrap_or_else(|| panic!("no answer to request {id} in {:?}", self.messages))
    }

    /// The result of request `id`.
    fn result(&self, id: i64) -> &Value {
        let response = self.response(id);
        response
            .get("result")
            .unwrap_or_else(|| panic!("request {id} failed: {response}"))
    }

    /// The error code of request `id`.
    fn error_code(&self, id: i64) -> i64 {
        self.response(id)["error"]["code"].as_i64().unwrap()
    }

    /// The notifications of `method`, in the order sent.
    fn notifications(&self, method: &str) -> Vec<&Value> {
        self.messages
            .iter()
            .filter(|m| m.get("method") == Some(&json!(method)))
            .map(|m| &m["params"])
            .collect()
    }
}

/// The bytes of `messages`, each framed as the base protocol frames it.
fn framed(messages: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for message in messages {
        let content = serde_json::to_vec(message).unwrap();
        out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", content.len()).as_bytes());
        out.extend_from_slice(&content);
    }
    out
}

/// Every message in `bytes`, unframed.
fn unframed(bytes: &[u8]) -> Vec<Value> {
    let mut input = bytes;
    let mut out = Vec::new();
    while let Some(content) = read_message(&mut input).unwrap() {
        out.push(serde_json::from_slice(&content).unwrap());
    }
    out
}

/// Runs a session of `messages` against `loader`'s files.
fn session(loader: MemoryLoader, messages: &[Value]) -> Session {
    let input = framed(messages);
    let mut output = Vec::new();
    let exit = run(loader, input.as_slice(), &mut output).expect("the session runs");
    Session {
        exit,
        messages: unframed(&output),
    }
}

/// How a session of the raw bytes `input` ends.
fn run_raw(input: &[u8]) -> Exit {
    let mut output = Vec::new();
    run(MemoryLoader::new(), input, &mut output).expect("the session runs")
}

/// A request.
fn request(id: i64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

/// A notification.
fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

/// The `initialize` request for the project in `/p`, with `options` as its
/// `initializationOptions` and `capabilities` as the client's.
fn initialize_with(options: Value, capabilities: Value) -> Value {
    request(
        0,
        "initialize",
        json!({
            "processId": null,
            "rootUri": "file:///p",
            "capabilities": capabilities,
            "initializationOptions": options,
        }),
    )
}

/// The ordinary `initialize`: a vanilla project, a client of UTF-16 positions, Markdown hovers
/// and prepare-rename.
fn initialize() -> Value {
    initialize_with(
        json!(null),
        json!({
            "textDocument": {
                "hover": { "contentFormat": ["markdown", "plaintext"] },
                "rename": { "prepareSupport": true },
            },
        }),
    )
}

/// `initialize`, then `initialized`, then `messages`, then `shutdown` and `exit`.
fn started(loader: MemoryLoader, messages: &[Value]) -> Session {
    let mut all = vec![initialize(), notification("initialized", json!({}))];
    all.extend_from_slice(messages);
    all.push(request(9999, "shutdown", json!(null)));
    all.push(notification("exit", json!(null)));
    session(loader, &all)
}

/// The URI of file `name` of the fixture project.
fn uri(name: &str) -> String {
    from_path(&Path::new("/p").join(name))
}

/// A project in `/p` whose `progs.src` lists `files` in order.
fn project(files: &[(&str, &str)]) -> MemoryLoader {
    let mut loader = MemoryLoader::new();
    let mut src = String::from("../progs.dat\n");
    for (name, text) in files {
        src.push_str(name);
        src.push('\n');
        loader.insert(Path::new("/p").join(name), text.as_bytes().to_vec());
    }
    loader.insert(Path::new("/p/progs.src"), src.into_bytes());
    loader
}

/// The position of the `n`-th `needle` in `text`, in UTF-16 code units.
fn position(text: &str, needle: &str, n: usize) -> Value {
    let at = text.match_indices(needle).nth(n).expect("the occurrence").0;
    let before = &text[..at];
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let character = before[start..].encode_utf16().count();
    json!({ "line": line, "character": character })
}
