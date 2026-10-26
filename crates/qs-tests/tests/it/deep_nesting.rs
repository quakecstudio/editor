// SPDX-License-Identifier: GPL-3.0-or-later

//! Input nested far past the limit goes through the whole front end under the FTEQCC
//! profile (P2-50): the parser bounds its tree (QC-27.15, QC-27.16, FTEF-18.32), so lowering
//! and resolving the tree cannot exhaust the stack either. A long chain of macros goes through
//! the preprocessor in front of them without exhausting it (QC-9.17, P2-65).

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use qc_sema::{Db, FileId, ModuleInput};
use qc_syntax::{Code, Dialect};

/// A file whose function `f` holds `body`.
fn in_body(body: &str) -> String {
    format!("float a;\nvoid() f =\n{{\n{body}\n}};\n")
}

/// The forms the P2-R2 review found overflowing the stack, each nested 100000 deep.
fn deep_sources() -> Vec<(&'static str, String)> {
    let n = 100_000;
    vec![
        ("prefix -", in_body(&format!("a = {}a;", "-".repeat(n)))),
        ("prefix +", in_body(&format!("a = {}a;", "+".repeat(n)))),
        (
            "spaced prefix -",
            in_body(&format!("a = {}a;", "- ".repeat(n))),
        ),
        ("prefix ++", in_body(&format!("{}a;", "++".repeat(n)))),
        ("prefix --", in_body(&format!("{}a;", "--".repeat(n)))),
        ("parameter lists", format!("float{} g;\n", "()".repeat(n))),
        ("pointers", format!("float{} p;\n", "*".repeat(n))),
        (
            "declarator pointers",
            format!("float a, {}b;\n", "*".repeat(n)),
        ),
        (
            "state frame",
            format!("void() f = [{}1, f] {{}};\n", "-".repeat(n)),
        ),
        (
            "parameter default",
            format!("void(float a = {}1) g;\n", "-".repeat(n)),
        ),
    ]
}

// spec FTEF-18.32: each form, 100000 deep, parses with "nesting too deep" and resolves through
// the incremental database under the FTEQCC profile without exhausting the stack
// spec QC-27.16: the tree past the limit is no deeper, so the semantic model walks it safely
#[test]
fn deep_fteqcc_input_resolves() {
    for (name, source) in deep_sources() {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        let file = FileId(0);
        db.set_file_text(file, source.into_bytes());
        let parse = db.parse(file);
        assert!(
            parse.errors().any(|d| d.code == Code::E0509),
            "{name}: no E0509"
        );
        let index = db.module_index(&[file]);
        assert!(
            index.diagnostics.iter().all(|d| d.file == file),
            "{name}: a diagnostic outside the file"
        );
    }
}

// spec QC-9.17: a file of 100000 chained `#define`s and a declaration using the last goes
// through `Db::analyze_module` under each dialect on a 2 MiB thread, with no diagnostic
#[test]
fn a_long_chain_of_macros_resolves() {
    let n: usize = 100_000;
    let mut source = "#define M0 1\n".to_owned();
    for (before, i) in (1..n).enumerate() {
        writeln!(source, "#define M{i} M{before}").unwrap();
    }
    writeln!(source, "float x = M{};", n.saturating_sub(1)).unwrap();
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let source = source.clone();
        let (codes, defs) = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                let mut db = Db::new();
                db.set_dialect(dialect);
                let file = FileId(0);
                db.set_file(file, PathBuf::from("a.qc"), source.into_bytes());
                let input = ModuleInput {
                    compiled: vec![file],
                    ..ModuleInput::default()
                };
                let analysis = db.analyze_module(&input);
                let codes: Vec<Code> = analysis.diagnostics().map(|d| d.diagnostic.code).collect();
                (codes, analysis.index.defs.len())
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!((codes, defs), (vec![], 1), "{dialect:?}");
    }
}
