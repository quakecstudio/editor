// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolution of FTEQCC's further statements and terms (`docs/spec/dialect-fteqcc.md`, FTEF-18),
//! the builtin redeclaration FTEQCC accepts (FTEF-17.9), and `#pragma autoproto` (FTEF-12.9).

use std::path::PathBuf;

use qc_diag::Code;
use qc_syntax::Dialect;

use super::fte_index_of;
use crate::db::{Db, ModuleAnalysis};
use crate::expand::ModuleInput;
use crate::resolve::ModuleIndex;
use crate::symbol::FileId;

/// The error codes of an index.
fn errors(index: &ModuleIndex) -> Vec<Code> {
    index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| d.diagnostic.code)
        .collect()
}

/// The messages of an index's diagnostics, warnings included.
fn messages(index: &ModuleIndex) -> Vec<String> {
    index
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.diagnostic.code.as_str(), d.diagnostic.message))
        .collect()
}

/// The names the uses of `index` resolve to, as `name -> definition name`.
fn resolved(index: &ModuleIndex) -> Vec<String> {
    index
        .refs
        .iter()
        .filter_map(|r| index.def(r.def).map(|d| d.name.clone()))
        .collect()
}

/// The module of `files` read through the preprocessor under the FTEQCC profile, with the
/// list-form `progs.src` autoprototyping `listed`.
fn analyze(files: &[(&str, &str)], listed: Option<bool>) -> ModuleAnalysis {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    for (index, (name, text)) in files.iter().enumerate() {
        db.set_file(FileId(index), PathBuf::from(name), text.as_bytes().to_vec());
    }
    let input = ModuleInput {
        prelude: Vec::new(),
        compiled: (0..files.len()).map(FileId).collect(),
        autoproto: listed,
    };
    db.analyze_module(&input)
}

/// The error codes of an analysis.
fn analysis_errors(analysis: &ModuleAnalysis) -> Vec<Code> {
    analysis
        .diagnostics()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| d.diagnostic.code)
        .collect()
}

// spec FTEF-18.3: the names in a state's frame and next expressions are uses, and a next think written as a name is declared
// spec FTEF-18.19: the frames of a cycle are uses
#[test]
fn state_expressions_resolve() {
    let index = fte_index_of(
        "float BASE = 3;\n.void() th;\nentity self;\n\
         void() a = [BASE + 1, self.th] {};\nvoid() b = [BASE, later] {};\n\
         void() c = [++ BASE .. BASE + 4] {};\nvoid() d = [MISSING, a] {};\nvoid() later = {};\n",
    );
    assert_eq!(errors(&index), [Code::E0301], "{:?}", messages(&index));
    assert!(messages(&index)[0].contains("MISSING"));
    let names = resolved(&index);
    assert_eq!(
        names.iter().filter(|n| *n == "BASE").count(),
        4,
        "{names:?}"
    );
    assert!(names.contains(&"later".to_owned()));
    assert!(names.contains(&"th".to_owned()));
}

// spec FTEF-18.5: each expression of a sequence is resolved
// spec FTEF-18.20: so are the sequences of returns, conditions and subscripts
#[test]
fn sequences_resolve() {
    let index = fte_index_of(
        "float x, y;\nfloat() f = { x = 1, y = 2; if (x, y) return x, y; return = x; return y; };\n",
    );
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    let index = fte_index_of("float() f = { local float x; x = 1, nowhere = 2; return x; };\n");
    assert_eq!(errors(&index), [Code::E0301]);
}

// spec FTEF-18.6: labels and goto name no values
#[test]
fn labels_resolve_nothing() {
    let index =
        fte_index_of("float x;\nvoid() f = { top: x = 1; :again goto top; goto again; };\n");
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
}

// spec FTEF-18.10: both ends of a case range are uses
#[test]
fn case_ranges_resolve() {
    let index = fte_index_of(
        "float LO = 1, HI = 5, x;\nvoid() f = { switch (x) { case LO .. HI: break; case 6 .. TOP: break; } };\n",
    );
    assert_eq!(errors(&index), [Code::E0301], "{:?}", messages(&index));
    let names = resolved(&index);
    assert!(names.contains(&"LO".to_owned()) && names.contains(&"HI".to_owned()));
}

// spec FTEF-18.12: the expression sizeof measures is resolved; the type it measures names nothing
#[test]
fn sizeof_operands_resolve() {
    let index = fte_index_of(
        "float x;\nfloat() f = { return sizeof(float) + sizeof(int *) + sizeof x + sizeof(x); };\n",
    );
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    assert_eq!(resolved(&index).iter().filter(|n| *n == "x").count(), 2);
    let index = fte_index_of("float() f = { return sizeof(nothing); };\n");
    assert_eq!(errors(&index), [Code::E0301]);
}

// spec FTEF-17.9: a builtin given for a function with a body keeps the body, with a warning
// spec QCD-7.23: W0206 "incompatible redeclaration. Ignoring replacement of qc function with builtin."
#[test]
fn builtin_after_body_is_a_warning() {
    let index =
        fte_index_of("float(float a) sin = { return a; };\nfloat(float angle) sin = #60;\n");
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    assert_eq!(
        messages(&index),
        ["W0206 incompatible redeclaration. Ignoring replacement of qc function with builtin."]
    );
    assert!(!Code::W0206.is_error());
    // A builtin and then a body, or a body twice, is still "NAME redeclared" (QCD-3.6).
    let index = fte_index_of("float(float a) cos = #61;\nfloat(float a) cos = { return a; };\n");
    assert_eq!(errors(&index), [Code::E0206]);
    let index = fte_index_of("float() g = { return 1; };\nfloat() g = { return 2; };\n");
    assert_eq!(errors(&index), [Code::E0206]);
    // A builtin of another type is the type mismatch of QCD-3.5.
    let index = fte_index_of("float(float a) tan = { return a; };\nvoid() tan = #62;\n");
    assert_eq!(errors(&index), [Code::E0205]);
    // Vanilla keeps QCD-3.6.
    let vanilla =
        super::index_of("float(float a) sin = { return a; };\nfloat(float a) sin = #60;\n");
    assert_eq!(errors(&vanilla), [Code::E0206]);
}

// spec FTEF-12.9: `#pragma autoproto` before any definition reads the module with autoprototyping, `#pragma autoproto 0` without
#[test]
fn autoproto_pragma_turns_autoprototyping_on() {
    let later_use = ("b.qc", "void() a = { later(); };\nvoid() later = {};\n");
    // Without autoprototyping `later` is used before it is declared.
    let plain = analyze(&[("a.qc", "float x;\n"), later_use], None);
    assert_eq!(analysis_errors(&plain), [Code::E0301]);
    let pragma = analyze(
        &[("a.qc", "#pragma autoproto\nfloat x;\n"), later_use],
        None,
    );
    assert_eq!(analysis_errors(&pragma), Vec::<Code>::new());
    // After a definition the pragma is ignored.
    let late = analyze(
        &[("a.qc", "float x;\n#pragma autoproto\n"), later_use],
        None,
    );
    assert_eq!(analysis_errors(&late), [Code::E0301]);
    let late_file = analyze(
        &[
            ("a.qc", "float x;\n"),
            ("c.qc", "#pragma autoproto\n"),
            later_use,
        ],
        None,
    );
    assert_eq!(analysis_errors(&late_file), [Code::E0301]);
    // A list-form `progs.src`'s pragma, which the graph reads, holds for the module, and a
    // compiled file's `#pragma autoproto 0` before any definition turns it off again.
    let listed = analyze(&[("a.qc", "float x;\n"), later_use], Some(true));
    assert_eq!(analysis_errors(&listed), Vec::<Code>::new());
    let off = analyze(
        &[("a.qc", "#pragma autoproto 0\nfloat x;\n"), later_use],
        Some(true),
    );
    assert_eq!(analysis_errors(&off), [Code::E0301]);
    let listed_off = analyze(&[("a.qc", "float x;\n"), later_use], Some(false));
    assert_eq!(analysis_errors(&listed_off), [Code::E0301]);
}
