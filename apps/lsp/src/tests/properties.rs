// SPDX-License-Identifier: GPL-3.0-or-later

//! Properties of the framing, the position conversion, the URI conversion and whole sessions
//! (TEST-4), added by the P2-RA readiness review.

use std::path::PathBuf;

use proptest::prelude::*;
use qc_project::MemoryLoader;
use serde_json::{Value, json};

use super::{framed, initialize, notification, project, request, unframed, uri};
use crate::lines::{Encoding, LineIndex, Position};
use crate::transport::{read_message, write_message};
use crate::uri::{Host, from_path, from_path_on, to_path, to_path_on};
use crate::{Exit, run};

/// Bytes that are mostly the ones line breaks and encodings turn on: CR, LF, ASCII, the lead
/// and continuation bytes of multi-byte UTF-8, and Quake's 0x80 to 0xFF graphics.
fn text() -> impl Strategy<Value = Vec<u8>> {
    let byte = prop_oneof![
        Just(b'\r'),
        Just(b'\n'),
        Just(b'a'),
        Just(0xC3),
        Just(0xA9),
        Just(0xF0),
        Just(0x90),
        Just(0x80),
        Just(0xE2),
        Just(0x82),
        any::<u8>(),
    ];
    prop::collection::vec(byte, 0..48)
}

/// The byte offsets of `text` a client can name: the start of each character of its UTF-8
/// decoding, each sequence that is not UTF-8 being one character, and the end; less an offset
/// between the CR and LF of one line break, which names the same place as the CR.
fn boundaries(text: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = 0_usize;
    for chunk in text.utf8_chunks() {
        for c in chunk.valid().chars() {
            out.push(at);
            at += c.len_utf8();
        }
        if !chunk.invalid().is_empty() {
            out.push(at);
            at += chunk.invalid().len();
        }
    }
    out.push(at);
    out.retain(|&o| !(o > 0 && text.get(o - 1) == Some(&b'\r') && text.get(o) == Some(&b'\n')));
    out
}

fn encoding() -> impl Strategy<Value = Encoding> {
    prop_oneof![Just(Encoding::Utf16), Just(Encoding::Utf8)]
}

/// A [`component`] of a Windows path, whatever the host the test runs on: no `\`.
fn windows_component() -> impl Strategy<Value = String> {
    component().prop_filter("a separator on Windows", |c| !c.contains('\\'))
}

/// A path component with no `/`: on Windows no `\`, its separator, and not a drive letter's
/// `x:`; on any other host both are bytes of a name like any other (P2-161).
fn component() -> impl Strategy<Value = String> {
    let windows = cfg!(windows);
    "[a-zA-Z0-9 ._~%#?:@!$&'()*+,;=\\\\\u{e9}\u{4e2d}-]{1,8}"
        .prop_filter("not . or ..", |c| c != "." && c != "..")
        .prop_filter("a separator on Windows", move |c| {
            !(windows && c.contains('\\'))
        })
        .prop_filter("a drive letter on Windows", move |c| {
            let b = c.as_bytes();
            !(windows && b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
        })
}

/// A request or notification a client may send after `initialize`, with params of any of the
/// shapes the methods take or of the wrong shape.
fn message() -> impl Strategy<Value = Value> {
    let method = prop::sample::select(vec![
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
        "workspace/symbol",
        "initialize",
    ]);
    let position = (0_u32..4, 0_u32..40)
        .prop_map(|(line, character)| json!({ "line": line, "character": character }));
    let file = prop::sample::select(vec!["a.qc", "b.qc", "progs.src", "none.qc"]);
    let params = (
        file,
        position.clone(),
        position,
        "[a-z ;={}().\n]{0,24}",
        any::<bool>(),
    )
        .prop_map(|(file, start, end, text, whole)| {
            let range = json!({ "start": start, "end": end });
            json!({
                "textDocument": {
                    "uri": uri(file), "version": 1, "languageId": "quakec", "text": text,
                },
                "position": start,
                "range": range,
                "newName": text,
                "context": { "includeDeclaration": whole },
                "contentChanges": if whole {
                    json!([{ "text": text }])
                } else {
                    json!([{ "range": range, "text": text }])
                },
            })
        });
    let params = prop_oneof![
        4 => params,
        1 => Just(Value::Null),
        1 => Just(json!({ "textDocument": 7 })),
    ];
    (method, params, prop::option::of(1_i64..1000)).prop_map(|(method, params, id)| match id {
        Some(id) => request(id, method, params),
        None => notification(method, params),
    })
}

proptest! {
    // LSP 3.18, "Header Part": `Content-Length` is "The length of the content part in bytes",
    // so every content written is read back whole, message after message.
    #[test]
    fn written_messages_read_back(contents in prop::collection::vec(
        prop::collection::vec(any::<u8>(), 0..64), 0..6,
    )) {
        let mut out = Vec::new();
        for content in &contents {
            write_message(&mut out, content).unwrap();
        }
        let mut input = out.as_slice();
        for content in &contents {
            let read = read_message(&mut input).unwrap();
            prop_assert_eq!(read.as_ref(), Some(content));
        }
        prop_assert!(read_message(&mut input).unwrap().is_none());
    }

    // The reader of the client's stream ends every input, whatever its bytes, with a message,
    // the end, or an error: never a panic and never a read past the input.
    #[test]
    fn arbitrary_input_never_panics_the_reader(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let mut input = bytes.as_slice();
        for _ in 0..=bytes.len() {
            match read_message(&mut input) {
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
    }

    // LSP 3.18, "Text Documents": a position names a place in the text; the place a byte
    // offset is at converts back to that offset, in either encoding.
    #[test]
    fn offsets_convert_to_positions_and_back(text in text(), encoding in encoding()) {
        let lines = LineIndex::new(&text, encoding);
        for offset in boundaries(&text) {
            let position = lines.position(offset);
            prop_assert_eq!(lines.offset(position), offset, "{:?}", position);
        }
    }

    // LSP 3.18, `Position`: positions are ordered as the text is, so a later offset at a
    // character's start is never at an earlier position, and every position converts to an
    // offset in the text.
    #[test]
    fn positions_follow_the_text(
        text in text(),
        encoding in encoding(),
        line in 0_u32..12,
        character in 0_u32..64,
    ) {
        let lines = LineIndex::new(&text, encoding);
        let mut last = Position::default();
        // P2-161: an offset inside a character is that character's position, so every offset
        // is in order, not only a character's start.
        for offset in (0..=text.len()).chain([text.len() + 2]) {
            let position = lines.position(offset);
            prop_assert!(position >= last, "{offset}: {position:?} before {last:?}");
            last = position;
        }
        let far = Position { line, character };
        prop_assert!(lines.offset(far) <= text.len());
    }

    // LSP 3.18, "URI": the server writes the URI of each path so that it reads back as that
    // path; every other byte is percent-encoded.
    #[test]
    fn paths_convert_to_uris_and_back(parts in prop::collection::vec(component(), 1..5)) {
        let path = PathBuf::from(format!("/{}", parts.join("/")));
        let written = from_path(&path);
        prop_assert!(written.starts_with("file:///"));
        let rest = &written["file://".len()..];
        prop_assert!(
            rest.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~/%".contains(&b)),
            "{}", written
        );
        prop_assert_eq!(to_path(&written), Some(path));
    }

    // P3-37: on a Windows host a UNC path `\\host\share\...` is written with its host as the
    // URI's, as VS Code's `URI.file` writes it, and reads back as that path with its host in
    // lower case.
    #[test]
    fn unc_paths_convert_to_uris_and_back(
        server in windows_component().prop_filter("a verbatim or device prefix", |c| c != "?"),
        parts in prop::collection::vec(windows_component(), 1..5),
    ) {
        let path = format!("\\\\{server}\\{}", parts.join("\\"));
        let written = from_path_on(std::path::Path::new(&path), Host::Windows);
        let rest = &written["file://".len()..];
        prop_assert!(!rest.starts_with('/'), "{}", written);
        prop_assert!(
            rest.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~/%".contains(&b)),
            "{}", written
        );
        // vscode-uri writes the host in lower case, and Windows reads a host in any case.
        let lower = format!("\\\\{}\\{}", server.to_ascii_lowercase(), parts.join("\\"));
        prop_assert_eq!(to_path_on(&written, Host::Windows), Some(PathBuf::from(lower)));
        prop_assert_eq!(to_path_on(&written, Host::Unix), None);
    }

    // LSP 3.18, "Response Message": "A Response Message sent as a result of a request"; every
    // request after `initialize`, whatever its params, gets exactly one answer with its id, and
    // the session ends as the lifecycle says.
    #[test]
    fn every_request_is_answered_once(messages in prop::collection::vec(message(), 0..12)) {
        let loader: MemoryLoader = project(&[
            ("a.qc", "float x;\nvoid() f = { x = 1; };\n"),
            ("b.qc", "void() g = { f(); };\n"),
        ]);
        let mut all = vec![initialize(), notification("initialized", json!({}))];
        all.extend(messages.iter().cloned());
        all.push(request(0x7fff, "shutdown", Value::Null));
        all.push(notification("exit", Value::Null));
        let mut output = Vec::new();
        let exit = run(loader, framed(&all).as_slice(), &mut output).unwrap();
        prop_assert_eq!(exit, Exit::Success);
        let answers = unframed(&output);
        for sent in all.iter().filter(|m| m.get("id").is_some()) {
            let count = answers
                .iter()
                .filter(|a| a.get("method").is_none() && a.get("id") == sent.get("id"))
                .count();
            let duplicates = all.iter().filter(|m| m.get("id") == sent.get("id")).count();
            prop_assert_eq!(count, duplicates, "{}", sent);
        }
    }
}
