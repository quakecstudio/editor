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
    let mut function = at("code.qc", CODE, "heal", 0);
    function["newName"] = json!("mend");
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
            request(5, "textDocument/rename", function),
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
    // ADR-0019, LSP 3.18 `TextDocumentEdit`: "If a client doesn't signal the capability,
    // servers shouldn't send `AnnotatedTextEdit` literals back to the client", so a client that
    // cannot confirm the edits of a name a map may use gets the rename refused.
    assert_eq!(s.error_code(5), -32803);
    let refused = s.response(5)["error"]["message"].as_str().unwrap();
    assert!(
        refused.contains("heal") && refused.contains("change annotations"),
        "{refused}"
    );
    assert!(s.notifications("window/showMessage").is_empty());
}

/// The client capabilities of a client that confirms annotated rename edits.
fn confirming() -> Value {
    json!({
        "workspace": { "workspaceEdit": {
            "documentChanges": true,
            "changeAnnotationSupport": { "groupsOnLabel": true },
        } },
        "textDocument": { "rename": { "prepareSupport": true, "honorsChangeAnnotations": true } },
    })
}

// LSP 3.18, `ChangeAnnotation.needsConfirmation`: "A flag which indicates that user confirmation
// is needed before applying the change". ADR-0019: to a client that honors change annotations
// (`workspace.workspaceEdit.changeAnnotationSupport`, `documentChanges` and the rename's
// `honorsChangeAnnotations`), every edit of a function or field a map may name is an
// `AnnotatedTextEdit` naming one annotation that needs confirmation; any other rename stays
// plain `changes`.
#[test]
fn rename_asks_before_editing_a_name_a_map_may_use() {
    let rename = |needle: &str, new: &str| {
        let mut params = at("code.qc", CODE, needle, 0);
        params["newName"] = json!(new);
        params
    };
    let s = session(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[
            initialize_with(json!(null), confirming()),
            notification("initialized", json!({})),
            notification(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri("code.qc"), "languageId": "quakec",
                                          "version": 7, "text": CODE } }),
            ),
            request(1, "textDocument/rename", rename("heal", "mend")),
            request(2, "textDocument/rename", rename("health", "hp")),
        ],
    );
    let edit = s.result(1);
    assert_eq!(edit.get("changes"), None);
    let annotation = &edit["changeAnnotations"]["map-name"];
    assert_eq!(annotation["needsConfirmation"], json!(true));
    assert!(annotation["label"].as_str().unwrap().contains("heal"));
    assert!(
        annotation["description"]
            .as_str()
            .unwrap()
            .contains("classname")
    );
    let changes = edit["documentChanges"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0]["textDocument"],
        json!({ "uri": uri("code.qc"), "version": 7 })
    );
    assert_eq!(
        changes[0]["edits"],
        json!([{ "range": range(CODE, "heal", 0), "newText": "mend", "annotationId": "map-name" }])
    );
    // A global no map names is renamed with plain edits, none annotated.
    let plain = s.result(2);
    assert_eq!(plain.get("changeAnnotations"), None);
    assert_eq!(plain.get("documentChanges"), None);
    assert_eq!(
        plain["changes"][uri("defs.qc")].as_array().unwrap().len(),
        1
    );
}

// LSP 3.18: a client with change annotations but no `honorsChangeAnnotations` for rename, or
// with no `documentChanges` to carry an `AnnotatedTextEdit`, cannot confirm, so the rename of a
// name a map may use is refused (ADR-0019).
#[test]
fn rename_is_refused_to_a_client_that_cannot_confirm() {
    for capabilities in [
        json!({ "workspace": { "workspaceEdit": {
            "documentChanges": true, "changeAnnotationSupport": {} } } }),
        json!({ "workspace": { "workspaceEdit": { "changeAnnotationSupport": {} } },
                "textDocument": { "rename": { "honorsChangeAnnotations": true } } }),
        json!({ "workspace": { "workspaceEdit": { "documentChanges": true } },
                "textDocument": { "rename": { "honorsChangeAnnotations": true } } }),
    ] {
        let mut params = at("code.qc", CODE, "heal", 0);
        params["newName"] = json!("mend");
        let s = session(
            project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
            &[
                initialize_with(json!(null), capabilities.clone()),
                request(1, "textDocument/rename", params),
            ],
        );
        assert_eq!(s.error_code(1), -32803, "{capabilities}");
    }
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
    let whole = json!({ "start": place(0, 0), "end": place(9, 0) });
    let s = started(
        project(&[("defs.qc", DEFS), ("code.qc", CODE)]),
        &[request(
            1,
            "textDocument/inlayHint",
            json!({ "textDocument": { "uri": uri("code.qc") }, "range": whole }),
        )],
    );
    let hint = |at: Value, label: &str| {
        let parameter = 2;
        json!({ "position": at, "label": label, "kind": parameter, "paddingRight": true })
    };
    assert_eq!(
        s.result(1),
        &json!([
            hint(position(CODE, "self,", 0), "e:"),
            hint(position(CODE, "'0 0 0'", 0), "o:"),
        ])
    );
}

/// A position.
fn place(line: u32, character: u32) -> Value {
    json!({ "line": line, "character": character })
}

// LSP 3.18, "Text Documents": a position whose character is "greater than the line length it
// defaults back to the line length", and a URI that is not a file names nothing.
#[test]
fn requests_outside_any_file() {
    let nowhere = json!({ "uri": "untitled:1" });
    let at_start = json!({ "textDocument": nowhere, "position": place(0, 0) });
    let with = |key: &str, value: Value| {
        let mut params = at_start.clone();
        params
            .as_object_mut()
            .unwrap()
            .insert(key.to_owned(), value);
        params
    };
    let past_the_line = json!({ "textDocument": { "uri": uri("a.qc") }, "position": place(0, 99) });
    let empty = json!({ "start": place(0, 0), "end": place(0, 0) });
    let s = started(
        project(&[("a.qc", "float a;\n")]),
        &[
            request(1, "textDocument/definition", at_start.clone()),
            request(
                2,
                "textDocument/references",
                with("context", json!({ "includeDeclaration": true })),
            ),
            request(3, "textDocument/hover", past_the_line),
            request(
                4,
                "textDocument/semanticTokens/full",
                json!({ "textDocument": nowhere }),
            ),
            request(5, "textDocument/completion", at_start.clone()),
            request(
                6,
                "textDocument/inlayHint",
                json!({ "textDocument": nowhere, "range": empty }),
            ),
            request(7, "textDocument/rename", with("newName", json!("x"))),
            request(8, "textDocument/prepareRename", at_start.clone()),
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
