// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary input to the language server, `quakec_lsp::run`, on a small in-memory
//! project (P2-165, TEST-5): the bytes as the client's stream, and a session the bytes draw,
//! `initialize` then requests and notifications with methods, positions, ranges and URIs taken
//! from the input, and edits whose text holds line breaks and characters of two, three and four
//! UTF-8 bytes (P3-38). Fails on a panic, on a request answered other than once (LSP 3.18,
//! "Response Message": "A Response Message sent as a result of a request"), and on a position in
//! an answer that is not a position of its document's text when the request was read, the text
//! the target tracks from the session's opens, edits and closes as the server keeps it.

#![no_main]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use libfuzzer_sys::fuzz_target;
use qc_project::MemoryLoader;
use quakec_lsp::lines::{Encoding, LineIndex, Position};
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

/// The pieces a drawn edit's text is made of: printable ASCII stands for itself, and these
/// add the line breaks and the characters whose UTF-8 and UTF-16 widths differ, which move
/// positions (P2-161).
const PIECES: [&str; 6] = ["\n", "\r\n", "\r", "\u{e9}", "\u{4e2d}", "\u{1f600}"];

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
/// `exit`, framed; and the messages in order.
fn session(data: &[u8]) -> (Vec<u8>, Vec<Value>) {
    let mut draw = Draw { bytes: data };
    let mut messages = Vec::new();
    let utf8 = draw.byte() % 2 == 0;
    let encodings = if utf8 {
        json!(["utf-8", "utf-16"])
    } else {
        json!(["utf-16"])
    };
    messages.push(json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "processId": null, "rootUri": "file:///p",
            "capabilities": { "general": { "positionEncodings": encodings } },
            "initializationOptions": if draw.byte() % 2 == 0 { json!({ "dialect": "fteqcc" }) } else { json!(null) },
        }}));
    messages.push(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    let mut next = 1_i64;
    while !draw.bytes.is_empty() && next < 64 {
        let method = *draw.pick(&METHODS);
        let uri = *draw.pick(&URIS);
        let document = json!({ "uri": uri, "version": draw.byte() });
        let range = json!({ "start": draw.position(), "end": draw.position() });
        let params = match method {
            "textDocument/didOpen" => json!({ "textDocument": {
                "uri": uri, "languageId": "quakec", "version": 1, "text": SOURCE } }),
            "textDocument/didChange" => {
                let text: String = (0..draw.byte() % 12)
                    .map(|_| match draw.byte() {
                        b @ 0..=94 => char::from(b' ' + b).to_string(),
                        b => (*draw.pick(&PIECES)).to_owned() + &"x".repeat(usize::from(b % 2)),
                    })
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
            next += 1;
            json!({ "jsonrpc": "2.0", "id": next - 1, "method": method, "params": params })
        };
        messages.push(message);
    }
    messages.push(json!({ "jsonrpc": "2.0", "id": next, "method": "shutdown", "params": null }));
    messages.push(json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
    let mut input = Vec::new();
    for message in &messages {
        frame(message, &mut input);
    }
    (input, messages)
}

/// The text of each document as the server keeps it: a `file` URI's path, normalized, to the
/// open document's text, else the project's file on disk, else none (`Server::did_change` reads
/// `host.text(&path).unwrap_or_default()`).
struct Documents {
    open: BTreeMap<PathBuf, Vec<u8>>,
}

impl Documents {
    fn key(uri: &str) -> Option<PathBuf> {
        quakec_lsp::uri::to_path(uri).map(|path| qc_project::normalize(&path))
    }

    fn text(&self, path: &Path) -> Vec<u8> {
        match self.open.get(path) {
            Some(text) => text.clone(),
            None if path == Path::new("/p/a.qc") => SOURCE.as_bytes().to_vec(),
            None => Vec::new(),
        }
    }

    /// Applies the notification `method` with `params`, as `Server::did_open`, `did_change` and
    /// `did_close` do, positions counted in `encoding`.
    fn apply(&mut self, method: &str, params: &Value, encoding: Encoding) {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let Some(path) = Self::key(uri) else {
            return;
        };
        match method {
            "textDocument/didOpen" => {
                let text = params["textDocument"]["text"].as_str().unwrap_or_default();
                self.open.insert(path, text.as_bytes().to_vec());
            }
            "textDocument/didChange" => {
                let mut text = self.text(&path);
                for change in params["contentChanges"].as_array().into_iter().flatten() {
                    let inserted = change["text"].as_str().unwrap_or_default().as_bytes();
                    text = match position_pair(&change["range"]) {
                        None => inserted.to_vec(),
                        Some((start, end)) => {
                            let lines = LineIndex::new(&text, encoding);
                            let start = lines.offset(start);
                            let end = lines.offset(end).max(start);
                            let mut edited = text[..start].to_vec();
                            edited.extend_from_slice(inserted);
                            edited.extend_from_slice(&text[end..]);
                            edited
                        }
                    };
                }
                self.open.insert(path, text);
            }
            "textDocument/didClose" => {
                self.open.remove(&path);
            }
            _ => {}
        }
    }
}

/// The start and end of a range.
fn position_pair(range: &Value) -> Option<(Position, Position)> {
    Some((as_position(&range["start"])?, as_position(&range["end"])?))
}

/// `value` as a position, if it is one.
fn as_position(value: &Value) -> Option<Position> {
    let line = u32::try_from(value.get("line")?.as_u64()?).ok()?;
    let character = u32::try_from(value.get("character")?.as_u64()?).ok()?;
    Some(Position { line, character })
}

/// Every position in the answer `value`, at any depth, with the path of the document it is in:
/// the `uri` of the object holding it, the key of a `WorkspaceEdit`'s `changes`, or `document`,
/// the request's.
fn positions(
    value: &Value,
    document: Option<&PathBuf>,
    out: &mut Vec<(Option<PathBuf>, Position)>,
) {
    match value {
        Value::Object(map) => {
            let own = map.get("uri").and_then(Value::as_str).map(Documents::key);
            let document = match &own {
                Some(key) => key.as_ref(),
                None => document,
            };
            if let Some(position) = as_position(value) {
                out.push((document.cloned(), position));
            }
            for (key, item) in map {
                if key == "changes"
                    && let Value::Object(changes) = item
                {
                    for (uri, edits) in changes {
                        positions(edits, Documents::key(uri).as_ref(), out);
                    }
                } else {
                    positions(item, document, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| positions(v, document, out)),
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    // The bytes as the client's stream: the server ends, however the stream is wrong.
    let mut output = Vec::new();
    let _ = quakec_lsp::run(loader(), data, &mut output);

    // A drawn session: each request answered once, and every position the server gives inside
    // its document's text as it stood when the request was read.
    let (input, messages) = session(data);
    let mut output = Vec::new();
    let _ = quakec_lsp::run(loader(), input.as_slice(), &mut output);
    let answers: Vec<Value> = answers(&output)
        .into_iter()
        .filter(|message| message.get("method").is_none())
        .collect();
    let encoding = answers
        .iter()
        .find(|answer| answer["id"] == 0)
        .and_then(|answer| answer["result"]["capabilities"]["positionEncoding"].as_str())
        .map_or(Encoding::Utf16, |kind| {
            if kind == "utf-8" {
                Encoding::Utf8
            } else {
                Encoding::Utf16
            }
        });
    // The texts each request saw, by its id.
    let mut documents = Documents {
        open: BTreeMap::new(),
    };
    let mut seen: BTreeMap<i64, (Option<PathBuf>, BTreeMap<PathBuf, Vec<u8>>)> = BTreeMap::new();
    for message in &messages {
        let method = message["method"].as_str().unwrap_or_default();
        match message.get("id").and_then(Value::as_i64) {
            Some(id) => {
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                let mut texts = BTreeMap::new();
                let paths = documents.open.keys().cloned();
                for path in [PathBuf::from("/p/a.qc")].into_iter().chain(paths) {
                    let text = documents.text(&path);
                    texts.insert(path, text);
                }
                seen.insert(id, (Documents::key(uri), texts));
            }
            None => documents.apply(method, &message["params"], encoding),
        }
    }
    let mut answered: BTreeMap<i64, usize> = BTreeMap::new();
    for answer in &answers {
        let Some(id) = answer.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let Some((document, texts)) = seen.get(&id) else {
            panic!("an answer to no request: {answer}");
        };
        *answered.entry(id).or_default() += 1;
        let mut found = Vec::new();
        positions(answer, document.as_ref(), &mut found);
        for (path, position) in found {
            let text = path
                .and_then(|path| texts.get(&path).cloned())
                .unwrap_or_default();
            let lines = LineIndex::new(&text, encoding);
            assert_eq!(
                lines.position(lines.offset(position)),
                position,
                "a position outside its document's text: {answer}"
            );
        }
    }
    for id in seen.keys() {
        let count = answered.get(id).copied().unwrap_or(0);
        assert_eq!(count, 1, "request {id} answered {count} times");
    }
});
