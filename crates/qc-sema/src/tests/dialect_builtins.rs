// SPDX-License-Identifier: GPL-3.0-or-later

//! A builtin initialised again, compared as FTEQCC compares it
//! (`docs/spec/dialect-fteqcc-rerelease.md` FTE-23.7, `docs/spec/dialect-fteqcc.md` FTEF-17.10).

use std::path::PathBuf;

use qc_diag::Code;
use qc_syntax::Dialect;

use super::{fte_index_of, index_of};
use crate::db::Db;
use crate::expand::ModuleInput;
use crate::lower::{LiteralValue, lower_file};
use crate::symbol::FileId;

/// The error codes of `source` resolved under the FTEQCC dialect.
fn fte_errors(source: &str) -> Vec<Code> {
    fte_index_of(source)
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.code)
        .filter(|code| code.is_error())
        .collect()
}

// spec FTE-23.7: a second builtin initialiser is the same value only when it has the number
// and engine name of the first, the definition's own name standing in for none, so a
// different engine name is E0206 though the number is the same
#[test]
fn builtins_compare_by_number_and_engine_name() {
    for (first, second, redeclared) in [
        ("#0:ex_a", "#0:ex_b", true),
        ("#5:ex_a", "#5:ex_b", true),
        ("#5", "#5:ex_a", true),
        ("#5:ex_a", "#5", true),
        ("#0", "#0:ex_a", true),
        ("#5:ex_a", "#6:ex_a", true),
        ("#5:ex_a", ":ex_a", true),
        (":ex_a", ":ex_b", true),
        // FTE-23.6: names compare byte for byte and end at a NUL.
        ("#\"\\bab\"", "#\"\\bcd\"", true),
        ("#\"ab\\x00cd\"", ":ab", false),
        ("#5", "#5:f", false),
        ("#5:f", "#5", false),
        ("#0", ":f", false),
        ("#0", "#\"f\"", false),
        ("#-0", "#0i", false),
        ("#5", "#5.0", false),
        ("#5:ex_a", ":5:ex_a", false),
        (":ex_a", ":\"ex_a\"", false),
        ("#0:ex_a", ":ex_a", false),
        ("#\"ex_a\"", ":ex_a", false),
        (":\"ex_\" \"a\"", "#0:ex_a", false),
        ("#\"\"", "#0", false),
        ("#\"\":ex_a", ":ex_a", false),
    ] {
        let source = format!("void() f = {first};\nvoid() f = {second};\n");
        let expected = if redeclared {
            vec![Code::E0206]
        } else {
            vec![]
        };
        assert_eq!(fte_errors(&source), expected, "{source}");
    }
}

// spec FTE-23.7: FTEQCC reuses the builtin function it made first with a number and engine
// name, so after another definition bound by the same ones, initialising `f` again with its own
// is E0206; a function it made after `f`'s does not matter
#[test]
fn the_first_function_with_a_number_and_name_is_the_one_reused() {
    for (source, redeclared) in [
        (
            "void() g = #5:ex_a;\nvoid() f = #5:ex_a;\nvoid() f = #5:ex_a;\n",
            true,
        ),
        (
            "void() g = :ex_a;\nvoid() f = #0:ex_a;\nvoid() f = :ex_a;\n",
            true,
        ),
        ("void() g = #5:f;\nvoid() f = #5;\nvoid() f = #5;\n", true),
        (
            "void() f = #5:ex_a;\nvoid() g = #5:ex_a;\nvoid() f = #5:ex_a;\n",
            false,
        ),
        ("void() g = #5;\nvoid() f = #5;\nvoid() f = #5;\n", false),
        (
            "void() g = #5:ex_b;\nvoid() f = #5:ex_a;\nvoid() f = #5:ex_a;\n",
            false,
        ),
    ] {
        let expected = if redeclared {
            vec![Code::E0206]
        } else {
            vec![]
        };
        assert_eq!(fte_errors(source), expected, "{source}");
    }
}

// spec FTE-23.7: a redeclaration FTEQCC reuses no function for makes one, which a later
// redeclaration by the same number and name does not reuse either: each is E0206
#[test]
fn a_mismatched_redeclaration_makes_its_own_function() {
    let source = "void() f = #5;\nvoid() f = #6;\nvoid() g = #6:f;\nvoid() g = #6:f;\n";
    assert_eq!(fte_errors(source), [Code::E0206, Code::E0206]);
}

/// The error codes of the one-file module `source` read through the preprocessor under the
/// FTEQCC dialect, as the compiler reads it.
fn fte_analysis_errors(source: &str) -> Vec<Code> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let input = ModuleInput {
        compiled: vec![FileId(0)],
        ..ModuleInput::default()
    };
    db.analyze_module(&input)
        .diagnostics()
        .map(|d| d.diagnostic.code)
        .filter(|code| code.is_error())
        .collect()
}

// spec FTE-23.10: read as the compiler reads it, a `#` before a macro is the macro's expansion,
// and before a name that is no macro E0209
// spec FTE-23.11: a number a macro gives a function is no error here, as no type is checked
#[test]
fn a_hash_before_a_macro_reads_its_expansion() {
    for source in [
        "#define N 5\nvoid() f = #N;\n",
        "#define N \"ex_a\"\nvoid() f = #N;\n",
        "#define N ex_a\nvoid() f = :N;\n",
        "#define M(x) x\nvoid() f = #M(\"ex_a\");\n",
    ] {
        assert_eq!(fte_analysis_errors(source), [], "{source:?}");
    }
    assert_eq!(fte_analysis_errors("void() f = #N;\n"), [Code::E0209]);
    assert_eq!(
        fte_analysis_errors("#define N 5\nvoid() f = # N;\n"),
        [Code::E0209]
    );
}

// spec FTEF-13.7: a prefixed string is a string, after `:` and joined after `#` and a string
#[test]
fn prefixed_strings_are_read_as_strings() {
    for source in [
        "void() f = :u8\"ex_a\";\n",
        "void() f = #\"ex_\" u8\"a\";\n",
        "void() f = #\"ex_\" L\"a\";\n",
        "void() f = :R\"x(ex_a)x\";\n",
    ] {
        assert_eq!(fte_analysis_errors(source), [], "{source:?}");
    }
    assert_eq!(
        fte_analysis_errors("void() f = #u8\"ex_a\";\n"),
        [Code::E0209]
    );
}

// spec FTEF-15.7: a name of 254 bytes after `#number:` is taken and one of 255 is too long
#[test]
fn fteqcc_names_after_a_builtin_number_are_up_to_254_bytes() {
    let long = format!("void() f = #5:{};\n", "n".repeat(255));
    assert_eq!(fte_analysis_errors(&long), [Code::E0202]);
    let short = format!("void() f = #5:{};\n", "n".repeat(254));
    assert_eq!(fte_analysis_errors(&short), []);
}

// spec FTEF-17.10: a builtin FTEQCC binds no number for is compared by its text, so the pinned
// FTEQCC's two "Bad builtin immediate" errors for `#16d` given twice (the parser's E0209) are
// the only ones; the vanilla profile reports every second initialiser
#[test]
fn unbound_builtins_and_vanilla_compare_as_before() {
    assert_eq!(fte_errors("void() f = #16d;\nvoid() f = #16d;\n"), []);
    assert_eq!(
        fte_errors("void() f = #16d;\nvoid() f = #17d;\n"),
        [Code::E0206]
    );
    // A `#` before neither a number nor a string binds nothing, and is no builtin 0.
    for (source, literal) in [
        ("void() f = #;\n", None),
        (
            "void() f = #\"\";\n",
            Some(LiteralValue::Builtin {
                number: 0,
                name: None,
            }),
        ),
        (
            "void() f = :ex_a;\n",
            Some(LiteralValue::Builtin {
                number: 0,
                name: Some("ex_a".to_owned()),
            }),
        ),
    ] {
        let parse = qc_syntax::parse_with(source.as_bytes(), Dialect::Fteqcc);
        let decls = lower_file(&parse, Dialect::Fteqcc).decls;
        let lowered = decls[0].literal.clone();
        match literal {
            None => assert!(
                matches!(lowered, Some(LiteralValue::Text(_))),
                "{source}: {lowered:?}"
            ),
            Some(literal) => assert_eq!(lowered, Some(literal), "{source}"),
        }
    }
    let vanilla: Vec<Code> = index_of("void() f = #5;\nvoid() f = #5;\n")
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.code)
        .collect();
    assert_eq!(vanilla, [Code::E0206]);
}
