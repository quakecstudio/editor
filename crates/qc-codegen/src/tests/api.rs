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
    assert_eq!(d[0].file, FileId::new(0));
}

#[test]
fn the_vanilla_and_fteqcc_dialects_are_compiled_and_gmqcc_is_not() {
    let compiled = |dialect: Dialect| {
        let mut db = Db::new();
        db.set_dialect(dialect);
        db.set_file(
            FileId::new(0),
            PathBuf::from("t.qc"),
            b"void() f = {};\n".to_vec(),
        );
        let input = ModuleInput {
            compiled: vec![FileId::new(0)],
            ..ModuleInput::default()
        };
        compile_module(&mut db, &input, |_| "t.qc".to_owned())
    };
    assert!(compiled(Dialect::Vanilla).is_ok());
    assert!(compiled(Dialect::Fteqcc).is_ok());
    assert_eq!(
        compiled(Dialect::Gmqcc),
        Err(CompileError::Dialect(Dialect::Gmqcc))
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

    // Bodies under the FTEQCC profile built from the rerelease subset's float operators,
    // compound assignments, increments, conditionals, in-bounds array subscripts, fields and
    // control flow: the grammar writes only programs FTEQCC accepts, and every one compiles.
    #[test]
    fn any_fteqcc_body_of_the_subset_is_lowered(body in fte_statements(3)) {
        let source = format!(
            "float a, b;\nentity e;\n.float fl;\nfloat arr[4];\nconst float K = 2;\n\
             float(float x) h = #1;\nvoid(string m) error = #10;\n\
             void() f = {{\nlocal float i;\n{body}\n}};\n"
        );
        let compiled = super::compile_fteqcc(&source);
        prop_assert!(compiled.is_ok(), "{source:?}: {compiled:?}");
    }
}

/// A float expression of the FTEQCC subset, `depth` operators deep at most.
fn fte_expression(depth: u32) -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        Just("a".to_owned()),
        Just("b".to_owned()),
        Just("i".to_owned()),
        Just("K".to_owned()),
        Just("e.fl".to_owned()),
        Just("arr[0]".to_owned()),
        Just("arr[3]".to_owned()),
        (0_u8..10).prop_map(|n| n.to_string()),
    ];
    leaf.prop_recursive(depth, 24, 3, |inner| {
        let binary = [
            "+", "-", "*", "/", "%", "&", "|", "^", "&&", "||", "==", "!=", "<", ">=",
        ];
        prop_oneof![
            (inner.clone(), 0..binary.len(), inner.clone())
                .prop_map(move |(l, op, r)| format!("({l} {} {r})", binary[op])),
            inner.clone().prop_map(|e| format!("!{e}")),
            inner.clone().prop_map(|e| format!("~{e}")),
            inner.clone().prop_map(|e| format!("(- {e})")),
            inner.clone().prop_map(|e| format!("h({e})")),
            inner.clone().prop_map(|e| format!("arr[({e}) & 3]")),
            (inner.clone(), inner.clone(), inner)
                .prop_map(|(c, t, f)| format!("({c} ? {t} : {f})")),
        ]
    })
}

/// The statements of a body of the FTEQCC subset, nested `depth` deep at most.
fn fte_statements(depth: u32) -> impl Strategy<Value = String> {
    // A compound assignment to an element its setter stores is not lowered (README), so
    // `arr[i]` is only stored to.
    let target = prop_oneof![
        Just("a"),
        Just("b"),
        Just("i"),
        Just("e.fl"),
        Just("arr[1]"),
    ];
    let assign = ["+=", "-=", "*=", "/=", "|=", "&="];
    let simple = prop_oneof![
        (target.clone(), fte_expression(3)).prop_map(|(t, e)| format!("{t} = {e};")),
        fte_expression(3).prop_map(|e| format!("arr[i] = {e};")),
        (target.clone(), 0..assign.len(), fte_expression(3))
            .prop_map(move |(t, op, e)| format!("{t} {} {e};", assign[op])),
        target.clone().prop_map(|t| format!("{t}++;")),
        target.prop_map(|t| format!("--{t};")),
        fte_expression(2).prop_map(|e| format!("h({e});")),
        Just("return;".to_owned()),
    ];
    let block = simple.prop_recursive(depth, 32, 4, |inner| {
        let body = proptest::collection::vec(inner, 0..4).prop_map(|s| s.join(" "));
        prop_oneof![
            (fte_expression(2), body.clone(), body.clone())
                .prop_map(|(c, t, f)| format!("if ({c}) {{ {t} }} else {{ {f} }}")),
            (fte_expression(2), body.clone())
                .prop_map(|(c, b)| format!("while ({c}) {{ {b} if (a) break; }}")),
            (fte_expression(2), body.clone())
                .prop_map(|(c, b)| format!("do {{ {b} if (b) continue; }} while ({c});")),
            (fte_expression(2), body.clone())
                .prop_map(|(c, b)| format!("for (i = 0; i < {c}; i++) {{ {b} }}")),
            // FTEQCC types `!x` as bool, which a float case does not match.
            (prop_oneof![Just("a"), Just("i"), Just("e.fl")], body.clone(), body).prop_map(
                |(c, x, y)| format!("switch ({c}) {{ case 1: {x} break; default: {y} }}")
            ),
        ]
    });
    proptest::collection::vec(block, 1..5).prop_map(|s| s.join("\n"))
}
