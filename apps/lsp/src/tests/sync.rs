// SPDX-License-Identifier: GPL-3.0-or-later

//! Document synchronization and published diagnostics.

use serde_json::{Value, json};

use super::{notification, position, project, request, started, uri};

/// The `didOpen` of file `name` with `text` at `version`.
fn open(name: &str, text: &str, version: i32) -> Value {
    notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri(name),
                "languageId": "quakec",
                "version": version,
                "text": text,
            },
        }),
    )
}

// LSP 3.18, "PublishDiagnostics Notification": "When a file changes, it is the server's
// responsibility to re-compute diagnostics and push them to the client. If the computed set is
// empty, the server has to push the empty array to clear former diagnostics."
#[test]
fn diagnostics_are_published_and_cleared() {
    let broken = "void() f = { health = 1; };\n";
    let s = started(
        project(&[("a.qc", broken)]),
        &[
            open("a.qc", broken, 1),
            notification(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri("a.qc"), "version": 2 },
                    "contentChanges": [{ "text": "float health;\nvoid() f = { health = 1; };\n" }],
                }),
            ),
        ],
    );
    let published = s.notifications("textDocument/publishDiagnostics");
    // On `initialized`, the error; the open changes nothing; the fix clears it.
    assert_eq!(published.len(), 2, "{published:?}");
    let first = &published[0];
    assert_eq!(first["uri"], json!(uri("a.qc")));
    assert_eq!(first.get("version"), None);
    let d = &first["diagnostics"][0];
    assert_eq!(d["severity"], json!(1));
    assert_eq!(d["source"], json!("quakec"));
    assert_eq!(d["range"]["start"], position(broken, "health", 0));
    assert!(d["code"].as_str().unwrap().starts_with('E'));
    assert_eq!(published[1]["diagnostics"], json!([]));
    assert_eq!(published[1]["version"], json!(2));
}

// LSP 3.18, "DidChangeTextDocument Notification": "apply the `TextDocumentContentChangeEvent`s
// in a single notification in the order you receive them", each range in the text the change
// before it left.
#[test]
fn incremental_changes_apply_in_order() {
    let text = "float a;\nvoid() f = { b = 1; };\n";
    let s = started(
        project(&[("a.qc", text)]),
        &[
            open("a.qc", text, 1),
            notification(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri("a.qc"), "version": 2 },
                    "contentChanges": [
                        // `a` becomes `bb`, then the first `b` of it is deleted.
                        { "range": { "start": { "line": 0, "character": 6 },
                                     "end": { "line": 0, "character": 7 } }, "text": "bb" },
                        { "range": { "start": { "line": 0, "character": 6 },
                                     "end": { "line": 0, "character": 7 } }, "text": "" },
                    ],
                }),
            ),
            request(
                1,
                "textDocument/definition",
                json!({ "textDocument": { "uri": uri("a.qc") }, "position": { "line": 1, "character": 13 } }),
            ),
        ],
    );
    let published = s.notifications("textDocument/publishDiagnostics");
    assert_eq!(published.last().unwrap()["diagnostics"], json!([]));
    assert_eq!(
        s.result(1)["range"]["start"],
        json!({ "line": 0, "character": 6 })
    );
}

// LSP 3.18, "DidCloseTextDocument Notification": "The document's master now exists where the
// document's Uri points to", so after a close the text on disk is analysed again.
#[test]
fn closing_returns_to_the_text_on_disk() {
    let disk = "float a;\n";
    let s = started(
        project(&[("a.qc", disk)]),
        &[
            open("a.qc", "float a = b;\n", 1),
            notification(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": uri("a.qc") } }),
            ),
            notification(
                "textDocument/didSave",
                json!({ "textDocument": { "uri": uri("a.qc") } }),
            ),
            // A document whose URI is not a file is not the server's.
            notification(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": "untitled:1", "languageId": "quakec",
                                          "version": 1, "text": "x" } }),
            ),
        ],
    );
    let published = s.notifications("textDocument/publishDiagnostics");
    assert_eq!(published.len(), 2, "{published:?}");
    assert_ne!(published[0]["diagnostics"], json!([]));
    assert_eq!(published[1]["diagnostics"], json!([]));
}

// LSP 3.18, "DidOpenTextDocument Notification": "The document's content is now managed by the
// client and the server must not try to read the document's content using the document's Uri",
// so an open file no module compiles is analysed from the text the client sent.
#[test]
fn an_open_file_outside_the_project() {
    let scratch = "void() f = { nothing = 1; };\n";
    let s = started(project(&[("a.qc", "")]), &[open("scratch.qc", scratch, 3)]);
    let published = s.notifications("textDocument/publishDiagnostics");
    assert_eq!(published.len(), 1);
    assert_eq!(published[0]["uri"], json!(uri("scratch.qc")));
    assert_eq!(published[0]["version"], json!(3));
}
