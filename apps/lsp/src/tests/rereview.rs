// SPDX-License-Identifier: GPL-3.0-or-later

//! Cases the P2-RA re-review added against the fixes of P2-161, P2-164 and P2-165.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{initialize_with, notification, project, request, session, uri};
use crate::transport::{MAX_HEADER_LINE, TransportError, read_message};
use crate::uri::{Host, from_path_on, to_path_on};

/// The codes of the diagnostics first published for `a.qc` of a project holding `text`, and
/// `toml` as its `quakec.toml` when given, for a client whose `initializationOptions` are
/// `options`.
fn codes(text: &str, toml: Option<&str>, options: Value) -> Vec<String> {
    let mut loader = project(&[("a.qc", text)]);
    if let Some(toml) = toml {
        loader.insert(Path::new("/p/quakec.toml"), toml.as_bytes().to_vec());
    }
    let s = session(
        loader,
        &[
            initialize_with(options, json!({})),
            notification("initialized", json!({})),
            request(1, "shutdown", json!(null)),
            notification("exit", json!(null)),
        ],
    );
    // A file with no problems is not published until it has had one to clear.
    let published = s.notifications("textDocument/publishDiagnostics");
    let Some(first) = published.iter().find(|p| p["uri"] == json!(uri("a.qc"))) else {
        return Vec::new();
    };
    first["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

/// A `quakec.toml` of `project`'s lines and one server module.
fn toml(project: &str) -> String {
    format!("{project}[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n")
}

// P2-164 and the README: `initializationOptions`' dialect reads a project with no
// `quakec.toml`; a project file's dialect, vanilla when it names none, wins over it.
#[test]
fn the_options_dialect_yields_to_the_project_file() {
    let text = "void() f = { float x = 1; x += 2; };\n";
    let options = json!({ "dialect": "fteqcc" });
    assert!(codes(text, None, options.clone()).is_empty());
    let vanilla = codes(text, Some(&toml("")), options);
    assert!(vanilla.iter().any(|c| c.starts_with('E')), "{vanilla:?}");
    let fte = toml("[project]\ndialect = \"fteqcc\"\n\n");
    assert!(codes(text, Some(&fte), json!({ "dialect": "vanilla" })).is_empty());
}

// P2-164: `initializationOptions`' target is a bare project's; a project file's target, id's
// engine when it names none, wins, so a builtin id's engine lacks is W0311 there.
#[test]
fn the_options_target_yields_to_the_project_file() {
    let text = "void() print_it = #0:ex_bprint;\nvoid() f = { print_it(); };\n";
    let options = json!({ "dialect": "fteqcc", "target": "rerelease" });
    assert!(codes(text, None, options.clone()).is_empty());
    let fte = toml("[project]\ndialect = \"fteqcc\"\n\n");
    assert_eq!(codes(text, Some(&fte), options), ["W0311"]);
}

// P2-165: a header line of exactly `MAX_HEADER_LINE` bytes, its `\r\n` included, is read; one
// byte more is refused.
#[test]
fn a_header_line_of_exactly_the_limit_is_read() {
    let line = |length: usize| {
        let name = "X".repeat(length - ": y\r\n".len());
        format!("{name}: y\r\nContent-Length: 2\r\n\r\n{{}}")
    };
    let fits = line(MAX_HEADER_LINE);
    assert_eq!(
        read_message(&mut fits.as_bytes()).unwrap(),
        Some(b"{}".to_vec())
    );
    let over = line(MAX_HEADER_LINE + 1);
    assert!(matches!(
        read_message(&mut over.as_bytes()),
        Err(TransportError::LongHeader)
    ));
}

// P2-161: a Windows path round-trips through its URI, the drive letter in either case and
// the separators either way.
#[test]
fn windows_drive_paths_round_trip() {
    for path in ["C:/q/a b/x.qc", "c:\\q\\x.qc", "Z:/"] {
        let uri = from_path_on(Path::new(path), Host::Windows);
        assert!(uri.starts_with("file:///"), "{uri}");
        assert_eq!(
            to_path_on(&uri, Host::Windows),
            Some(PathBuf::from(path.replace('\\', "/"))),
            "{uri}"
        );
    }
}
