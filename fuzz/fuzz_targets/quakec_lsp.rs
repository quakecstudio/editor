// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary input to the language server, `quakec_lsp::run`, on a small in-memory
//! project (P2-165, TEST-5): the bytes as the client's stream, and a session the bytes draw,
//! `initialize` then requests and notifications with methods, positions, ranges and URIs taken
//! from the input. Fails on a panic, on an answer to no request or two answers to one, and on
//! a position outside the open file's text.

#![no_main]

use std::collections::BTreeMap;
use std::path::Path;

use libfuzzer_sys::fuzz_target;
use qc_project::MemoryLoader;
use serde_json::{Value, json};

/// The project's one file, whose text the session's edits change.
const SOURCE: &str =
    "float speed;\nvoid() worldspawn = {\n\tspeed = 300;\n\tlocal float \u{e9} = speed;\n};\n";

/// The methods a drawn message takes.
const METHODS: [&str; 14] = [
    "textDocument/definition",
    "textDocument/references",
    "textDocument/prepareRename",
    "textDocument/rename",
    "textDocument/hover",
    "textDocument/completion",
    "textDocument/semanticTokens/full",
    "textDocument/inlayHint",
    "textDocument/didOpen",
    "textDocument/didChange",
    "textDocument/didClose",
    "textDocument/didSave",
    "$/cancelRequest",
    "workspace/didChangeConfiguration",
];

/// The URIs a drawn message names: the project's file, a file of another directory, a drive
/// letter's and one of another scheme.
const URIS: [&str; 4] = [
    "file:///p/a.qc",
    "file:///q/b.qc",
    "file:///c:/x.qc",
    "untitled:Untitled-1",
];

fn loader() -> MemoryLoader {
    let mut loader = MemoryLoader::new();
    loader.insert(Path::new("/p/progs.src"), b"../progs.dat\na.qc\n".to_vec());
    loader.insert(Path::new("/p/a.qc"), SOURCE.as_bytes().to_vec());
    loader
}

fn frame(message: &Value, out: &mut Vec<u8>) {
    let content = message.to_string();
    out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", content.len()).as_bytes());
    out.extend_from_slice(content.as_bytes());
}

/// The messages the server wrote.
fn answers(output: &[u8]) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut rest = output;
    while let Ok(Some(content)) = quakec_lsp::transport::read_message(&mut rest) {
        if let Ok(value) = serde_json::from_slice(&content) {
            messages.push(value);
        }
    }
    messages
}

/// The input's bytes, read one at a time, 0 once they run out.
struct Draw<'a> {
    bytes: &'a [u8],
}

impl Draw<'_> {
    fn byte(&mut self) -> u8 {
        let Some((&first, rest)) = self.bytes.split_first() else {
            return 0;
        };
        self.bytes = rest;
        first
    }

    fn pick<'s, T>(&mut self, items: &'s [T]) -> &'s T {
        &items[usize::from(self.byte()) % items.len()]
    }

    fn position(&mut self) -> Value {
        json!({ "line": self.byte() % 8, "character": self.byte() % 40 })
    }
}

/// A session drawn from `data`: `initialize`, `initialized`, the drawn messages, `shutdown` and
/// `exit`; and the ids of the requests in it, each once.
fn session(data: &[u8]) -> (Vec<u8>, Vec<i64>) {
    let mut draw = Draw { bytes: data };
    let mut input = Vec::new();
    let utf8 = draw.byte() % 2 == 0;
    let encodings = if utf8 {
        json!(["utf-8", "utf-16"])
    } else {
        json!(["utf-16"])
    };
    frame(
        &json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "processId": null, "rootUri": "file:///p",
            "capabilities": { "general": { "positionEncodings": encodings } },
            "initializationOptions": if draw.byte() % 2 == 0 { json!({ "dialect": "fteqcc" }) } else { json!(null) },
        }}),
        &mut input,
    );
    frame(
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
        &mut input,
    );
    let mut ids = vec![0];
    let mut next = 1_i64;
    while !draw.bytes.is_empty() && ids.len() < 64 {
        let method = *draw.pick(&METHODS);
        let uri = *draw.pick(&URIS);
        let document = json!({ "uri": uri, "version": draw.byte() });
        let range = json!({ "start": draw.position(), "end": draw.position() });
        let params = match method {
            "textDocument/didOpen" => json!({ "textDocument": {
                "uri": uri, "languageId": "quakec", "version": 1, "text": SOURCE } }),
            "textDocument/didChange" => {
                let text: String = (0..draw.byte() % 12)
                    .map(|_| char::from(b' ' + draw.byte() % 95))
                    .collect();
                let change = if draw.byte() % 2 == 0 {
                    json!({ "range": range, "text": text })
                } else {
                    json!({ "text": text })
                };
                json!({ "textDocument": document, "contentChanges": [change] })
            }
            "textDocument/rename" => json!({ "textDocument": { "uri": uri },
                "position": draw.position(), "newName": if draw.byte() % 2 == 0 { "hops" } else { "1x" } }),
            "textDocument/inlayHint" => json!({ "textDocument": { "uri": uri }, "range": range }),
            "textDocument/references" => json!({ "textDocument": { "uri": uri },
                "position": draw.position(), "context": { "includeDeclaration": draw.byte() % 2 == 0 } }),
            "$/cancelRequest" => json!({ "id": draw.byte() }),
            "workspace/didChangeConfiguration" => json!({ "settings": {} }),
            _ => json!({ "textDocument": { "uri": uri }, "position": draw.position() }),
        };
        let notifies = method.contains("/did") || method.starts_with('$');
        let message = if notifies {
            json!({ "jsonrpc": "2.0", "method": method, "params": params })
        } else {
            ids.push(next);
            next += 1;
            json!({ "jsonrpc": "2.0", "id": next - 1, "method": method, "params": params })
        };
        frame(&message, &mut input);
    }
    ids.push(next);
    frame(
        &json!({ "jsonrpc": "2.0", "id": next, "method": "shutdown", "params": null }),
        &mut input,
    );
    frame(
        &json!({ "jsonrpc": "2.0", "method": "exit", "params": null }),
        &mut input,
    );
    (input, ids)
}

/// Every `line` of a position in `value`, at any depth.
fn lines(value: &Value, out: &mut Vec<u64>) {
    match value {
        Value::Object(map) => {
            if let (Some(line), Some(_)) = (map.get("line"), map.get("character"))
                && let Some(line) = line.as_u64()
            {
                out.push(line);
            }
            map.values().for_each(|v| lines(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| lines(v, out)),
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    // The bytes as the client's stream: the server ends, however the stream is wrong.
    let mut output = Vec::new();
    let _ = quakec_lsp::run(loader(), data, &mut output);

    // A drawn session: one answer per request, to a request that was sent.
    let (input, ids) = session(data);
    let mut output = Vec::new();
    let _ = quakec_lsp::run(loader(), input.as_slice(), &mut output);
    let mut answered: BTreeMap<i64, usize> = BTreeMap::new();
    for message in answers(&output) {
        if message.get("method").is_some() {
            continue;
        }
        if let Some(id) = message.get("id").and_then(Value::as_i64) {
            assert!(ids.contains(&id), "an answer to no request: {message}");
            *answered.entry(id).or_default() += 1;
        }
        // A position the server gives in the project's file is inside some line the edits
        // could have made: no more lines than the text and the edits' bytes can hold.
        let mut found = Vec::new();
        lines(&message, &mut found);
        let most = u64::try_from(SOURCE.lines().count() + data.len() + 2).unwrap_or(u64::MAX);
        assert!(found.iter().all(|&line| line <= most), "{message}");
    }
    for (id, count) in answered {
        assert_eq!(count, 1, "request {id} answered {count} times");
    }
});
