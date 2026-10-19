// SPDX-License-Identifier: GPL-3.0-or-later

//! Input nested far past the limit goes through the whole front end under the FTEQCC
//! profile (P2-50): the parser bounds its tree (QC-27.15, QC-27.16, FTEF-18.26), so lowering
//! and resolving the tree cannot exhaust the stack either.

use qc_sema::{Db, FileId};
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
        ("spaced prefix -", in_body(&format!("a = {}a;", "- ".repeat(n)))),
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

// spec FTEF-18.26: each form, 100000 deep, parses with "nesting too deep" and resolves through
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
