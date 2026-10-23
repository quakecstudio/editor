// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's warning settings, command line and compiler flags as the semantic model reads them
//! (`docs/spec/dialect-fteqcc.md` FTEF-19.22 to FTEF-19.27).

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::lower::LiteralValue;
use crate::symbol::{DefId, FileId};

/// The diagnostics of the FTEQCC file `source` read with the command line `args`.
fn analysed(source: &str, args: &[&str]) -> Vec<(Code, String)> {
    let (_, diagnostics, _) = analyse(source, args);
    diagnostics
}

/// The database, the diagnostics and the module index of the FTEQCC file `source` read with the
/// command line `args`.
fn analyse(source: &str, args: &[&str]) -> (Db, Vec<(Code, String)>, crate::ModuleIndex) {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FileId(0)],
        command_line: CommandLine::new(args),
        ..ModuleInput::default()
    });
    let diagnostics = analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect();
    (db, diagnostics, analysis.index)
}

fn codes(source: &str, args: &[&str]) -> Vec<Code> {
    analysed(source, args).into_iter().map(|(c, _)| c).collect()
}

const SHADOW: &str = "float x = 1;\nvoid(float x) g = {};\n";

// spec FTEF-19.22: a definition's warning is reported as the settings stand: ignored, a warning
// or an error, E0125, with its text
#[test]
fn definition_warnings_follow_the_settings() {
    assert_eq!(codes(SHADOW, &[]), [Code::W0208]);
    assert_eq!(codes(SHADOW, &["-Wno-F306"]), []);
    assert_eq!(
        analysed(SHADOW, &["-Werror-F306"]),
        [(
            Code::E0125,
            "Local \"x\" hides global with same name and type".to_owned()
        )]
    );
    assert_eq!(codes(SHADOW, &["-Werror"]), [Code::E0125]);
    assert_eq!(codes(SHADOW, &["-Wno-F306", "-Werror"]), []);
    assert_eq!(codes(SHADOW, &["-Werror", "-Wno-F306"]), []);
    assert_eq!(codes(SHADOW, &["-Wnone"]), []);
    assert_eq!(codes(SHADOW, &["-Wno-mundane"]), []);
    // `#pragma warning` changes the settings from where it stands.
    let pragma = "#pragma warning disable F306\nfloat x = 1;\nvoid(float x) g = {};\n\
                  #pragma warning error F306\nvoid(float x) h = {};\n";
    assert_eq!(codes(pragma, &[]), [Code::E0125]);
}

// spec FTEF-19.22: the parser's warnings follow the settings too, and the nameless warnings only
// the options that reach every warning
#[test]
fn parser_and_nameless_warnings_follow_the_settings() {
    let unsigned_float = "unsigned float x;\n";
    assert_eq!(codes(unsigned_float, &[]), [Code::W0226]);
    assert_eq!(codes(unsigned_float, &["-Wno-unimplemented"]), []);
    assert_eq!(codes(unsigned_float, &["-Werror"]), [Code::E0125]);
    let semicolon = "float x\n";
    assert_eq!(codes(semicolon, &[]), [Code::W0203]);
    assert_eq!(codes(semicolon, &["-Werror"]), [Code::E0125]);
    assert_eq!(codes(semicolon, &["-Wnone"]), []);
    // W0311 is this project's own, and no setting changes it.
    assert_eq!(qc_preproc::FteWarning::of_code(Code::W0311), None);
    assert_eq!(qc_preproc::FteWarning::of_code(Code::W0201), None);
}

// spec FTEF-19.22: each warning code is the warning FTEQCC gives its text with
#[test]
fn warning_codes_are_fteqcc_warnings() {
    use qc_preproc::FteWarning as W;
    for (code, warning) in [
        (Code::W0203, W::UndesirableConvention),
        (Code::W0204, W::PointlessStatement),
        (Code::W0313, W::PointlessStatement),
        (Code::W0205, W::UnexpectedPunct),
        (Code::W0215, W::UnexpectedPunct),
        (Code::W0206, W::CompatibilityHack),
        (Code::W0210, W::CompatibilityHack),
        (Code::W0211, W::CompatibilityHack),
        (Code::W0223, W::CompatibilityHack),
        (Code::W0207, W::DuplicateDefinition),
        (Code::W0208, W::SameNameAsGlobal),
        (Code::W0209, W::TypeMismatchRedecOptional),
        (Code::W0212, W::RedeclarationMismatch),
        (Code::W0213, W::MisusedAutocvar),
        (Code::W0214, W::IgnoredKeyword),
        (Code::W0226, W::IgnoredKeyword),
        (Code::W0216, W::MemberNotDefined),
        (Code::W0217, W::NotStandardBehaviour),
        (Code::W0218, W::GmqccSpecific),
        (Code::W0219, W::NotConstant),
        (Code::W0220, W::LaxCast),
        (Code::W0221, W::KeywordDisabled),
        (Code::W0222, W::ParamWithNoName),
        (Code::W0224, W::DeprecatedSyntax),
        (Code::W0225, W::EnumflagsNotInteger),
        (Code::W0312, W::DuplicateLabel),
        (Code::W0314, W::DeprecatedVariable),
    ] {
        assert_eq!(W::of_code(code), Some(warning), "{code:?}");
    }
}

// spec FTEF-19.23: an engine name's strings are read with the flags in force there
#[test]
fn engine_names_are_read_with_the_flags() {
    let name = |source: &str| {
        let (db, _, _) = analyse(source, &[]);
        let decls = db.expanded_decls(FileId(0)).unwrap();
        let decl = decls.decls.iter().find(|d| d.name == "f").unwrap();
        match &decl.literal {
            Some(LiteralValue::Builtin { name, .. }) => name.clone(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(
        name("#pragma flag disable qcfuncs\nvoid() f = #\"a\\101\";\n").as_deref(),
        Some("aA")
    );
    assert_ne!(name("void() f = #\"a\\101\";\n").as_deref(), Some("aA"));
}

// spec FTEF-19.24: `assumeint` and `assumedouble` type the numbers after the pragma
#[test]
fn assumed_types_from_the_pragma() {
    assert_eq!(
        analysed("#pragma flag enable assumeint\nstring s = 5;\n", &[]),
        [(
            Code::W0220,
            "Implicit cast from __int32 to string".to_owned()
        )]
    );
    assert_eq!(codes("string s = 5;\n", &[]), [Code::E0213]);
    assert_eq!(
        codes("#pragma flag enable assumeint\nfloat x = 5;\nint x;\n", &[]),
        [Code::E0205]
    );
    assert_eq!(
        codes("#pragma flag enable assumeint\nfloat x = 'a';\n", &[]),
        []
    );
    assert_eq!(
        analysed("#pragma flag enable assumedouble\nstring s = 1.5;\n", &[]),
        [(
            Code::E0213,
            "Cannot cast from __double to string".to_owned()
        )]
    );
    assert_eq!(codes("string s = 5;\n", &["-Fassumeint"]), [Code::W0220]);
}

// spec FTEF-19.31: under `-Fcaseinsens` a use finds a definition of the name in any case, and
// a definition is never one of another case
#[test]
fn case_insensitive_names() {
    let source = "float Foo;\nvoid() g = { foo = 1; };\n";
    assert_eq!(codes(source, &[]), [Code::E0301]);
    let (_, diagnostics, index) = analyse(source, &["-Fcaseinsens"]);
    assert_eq!(diagnostics, []);
    let foo = index
        .defs
        .iter()
        .position(|d| d.name == "Foo")
        .map(DefId)
        .unwrap();
    assert_eq!(index.refs.iter().filter(|r| r.def == foo).count(), 1);
    assert_eq!(
        codes(source, &["-Fcaseinsens", "-Fno-caseinsens"]),
        [Code::E0301]
    );
    // The name as written wins.
    let (_, _, index) = analyse(
        "float Foo;\nfloat FOO;\nvoid() g = { FOO = 1; };\n",
        &["-Fcaseinsens"],
    );
    let exact = index
        .defs
        .iter()
        .position(|d| d.name == "FOO")
        .map(DefId)
        .unwrap();
    assert!(index.refs.iter().any(|r| r.def == exact));
    // A local before a global.
    let (_, _, index) = analyse(
        "float FOO;\nvoid() g = { float Foo; foo = 1; };\n",
        &["-Fcaseinsens"],
    );
    let local = index
        .defs
        .iter()
        .position(|d| d.name == "Foo")
        .map(DefId)
        .unwrap();
    assert!(index.refs.iter().any(|r| r.def == local));
    assert_eq!(codes("float Foo;\nstring foo;\n", &["-Fcaseinsens"]), []);
    // Of several in other cases, the one defined last.
    assert_eq!(
        codes(
            "float Foo = 1;\nstring FOO = \"a\";\nfloat x = foo;\n",
            &["-Fcaseinsens"]
        ),
        [Code::E0213]
    );
    assert_eq!(
        codes(
            "string FOO = \"a\";\nfloat Foo = 1;\nfloat x = foo;\n",
            &["-Fcaseinsens"]
        ),
        []
    );
    assert_eq!(codes("typedef float Foo;\nFOO x;\n", &["-Fcaseinsens"]), []);
    // A `for` statement's local is gone after it in any case too (FTEF-19.19).
    let after_loop = "void() g = { for (float I = 0; I < 3; I = I + 1) {} i = 1; };\n";
    assert_eq!(codes(after_loop, &["-Fcaseinsens"]), [Code::E0301]);
    // A global's initialiser and a member access outside a function find names in any case.
    assert_eq!(
        codes(
            "entity E;\n.float fld;\nvoid() g = { e.fld = 1; };\n",
            &["-Fcaseinsens"]
        ),
        []
    );
    assert_eq!(codes("typedef float Foo;\nFOO x;\n", &[]), [Code::E0203]);
}

// spec FTEF-19.27: the module's source in source form is parsed past a `#!` line
#[test]
fn a_module_source_is_parsed_past_its_hash_bang() {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(
        FileId(0),
        PathBuf::from("progs.src"),
        b"#!/usr/bin/fteqcc\nfloat x;\n".to_vec(),
    );
    db.set_file(FileId(1), PathBuf::from("t.qc"), b"float y;\n".to_vec());
    let before = db.parse(FileId(0));
    db.analyze_module(&ModuleInput {
        prelude: vec![FileId(0)],
        compiled: vec![FileId(1)],
        ..ModuleInput::default()
    });
    let after = db.parse(FileId(0));
    assert!(after.diagnostics().is_empty(), "{:?}", after.diagnostics());
    assert_ne!(before.diagnostics().len(), 0);
    assert_eq!(db.parse(FileId(1)).diagnostics().len(), 0);
    // A file the prelude no longer names is a plain file again.
    db.analyze_module(&ModuleInput {
        compiled: vec![FileId(1)],
        ..ModuleInput::default()
    });
    assert_ne!(db.parse(FileId(0)).diagnostics().len(), 0);
}

// spec FTEF-19.28: with `qcfuncs` off on the command line, `string`, `vector` and `entity` are no
// type names, and their `__` names are
#[test]
fn core_type_names_without_qcfuncs() {
    assert_eq!(
        analysed("vector v;\n", &["-Fno-qcfuncs"]),
        [(Code::E0203, "\"vector\" is not a type".to_owned())]
    );
    assert_eq!(
        codes("string s;\nentity e;\n", &["-Kno-qcfuncs"]),
        [Code::E0203, Code::E0203]
    );
    assert_eq!(
        codes(
            "__string s;\n__vector v;\n__entity e;\nfloat f;\n",
            &["-Fno-qcfuncs"]
        ),
        []
    );
    assert_eq!(codes("vector v;\n", &[]), []);
    assert_eq!(codes("#pragma flag disable qcfuncs\nvector v;\n", &[]), []);
}
