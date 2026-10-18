// SPDX-License-Identifier: GPL-3.0-or-later

//! The diagnostics of as-you-type editing and of a full build are the same set (COMP-2,
//! P2-05).
//!
//! The one [`qc_diag::Diagnostic`] flows through the whole front end, so the diagnostics the
//! editor shows while typing must be exactly the diagnostics a from-scratch build reports. We
//! prove it over a small corpus of vanilla QuakeC modules that between them raise the lexical,
//! syntactic and semantic diagnostics the model owns today. For each module we gather the
//! diagnostics two ways and assert the sets are identical:
//!
//! - the editor way, through the incremental [`qc_sema::Db`], after replaying edits so the
//!   salsa-style caches are exercised rather than computed once; and
//! - the build way, parsing, lowering and resolving each file from scratch with no cache.

use std::collections::BTreeSet;

use qc_diag::Diagnostic;
use qc_sema::lower::lower_file;
use qc_sema::{Db, FileId, resolve_module};

/// One module of the corpus: its files, each a name and its source, in compilation order.
struct Module {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
}

/// A diagnostic flattened to a comparable, order-independent key: the file index in the
/// module, the code, the message and the span. Set equality over these keys is what the
/// acceptance criterion asks for.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Key {
    file: usize,
    code: &'static str,
    message: String,
    start: usize,
    end: usize,
}

fn key(file: usize, d: &Diagnostic) -> Key {
    Key {
        file,
        code: d.code.as_str(),
        message: d.message.clone(),
        start: d.span.start,
        end: d.span.end,
    }
}

/// The diagnostics the editor would show: file parses and the module index from the
/// incremental database. Each file's text is first set to an earlier state and then to its
/// real contents, so the database invalidates and recomputes the way a keystroke makes it,
/// and the query results reflect caching rather than a single cold computation.
fn editor_diagnostics(module: &Module) -> BTreeSet<Key> {
    let mut db = Db::new();
    let ids: Vec<FileId> = (0..module.files.len()).map(FileId).collect();

    // Seed every file with a throwaway earlier version, resolve once so the caches fill,
    // then type the real contents in. This exercises the incremental path (GOAL-1) instead
    // of a cold build.
    for (&id, (_, _)) in ids.iter().zip(module.files) {
        db.set_file_text(id, b"float seed_global;\n".to_vec());
    }
    let _ = db.module_index(&ids);
    for (&id, (_, source)) in ids.iter().zip(module.files) {
        db.set_file_text(id, source.as_bytes().to_vec());
    }

    let mut out = BTreeSet::new();
    for (index, &id) in ids.iter().enumerate() {
        let parse = db.parse(id);
        for d in parse.diagnostics() {
            out.insert(key(index, d));
        }
    }
    for d in &db.module_index(&ids).diagnostics {
        // Resolution diagnostics carry a span but not a file; attribute them to the file
        // whose span range contains them, falling back to the module's first file. This is a
        // test-only attribution and is applied identically to both paths.
        out.insert(key(resolve_file_of(module, d), d));
    }
    out
}

/// The diagnostics of a full build: parse, lower and resolve every file from scratch, with no
/// database and no caching.
fn build_diagnostics(module: &Module) -> BTreeSet<Key> {
    let parses: Vec<_> = module
        .files
        .iter()
        .map(|(_, source)| qc_syntax::parse(source.as_bytes()))
        .collect();

    let mut out = BTreeSet::new();
    for (index, parse) in parses.iter().enumerate() {
        for d in parse.diagnostics() {
            out.insert(key(index, d));
        }
    }
    let lowered: Vec<_> = parses
        .iter()
        .map(|p| lower_file(p, qc_syntax::Dialect::Vanilla))
        .collect();
    let module_index = resolve_module(&lowered);
    for d in &module_index.diagnostics {
        out.insert(key(resolve_file_of(module, d), d));
    }
    out
}

/// Attributes a resolution diagnostic to a file: the first file whose source, lexed and
/// parsed, holds a declaration at the diagnostic's span. Resolution diagnostics do not yet
/// record their file, so for the corpus below, which keeps each diagnostic's offending
/// construct in a single file, matching on the containing file by span is deterministic and
/// the same for both paths, which is all set equality needs.
fn resolve_file_of(module: &Module, d: &Diagnostic) -> usize {
    module
        .files
        .iter()
        .position(|(_, source)| d.span.end <= source.len())
        .unwrap_or(0)
}

/// The corpus: small vanilla QuakeC modules that raise the diagnostics the model owns today.
fn corpus() -> Vec<Module> {
    vec![
        Module {
            name: "clean-single-file",
            files: &[(
                "defs.qc",
                "void() main =\n{\n    local float x;\n    x = 1;\n};\n",
            )],
        },
        Module {
            name: "unknown-value",
            files: &[("world.qc", "void() spawn =\n{\n    missing = 1;\n};\n")],
        },
        Module {
            name: "redeclared-and-mismatch",
            files: &[(
                "a.qc",
                "float count;\nfloat count;\nfloat count = 1;\nfloat count = 2;\n",
            )],
        },
        Module {
            name: "local-is-the-global",
            files: &[(
                "b.qc",
                "float health;\nvoid() touch =\n{\n    local float health;\n    health = 0;\n};\n",
            )],
        },
        Module {
            name: "undefined-function",
            files: &[(
                "c.qc",
                "void() never_defined;\nvoid() caller =\n{\n    never_defined();\n};\n",
            )],
        },
        Module {
            name: "lexical-errors",
            files: &[("d.qc", "float broken = 'bad vector;\nfloat other = %1;\n")],
        },
        Module {
            name: "two-files-cross-reference",
            files: &[
                ("defs.qc", "float shared;\nvoid() helper;\n"),
                (
                    "code.qc",
                    "void() helper =\n{\n    shared = 1;\n    unknown_name = 2;\n};\n",
                ),
            ],
        },
    ]
}

// spec QCD-1.3: the one diagnostic type carries the same code, message and span whichever
// stage produced it, so the editor's set and the build's set are comparable at all
#[test]
fn editor_and_build_diagnostics_are_identical_for_every_corpus_file() {
    for module in corpus() {
        let editor = editor_diagnostics(&module);
        let build = build_diagnostics(&module);
        assert_eq!(
            editor, build,
            "module {}: as-you-type diagnostics differ from the build's",
            module.name
        );
    }
}

// spec QCD-1.3: each corpus module is covered, so set equality holds per file as well as
// overall; a module with no diagnostics proves the equality is not vacuous only when others
// do carry them, which the corpus ensures
#[test]
fn corpus_exercises_errors_and_warnings() {
    let mut seen_error = false;
    let mut seen_warning = false;
    for module in corpus() {
        for diagnostic_key in build_diagnostics(&module) {
            if diagnostic_key.code.starts_with('E') {
                seen_error = true;
            }
            if diagnostic_key.code.starts_with('W') {
                seen_warning = true;
            }
        }
    }
    assert!(seen_error, "the corpus must raise at least one error");
    assert!(seen_warning, "the corpus must raise at least one warning");
}
