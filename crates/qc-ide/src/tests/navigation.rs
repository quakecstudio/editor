// SPDX-License-Identifier: GPL-3.0-or-later

//! Go-to-definition and find-references.

use qc_diag::Span;

use super::{ProjectSpec, host, nth, one, path};
use crate::FileRange;

/// The range of the `n`-th `needle` of file `name` of text `text`.
fn at(name: &str, text: &str, needle: &str, n: usize) -> FileRange {
    let start = nth(text, needle, n);
    FileRange {
        path: path(name),
        span: Span::new(start, start + needle.len()),
    }
}

// spec QC-12.5: a use goes to the declaration of the definition it resolves to, in the file
// that declares it
#[test]
fn definition_of_a_use_in_another_file() {
    let defs = "float health;\n";
    let code = "void() f = { health = 1; };\n";
    let mut host = host(&ProjectSpec {
        files: &[("defs.qc", defs), ("code.qc", code)],
        ..ProjectSpec::default()
    });
    let found = host.definition(&path("code.qc"), nth(code, "health", 0));
    assert_eq!(found, Some(at("defs.qc", defs, "health", 0)));
}

// spec QC-11.5: the name of a function's body goes to its prototype
#[test]
fn definition_of_a_body_is_its_prototype() {
    let text = "void() f;\nvoid() f = { };\n";
    let mut host = one(text);
    let found = host.definition(&path("a.qc"), nth(text, "f", 1));
    assert_eq!(found, Some(at("a.qc", text, "f", 0)));
}

// spec QC-12.5: a cursor just after a name, as an editor sends it, still names it; white space
// with no name before it names nothing
#[test]
fn definition_at_the_end_of_a_name_and_in_white_space() {
    let text = "float health;\nvoid() f = { health  = 1; };\n";
    let mut host = one(text);
    let end = nth(text, "health", 1) + "health".len();
    assert_eq!(
        host.definition(&path("a.qc"), end),
        Some(at("a.qc", text, "health", 0))
    );
    assert_eq!(host.definition(&path("a.qc"), end + 1), None);
    assert_eq!(host.definition(&path("other.qc"), 0), None);
}

// spec QC-12.4: references of a local are those of its own function only, with its
// declaration when asked
#[test]
fn references_of_a_local() {
    let text = "void() f = { local float t; t = 1; };\nvoid() g = { local float t; t = 2; };\n";
    let mut host = one(text);
    let local = nth(text, "t;", 0);
    let without = host.references(&path("a.qc"), local, false);
    let use_of_t = nth(text, "t =", 0);
    assert_eq!(
        without,
        [FileRange {
            path: path("a.qc"),
            span: Span::new(use_of_t, use_of_t + 1),
        }]
    );
    let with = host.references(&path("a.qc"), local, true);
    assert_eq!(with.len(), 2);
    assert_eq!(with[0].span.start, local);
}

// spec QC-11.5: find-references lists every declaration of a function with its uses
#[test]
fn references_include_every_declaration() {
    let text = "void() f;\nvoid() g = { f(); };\nvoid() f = { };\n";
    let mut host = one(text);
    let starts: Vec<usize> = host
        .references(&path("a.qc"), nth(text, "f", 0), true)
        .iter()
        .map(|r| r.span.start)
        .collect();
    assert_eq!(
        starts,
        [nth(text, "f", 0), nth(text, "f", 1), nth(text, "f", 2)]
    );
    assert!(host.references(&path("a.qc"), text.len(), true).is_empty());
}

// spec QC-23.3: a definition in a file two modules compile is one definition, so its
// references are those of both modules
#[test]
fn references_across_modules() {
    let defs = "float shared;\n";
    let server = "void() s = { shared = 1; };\n";
    let client = "void() c = { shared = 2; };\n";
    let mut host = host(&ProjectSpec {
        files: &[("defs.qc", defs), ("server.qc", server)],
        extra: &[
            ("client.qc", client),
            ("csprogs.src", "../csprogs.dat\ndefs.qc\nclient.qc\n"),
        ],
        ..ProjectSpec::default()
    });
    let found = host.references(&path("server.qc"), nth(server, "shared", 0), true);
    assert_eq!(
        found,
        [
            at("client.qc", client, "shared", 0),
            at("defs.qc", defs, "shared", 0),
            at("server.qc", server, "shared", 0),
        ]
    );
}
