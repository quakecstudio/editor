// SPDX-License-Identifier: GPL-3.0-or-later

//! The server lifecycle and the base protocol's errors.

use qc_project::MemoryLoader;
use serde_json::json;

use super::{
    framed, initialize, initialize_with, notification, project, request, run_raw, session, started,
};
use crate::Exit;
use crate::server::{TOKEN_MODIFIERS, TOKEN_TYPES};

// LSP 3.18, "Initialize Request": "For a request, the response should be an error with `code:
// -32002`." and "Notifications should be dropped, except for the exit notification."
#[test]
fn before_initialize() {
    let s = session(
        MemoryLoader::new(),
        &[
            request(1, "textDocument/hover", json!({})),
            notification("textDocument/didOpen", json!({})),
            notification("exit", json!(null)),
        ],
    );
    assert_eq!(s.error_code(1), -32002);
    assert_eq!(s.messages.len(), 1);
    // LSP 3.18, "Exit Notification": "otherwise with `error` code 1".
    assert_eq!(s.exit, Exit::Failure);
}

// LSP 3.18, "Shutdown Request": "If a server receives requests after a shutdown request those
// requests should error with `InvalidRequest`." "Exit Notification": "The server should exit
// with `success` code 0 if the shutdown request has been received before".
#[test]
fn shutdown_then_exit() {
    let s = session(
        project(&[("a.qc", "")]),
        &[
            initialize(),
            request(1, "initialize", json!({ "capabilities": {} })),
            request(2, "shutdown", json!(null)),
            request(3, "textDocument/hover", json!({})),
            notification("textDocument/didOpen", json!({})),
            notification("exit", json!(null)),
        ],
    );
    // "The `initialize` request may only be sent once."
    assert_eq!(s.error_code(1), -32600);
    assert_eq!(s.result(2), &json!(null));
    assert_eq!(s.error_code(3), -32600);
    assert_eq!(s.exit, Exit::Success);
}

// LSP 3.18, "Exit Notification": with no shutdown before it the server exits with code 1, and
// so it does when the input ends.
#[test]
fn exit_without_shutdown_and_end_of_input() {
    let s = session(
        project(&[("a.qc", "")]),
        &[initialize(), notification("exit", json!(null))],
    );
    assert_eq!(s.exit, Exit::Failure);
    let s = session(project(&[("a.qc", "")]), &[initialize()]);
    assert_eq!(s.exit, Exit::Failure);
    assert_eq!(run_raw(b""), Exit::Failure);
}

// LSP 3.18, `ServerCapabilities.positionEncoding`: "If the client didn't provide any position
// encodings the only valid value that a server can return is 'utf-16'."
#[test]
fn the_position_encoding() {
    let utf16 = started(project(&[("a.qc", "")]), &[]);
    let caps = &utf16.result(0)["capabilities"];
    assert_eq!(caps["positionEncoding"], json!("utf-16"));
    let offered = session(
        project(&[("a.qc", "")]),
        &[initialize_with(
            json!(null),
            json!({ "general": { "positionEncodings": ["utf-16", "utf-8"] } }),
        )],
    );
    assert_eq!(
        offered.result(0)["capabilities"]["positionEncoding"],
        json!("utf-8")
    );
}

// LSP 3.18, `renameProvider`: "`RenameOptions` may only be specified if the client states that
// it supports `prepareSupport` in its initial `initialize` request."
#[test]
fn the_capabilities() {
    let s = started(project(&[("a.qc", "")]), &[]);
    let caps = &s.result(0)["capabilities"];
    assert_eq!(caps["renameProvider"], json!({ "prepareProvider": true }));
    assert_eq!(caps["textDocumentSync"]["change"], json!(2));
    assert_eq!(caps["textDocumentSync"]["openClose"], json!(true));
    assert_eq!(caps["definitionProvider"], json!(true));
    assert_eq!(caps["referencesProvider"], json!(true));
    assert_eq!(caps["hoverProvider"], json!(true));
    assert_eq!(caps["inlayHintProvider"], json!(true));
    assert_eq!(
        caps["completionProvider"]["triggerCharacters"],
        json!(["."])
    );
    let legend = &caps["semanticTokensProvider"]["legend"];
    assert_eq!(legend["tokenTypes"], json!(TOKEN_TYPES));
    assert_eq!(legend["tokenModifiers"], json!(TOKEN_MODIFIERS));
    assert_eq!(s.result(0)["serverInfo"]["name"], json!("quakec-lsp"));
    let plain = session(
        project(&[("a.qc", "")]),
        &[initialize_with(json!(null), json!({}))],
    );
    assert_eq!(
        plain.result(0)["capabilities"]["renameProvider"],
        json!(true)
    );
}

// LSP 3.18, "$ Notifications and Requests": "If a server or client receives notifications
// starting with '$/' it is free to ignore the notification. If a server or client receives a
// request starting with '$/' it must error the request with error code `MethodNotFound`".
#[test]
fn unknown_methods() {
    let s = started(
        project(&[("a.qc", "")]),
        &[
            notification("$/cancelRequest", json!({ "id": 5 })),
            notification("workspace/didChangeConfiguration", json!({})),
            request(1, "$/custom", json!({})),
            request(2, "textDocument/formatting", json!({})),
        ],
    );
    assert_eq!(s.error_code(1), -32601);
    assert_eq!(s.error_code(2), -32601);
}

// LSP 3.18, "Error Codes": `ParseError` -32700 for content that is not JSON, `InvalidRequest`
// -32600 for one that is no request, `InvalidParams` -32602 for params of the wrong shape.
#[test]
fn malformed_messages() {
    let mut input = framed(&[initialize()]);
    input.extend_from_slice(b"Content-Length: 5\r\n\r\n{nope");
    input.extend_from_slice(&framed(&[
        json!({ "jsonrpc": "2.0", "id": 7 }),
        json!({ "jsonrpc": "2.0", "id": 8, "result": null }),
        request(9, "textDocument/hover", json!({ "position": 3 })),
    ]));
    let mut output = Vec::new();
    let exit = crate::run(project(&[("a.qc", "")]), input.as_slice(), &mut output).unwrap();
    assert_eq!(exit, Exit::Failure);
    let messages = super::unframed(&output);
    let codes: Vec<(serde_json::Value, i64)> = messages
        .iter()
        .filter_map(|m| Some((m["id"].clone(), m.get("error")?["code"].as_i64()?)))
        .collect();
    assert_eq!(
        codes,
        [
            (json!(null), -32700),
            (json!(7), -32600),
            (json!(9), -32602)
        ]
    );
}

// LSP 3.18, "Initialize Request": the server may not send notifications "until it has responded
// with an `InitializeResult`", so a project that does not open is reported after the answer.
#[test]
fn a_project_that_does_not_open() {
    let mut loader = MemoryLoader::new();
    loader.insert("/p/quakec.toml", b"[project\n".to_vec());
    let s = started(loader, &[]);
    let position = |m: &serde_json::Value| s.messages.iter().position(|x| x == m).unwrap();
    let shown = s.notifications("window/showMessage");
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0]["type"], json!(1));
    let answer = s.response(0).clone();
    let message = s
        .messages
        .iter()
        .find(|m| m.get("method") == Some(&json!("window/showMessage")))
        .unwrap()
        .clone();
    assert!(position(&answer) < position(&message));
}

// LSP 3.18, `InitializeParams.initializationOptions`: "User provided initialization options."
// An option a project file would not accept is a bad parameter.
#[test]
fn bad_initialization_options() {
    let s = session(
        project(&[("a.qc", "")]),
        &[initialize_with(json!({ "dialect": "klingon" }), json!({}))],
    );
    assert_eq!(s.error_code(0), -32602);
}
