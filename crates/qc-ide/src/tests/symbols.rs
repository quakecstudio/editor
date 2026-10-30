// SPDX-License-Identifier: GPL-3.0-or-later

//! The outline of a file and of the project.

use super::{ProjectSpec, host, nth, path};
use crate::{Symbol, SymbolKind};

/// The names and kinds of `symbols`, in order.
fn named(symbols: &[Symbol]) -> Vec<(&str, SymbolKind)> {
    symbols.iter().map(|s| (s.name.as_str(), s.kind)).collect()
}

// spec QC-11.6: a file's outline is its global definitions in source order: a function, a
// field, a global with an initialiser as a constant and one without as a variable; parameters
// and locals are not in it
#[test]
fn a_files_outline_is_its_global_definitions() {
    let defs = ".float health;\nfloat speed = 300;\nentity target;\n";
    let code = "void(float d) hurt = { local float x; x = d; };\nvoid() f;\nvoid() f = {};\n";
    let mut host = host(&ProjectSpec {
        files: &[("defs.qc", defs), ("code.qc", code)],
        ..ProjectSpec::default()
    });
    let outline = host.document_symbols(&path("defs.qc"));
    assert_eq!(
        named(&outline),
        [
            ("health", SymbolKind::Field),
            ("speed", SymbolKind::Constant),
            ("target", SymbolKind::Global),
        ]
    );
    let first = outline.first().unwrap();
    assert_eq!(first.range.path, path("defs.qc"));
    assert_eq!(first.range.span.start, nth(defs, "health", 0));
    let code_outline = host.document_symbols(&path("code.qc"));
    assert_eq!(
        named(&code_outline),
        [("hurt", SymbolKind::Function), ("f", SymbolKind::Function)],
        "a function declared twice is listed once, at its first declaration"
    );
    assert!(host.document_symbols(&path("nowhere.qc")).is_empty());
}

// spec QC-12.2: the project's symbols are every file's, by path and then source order, each
// once though two modules compile the file
#[test]
fn the_projects_symbols_are_every_files() {
    let shared = "float shared;\n";
    let mut host = host(&ProjectSpec {
        files: &[("defs.qc", shared), ("a.qc", "float a;\n")],
        extra: &[
            (
                "quakec.toml",
                "[project]\nname = \"p\"\n\n[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n\n[[module]]\nkind = \"csqc\"\nsrc = \"csprogs.src\"\n",
            ),
            ("csprogs.src", "../csprogs.dat\ndefs.qc\n"),
        ],
        ..ProjectSpec::default()
    });
    let all = host.workspace_symbols();
    assert_eq!(
        named(&all),
        [("a", SymbolKind::Global), ("shared", SymbolKind::Global)]
    );
    assert_eq!(all.first().unwrap().range.path, path("a.qc"));
}
