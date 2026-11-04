// SPDX-License-Identifier: GPL-3.0-or-later

//! Answers the other modules did not pin, added by the P2-RA readiness review: each kills a
//! mutant that survived the review's measurement.

use std::io::{BufReader, Read as _};

use qc_project::FsLoader;
use serde_json::{Value, json};

use super::{initialize_with, notification, position, project, request, session, unframed};
use crate::Server;
use crate::transport::{MAX_CONTENT, read_message};
use crate::uri::from_path;

/// Two builtins: one every engine has, one that id's engine lacks.
const BUILTINS: &str = "float(string s) strlen = #114;\n\
                        void(entity e, vector o) setorigin = #2;\nentity world;\n\
                        void() f = { strlen(\"\"); setorigin(world, '0 0 0'); };\n";

/// The hover text of `needle` in [`BUILTINS`], for a client initialized with `options` and
/// `capabilities`.
fn hover(options: Value, capabilities: Value, needle: &str) -> String {
    let s = session(
        project(&[("a.qc", BUILTINS)]),
        &[
            initialize_with(options, capabilities),
            notification("initialized", json!({})),
            request(
                1,
                "textDocument/hover",
                json!({
                    "textDocument": { "uri": super::uri("a.qc") },
                    "position": position(BUILTINS, needle, 0),
                }),
            ),
        ],
    );
    s.result(1)["contents"]["value"]
        .as_str()
        .unwrap()
        .to_owned()
}

// LSP 3.18, `MarkupContent`: a builtin's hover says which engines have it, each part on a line
// of its own: in Markdown a line ends in two spaces, in plain text it is a line break.
#[test]
fn a_builtins_hover_lists_the_engines_with_and_without_it() {
    let markdown = json!({ "textDocument": { "hover": { "contentFormat": ["markdown"] } } });
    assert_eq!(
        hover(json!(null), markdown.clone(), "strlen("),
        "```quakec\nfloat(string s) strlen\n```\n\n---\n\nBuiltin `#114`.  \n\
         Vanilla, the target engine, does not have it.  \n\
         Engines with it: Ironwail, vkQuake, FTEQW, DarkPlaces.  \n\
         Engines without it: Vanilla, QuakeSpasm, KEX."
    );
    assert_eq!(
        hover(json!(null), markdown, "setorigin("),
        "```quakec\nvoid(entity e, vector o) setorigin\n```\n\n---\n\nBuiltin `#2`.  \n\
         Vanilla has it as `void(entity e, vector o) setorigin`.  \n\
         Engines with it: Vanilla, QuakeSpasm, Ironwail, vkQuake, FTEQW, DarkPlaces, KEX."
    );
    assert_eq!(
        hover(json!({ "target": "fte" }), json!({}), "strlen("),
        "float(string s) strlen\n\nBuiltin #114.\n\
         FTEQW has it as float(string s) strlen (extensions FRIK_FILE, FTE_STRINGS, \
         ZQ_QC_STRINGS).\n\
         Engines with it: Ironwail, vkQuake, FTEQW, DarkPlaces.\n\
         Engines without it: Vanilla, QuakeSpasm, KEX."
    );
}

// LSP 3.18, `positionEncoding`: "If the client didn't provide any position encodings the only
// valid value that a server can return is 'utf-16'"; a client offering only UTF-8 gets it, and
// one offering only UTF-16 gets UTF-16.
#[test]
fn the_position_encoding_is_the_one_offered() {
    for (offered, chosen) in [(json!(["utf-8"]), "utf-8"), (json!(["utf-16"]), "utf-16")] {
        let s = session(
            project(&[("a.qc", "")]),
            &[initialize_with(
                json!(null),
                json!({ "general": { "positionEncodings": offered } }),
            )],
        );
        assert_eq!(
            s.result(0)["capabilities"]["positionEncoding"],
            json!(chosen),
            "{offered}"
        );
    }
}

// LSP 3.18, "DidSaveTextDocument Notification": a save is when the server reads the project
// again, so a file the saved `progs.src` now lists is compiled and its definitions resolve.
#[test]
fn a_save_reads_the_project_again() {
    let dir = std::env::temp_dir().join(format!("quakec-lsp-save-{}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\n").unwrap();
    std::fs::write(dir.join("a.qc"), "void() f = { health = 1; };\n").unwrap();
    std::fs::write(dir.join("defs.qc"), "float health;\n").unwrap();
    let mut output = Vec::new();
    let mut server = Server::new(FsLoader, &mut output);
    let mut send = |message: Value| {
        server
            .handle(serde_json::to_vec(&message).unwrap().as_slice())
            .unwrap()
    };
    send(request(
        0,
        "initialize",
        json!({ "processId": null, "rootUri": from_path(&dir), "capabilities": {} }),
    ));
    send(notification("initialized", json!({})));
    std::fs::write(dir.join("progs.src"), "progs.dat\ndefs.qc\na.qc\n").unwrap();
    send(notification(
        "textDocument/didSave",
        json!({ "textDocument": { "uri": from_path(&dir.join("progs.src")) } }),
    ));
    let _ignored = std::fs::remove_dir_all(&dir);
    let a = json!(from_path(&dir.join("a.qc")));
    let published: Vec<Value> = unframed(&output)
        .into_iter()
        .filter(|m| m["method"] == json!("textDocument/publishDiagnostics"))
        .filter(|m| m["params"]["uri"] == a)
        .map(|m| m["params"]["diagnostics"].clone())
        .collect();
    assert_eq!(published.len(), 2, "{published:?}");
    assert_ne!(published[0], json!([]));
    assert_eq!(published[1], json!([]));
}

// LSP 3.18, "Header Part": `Content-Length` is the content's length; one of exactly
// `MAX_CONTENT` bytes, the longest the server reads, is read whole.
#[test]
fn a_message_of_the_longest_length_is_read() {
    let header = format!("Content-Length: {MAX_CONTENT}\r\n\r\n");
    let length = u64::try_from(MAX_CONTENT).unwrap();
    let body = std::io::repeat(b' ').take(length);
    let mut input = BufReader::new(header.as_bytes().chain(body));
    let content = read_message(&mut input).unwrap().unwrap();
    assert_eq!(content.len(), MAX_CONTENT);
    assert!(read_message(&mut input).unwrap().is_none());
}
