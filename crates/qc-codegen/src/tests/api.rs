// SPDX-License-Identifier: GPL-3.0-or-later

//! The entry points: dialects, located diagnostics, and any input lowered without an internal
//! error.

use std::path::PathBuf;

use proptest::prelude::*;
use qc_sema::{Db, FileId, ModuleInput};
use qc_syntax::Dialect;

use super::compile_source;
use crate::{CompileError, Unit, compile, compile_module};

// spec QC-9.7: a compiler error is at the span of the source file it came from, after the
// preprocessor's lines and through a macro's use.
#[test]
fn errors_are_located_in_the_source() {
    let source = "#define TEXT \"x\"\nfloat a;\nvoid() f = { a = TEXT; };\n";
    let Err(CompileError::Diagnostics(d)) = compile_source(source) else {
        panic!("compiled");
    };
    assert_eq!(d.len(), 1);
    let span = d[0].diagnostic.span;
    assert_eq!(&source[span.start..span.end], "=");
    assert_eq!(d[0].file, FileId(0));
}

#[test]
fn only_the_vanilla_dialect_is_compiled() {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(
        FileId(0),
        PathBuf::from("t.qc"),
        b"void() f = {};\n".to_vec(),
    );
    let input = ModuleInput {
        compiled: vec![FileId(0)],
        ..ModuleInput::default()
    };
    assert_eq!(
        compile_module(&mut db, &input, |_| "t.qc".to_owned()),
        Err(CompileError::Dialect(Dialect::Fteqcc))
    );
}

#[test]
fn units_compile_without_a_database() {
    let parse = qc_syntax::parse_with(b"float a;\nvoid() f = { a = 1; };\n", Dialect::Vanilla);
    let units = [Unit {
        name: "u.qc",
        parse: &parse,
    }];
    let compiled = compile(&units).unwrap();
    assert_eq!(super::string(&compiled.progs, 1), "u.qc");
    let bad = qc_syntax::parse_with(b"float a;\nvoid() f = { a = ; };\n", Dialect::Vanilla);
    let units = [Unit {
        name: "u.qc",
        parse: &bad,
    }];
    assert!(matches!(compile(&units), Err(CompileError::Diagnostics(_))));
}

/// Words a vanilla program is made of, to build token soup from.
const WORDS: &[&str] = &[
    "float", "vector", "string", "entity", "void", ".", "(", ")", "{", "}", ";", ",", "=", "==",
    "!=", "<", ">", "<=", ">=", "+", "-", "*", "/", "&", "|", "&&", "||", "!", "a", "b", "e", "f",
    "g", "v", "fl", "local", "return", "if", "else", "while", "do", "#1", "1", "'1 2 3'", "\"s\"",
    "[", "]", "$frame", "\n",
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    // Any program the semantic model accepts is lowered: the compiler never meets a construct
    // it cannot lower, and never panics.
    #[test]
    fn any_accepted_program_is_lowered(words in proptest::collection::vec(0..WORDS.len(), 0..60)) {
        let prelude = "float a, b;\nvector v;\nentity e;\n.float fl;\nvoid() g;\n";
        let mut source = String::from(prelude);
        for w in words {
            source.push_str(WORDS[w]);
            source.push(' ');
        }
        source.push_str("\nvoid() g = {};\n");
        match compile_source(&source) {
            Ok(_) | Err(CompileError::Diagnostics(_)) => {}
            Err(other) => prop_assert!(false, "{source:?}: {other:?}"),
        }
    }

    // Statements in a body: every one the parser and the semantic model accept is lowered.
    #[test]
    fn any_accepted_body_is_lowered(words in proptest::collection::vec(0..WORDS.len(), 0..40)) {
        let mut source = String::from(
            "float a, b;\nvector v;\nentity e;\n.float fl;\n.vector fv;\nstring s;\n\
             float(float x) h = #1;\nvoid() f = {\n",
        );
        for w in words {
            source.push_str(WORDS[w]);
            source.push(' ');
        }
        source.push_str("\n};\n");
        match compile_source(&source) {
            Ok(_) | Err(CompileError::Diagnostics(_)) => {}
            Err(other) => prop_assert!(false, "{source:?}: {other:?}"),
        }
    }
}
