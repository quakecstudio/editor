// SPDX-License-Identifier: GPL-3.0-or-later

//! Each language feature through the protocol.

use serde_json::{Value, json};

use super::{initialize_with, notification, position, project, request, session, started, uri};

/// The params of a request at the `n`-th `needle` of file `name` of text `text`.
fn at(name: &str, text: &str, needle: &str, n: usize) -> Value {
    json!({ "textDocument": { "uri": uri(name) }, "position": position(text, needle, n) })
}

/// The range of the `n`-th `needle`.
fn range(text: &str, needle: &str, n: usize) -> Value {
    let start = position(text, needle, n);
    let end = json!({
        "line": start["line"],
        "character": start["character"].as_u64().unwrap() + needle.encode_utf16().count() as u64,
    });
    json!({ "start": start, "end": end })
}

/// Two files: definitions and code that uses them, with a non-ASCII comment before the uses so
/// that a UTF-16 column is not a byte offset.
const DEFS: &str = "// The player's health.\nfloat health;\n\
                    void(entity e, vector o) setorigin = #2;\nentity self;\n";
const CODE: &str = "void(float amount) heal = {\n\t/* \u{e9}t\u{e9} */ health = health + amount;\n\
                    \tsetorigin(self, '0 0 0');\n};\n";

// LSP 3.18, "Go to Definition Request": "resolve the definition location of a symbol at a given
// text document position", a `Location`, or `null` where there is none.
#[test]
fn definition() {
    let s = started(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[
            request(
                1,
                "textDocument/definition",
                at("code.qc", CODE, "health", 0),
            ),
            request(2, "textDocument/definition", at("code.qc", CODE, "{", 0)),
        ],
    );
    assert_eq!(
        s.result(1),
        &json!({ "uri": uri("defs.qc"), "range": range(DEFS, "health", 1) })
    );
    assert_eq!(s.result(2), &json!(null));
}

// LSP 3.18, "Find References Request": "resolve project-wide references for the symbol denoted
// by the given text document position", with `includeDeclaration` to "Include the declaration
// of the current symbol".
#[test]
fn references() {
    let with = |include: bool| {
        let mut params = at("code.qc", CODE, "health", 1);
        params["context"] = json!({ "includeDeclaration": include });
        params
    };
    let s = started(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[
            request(1, "textDocument/references", with(true)),
            request(2, "textDocument/references", with(false)),
        ],
    );
    let uris = |id: i64| -> Vec<String> {
        s.result(id)
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["uri"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(uris(1), [uri("code.qc"), uri("code.qc"), uri("defs.qc")]);
    assert_eq!(uris(2), [uri("code.qc"), uri("code.qc")]);
    assert_eq!(s.result(2)[0]["range"], range(CODE, "health", 0));
}

// LSP 3.18, "Rename Request": the answer is a `WorkspaceEdit`; "If the given name is not valid,
// the request must return a ResponseError with an appropriate message set." "Prepare Rename
// Request": "If `null` is returned then it is deemed that a 'textDocument/rename' request is not
// valid at the given position."
#[test]
fn rename() {
    let mut renamed = at("code.qc", CODE, "health", 0);
    renamed["newName"] = json!("hp");
    let mut invalid = renamed.clone();
    invalid["newName"] = json!("2hp");
    let s = started(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[
            request(
                1,
                "textDocument/prepareRename",
                at("code.qc", CODE, "health", 1),
            ),
            request(2, "textDocument/prepareRename", at("code.qc", CODE, "{", 0)),
            request(3, "textDocument/rename", renamed),
            request(4, "textDocument/rename", invalid),
        ],
    );
    assert_eq!(s.result(1), &range(CODE, "health", 1));
    assert_eq!(s.result(2), &json!(null));
    let changes = &s.result(3)["changes"];
    let code = changes[uri("code.qc")].as_array().unwrap();
    assert_eq!(code.len(), 2);
    assert_eq!(
        code[0],
        json!({ "range": range(CODE, "health", 0), "newText": "hp" })
    );
    assert_eq!(changes[uri("defs.qc")].as_array().unwrap().len(), 1);
    assert_eq!(s.error_code(4), -32803);
    let message = s.response(4)["error"]["message"].as_str().unwrap();
    assert!(message.contains("not a QuakeC name"), "{message}");
}

// LSP 3.18, "Hover Request": a `Hover` whose contents are `MarkupContent` in a format the client
// lists in `contentFormat`, and `null` where there is nothing.
#[test]
fn hover() {
    let s = started(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[
            request(1, "textDocument/hover", at("code.qc", CODE, "health", 0)),
            request(2, "textDocument/hover", at("code.qc", CODE, "setorigin", 0)),
            request(3, "textDocument/hover", at("code.qc", CODE, "'0 0 0'", 0)),
        ],
    );
    let health = s.result(1);
    assert_eq!(health["range"], range(CODE, "health", 0));
    assert_eq!(health["contents"]["kind"], json!("markdown"));
    assert_eq!(
        health["contents"]["value"],
        json!("```quakec\nfloat health\n```\n\n---\n\nThe player's health.")
    );
    let builtin = s.result(2)["contents"]["value"].as_str().unwrap();
    assert!(builtin.contains("Builtin `#2`."), "{builtin}");
    assert!(
        builtin.contains("Vanilla has it as `void(entity e, vector o) setorigin`"),
        "{builtin}"
    );
    assert_eq!(s.result(3), &json!(null));
    // A client that lists no Markdown gets plain text.
    let plain = session(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[
            initialize_with(json!(null), json!({})),
            notification("initialized", json!({})),
            request(1, "textDocument/hover", at("code.qc", CODE, "setorigin", 0)),
        ],
    );
    let contents = &plain.result(1)["contents"];
    assert_eq!(contents["kind"], json!("plaintext"));
    assert!(
        contents["value"]
            .as_str()
            .unwrap()
            .starts_with("void(entity e, vector o) setorigin\n\nBuiltin #2.")
    );
}

// LSP 3.18, "Completion Request": a `CompletionItem[]` "is interpreted to be complete", each with
// a `label`, a `CompletionItemKind` and a `detail`.
#[test]
fn completion() {
    let s = started(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[request(
            1,
            "textDocument/completion",
            at("code.qc", CODE, "health =", 0),
        )],
    );
    let items = s.result(1).as_array().unwrap();
    let find = |label: &str| items.iter().find(|i| i["label"] == json!(label)).cloned();
    assert_eq!(
        find("health"),
        Some(json!({ "label": "health", "kind": 6, "detail": "float" }))
    );
    assert_eq!(find("heal").unwrap()["kind"], json!(3));
    assert_eq!(find("amount").unwrap()["kind"], json!(6));
}

// LSP 3.18, "Semantic Tokens": "Each token is represented using 5 integers", `deltaLine`,
// `deltaStart` "relative to 0 or the previous token's start if they are on the same line",
// `length`, `tokenType` and `tokenModifiers` as bits of the legend.
#[test]
fn semantic_tokens() {
    let text = "float g;\nvoid(float p) f = { g = p; };\n";
    let s = started(
        project(&[("a.qc", text)]),
        &[request(
            1,
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": uri("a.qc") } }),
        )],
    );
    // g (variable, declaration), p (parameter, declaration), f (function, declaration), g, p.
    assert_eq!(
        s.result(1)["data"],
        json!([
            0, 6, 1, 2, 1, 1, 11, 1, 3, 1, 0, 3, 1, 0, 1, 0, 6, 1, 2, 0, 0, 4, 1, 3, 0
        ])
    );
}

// LSP 3.18, "Inlay Hint Request": "The visible document range for which inlay hints should be
// computed", answered with `InlayHint[]` of `InlayHintKind` 2, a parameter.
#[test]
fn inlay_hints() {
    let s = started(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[request(
            1,
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri("code.qc") },
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 9, "character": 0 } },
            }),
        )],
    );
    assert_eq!(
        s.result(1),
        &json!([
            { "position": position(CODE, "self,", 0), "label": "e:", "kind": 2, "paddingRight": true },
            { "position": position(CODE, "'0 0 0'", 0), "label": "o:", "kind": 2, "paddingRight": true },
        ])
    );
}

// LSP 3.18, "Text Documents": a position whose character is "greater than the line length it
// defaults back to the line length", and a URI that is not a file names nothing.
#[test]
fn requests_outside_any_file() {
    let s = started(
        project(&[("a.qc", "float a;\n")]),
        &[
            request(
                1,
                "textDocument/definition",
                json!({ "textDocument": { "uri": "untitled:1" }, "position": { "line": 0, "character": 0 } }),
            ),
            request(
                2,
                "textDocument/references",
                json!({ "textDocument": { "uri": "untitled:1" }, "position": { "line": 0, "character": 0 },
                        "context": { "includeDeclaration": true } }),
            ),
            request(
                3,
                "textDocument/hover",
                json!({ "textDocument": { "uri": uri("a.qc") }, "position": { "line": 0, "character": 99 } }),
            ),
            request(
                4,
                "textDocument/semanticTokens/full",
                json!({ "textDocument": { "uri": "untitled:1" } }),
            ),
            request(
                5,
                "textDocument/completion",
                json!({ "textDocument": { "uri": "untitled:1" }, "position": { "line": 0, "character": 0 } }),
            ),
            request(
                6,
                "textDocument/inlayHint",
                json!({ "textDocument": { "uri": "untitled:1" },
                        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } } }),
            ),
            request(
                7,
                "textDocument/rename",
                json!({ "textDocument": { "uri": "untitled:1" }, "position": { "line": 0, "character": 0 }, "newName": "x" }),
            ),
            request(
                8,
                "textDocument/prepareRename",
                json!({ "textDocument": { "uri": "untitled:1" }, "position": { "line": 0, "character": 0 } }),
            ),
        ],
    );
    assert_eq!(s.result(1), &json!(null));
    assert_eq!(s.result(2), &json!([]));
    assert_eq!(s.result(3), &json!(null));
    assert_eq!(s.result(4), &json!({ "data": [] }));
    assert_eq!(s.result(5), &json!([]));
    assert_eq!(s.result(6), &json!([]));
    assert_eq!(s.error_code(7), -32803);
    assert_eq!(s.result(8), &json!(null));
}
