// SPDX-License-Identifier: GPL-3.0-or-later

//! Declarations, types, function definitions and their errors.

use super::{error_codes, errors, kinds};
use crate::ast;
use crate::{Code, LIMIT_MAX_PARMS, LIMIT_QCC_MAX_NAME, SyntaxKind, parse};

/// The type of the first declaration of `source`, in a compact notation: `float`, `.T`,
/// `T(T name, ...)`.
fn type_of(source: &str) -> String {
    let parsed = parse(source.as_bytes());
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    show(&decl.ty().unwrap())
}

fn show(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Simple(simple) => format!("{:?}", simple.kind().unwrap()).to_lowercase(),
        ast::Type::Field(field) => format!(".{}", field.inner().map_or("?".into(), |t| show(&t))),
        ast::Type::Function(function) => {
            let ret = format!("{:?}", function.return_type().unwrap().kind().unwrap());
            let params = function.params().unwrap();
            let list: Vec<String> = if params.is_variadic() {
                vec!["...".to_owned()]
            } else {
                params
                    .params()
                    .map(|p| {
                        format!(
                            "{} {}",
                            p.ty().map_or("?".into(), |t| show(&t)),
                            p.name().map(|n| n.text_lossless()).unwrap_or_default()
                        )
                    })
                    .collect()
            };
            format!("{}({})", ret.to_lowercase(), list.join(", "))
        }
    }
}

// spec QC-11.1: a declaration is a type, names separated by `,` with optional initialisers,
// then `;`
#[test]
fn declaration_list() {
    let parsed = parse(b"float a, b = 1, c;");
    assert_eq!(errors(&parsed), vec![]);
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    let names: Vec<String> = decl
        .declarators()
        .map(|d| d.name().unwrap().text_lossless())
        .collect();
    assert_eq!(names, ["a", "b", "c"]);
    let inits: Vec<bool> = decl
        .declarators()
        .map(|d| d.initializer().is_some())
        .collect();
    assert_eq!(inits, [false, true, false]);
}

// spec QC-11.1: there is nothing but declarations at file level: no statements, no stray `;`
#[test]
fn only_declarations_at_file_level() {
    assert_eq!(
        errors(&parse(b"float a;;")),
        vec![(Code::E0203, "\";\" is not a type".to_owned())]
    );
    assert_eq!(
        errors(&parse(b"x1 = 2;\nfloat b;")),
        vec![(Code::E0203, "\"x1\" is not a type".to_owned())]
    );
    let parsed = parse(b"x1 = 2;\nfloat b;");
    assert_eq!(parsed.tree().unwrap().declarations().count(), 1);
}

// spec QC-11.2: functions in a list are separated by `,` and the `;` after a body is required
#[test]
fn function_definitions_in_a_list() {
    let parsed = parse(b"void() a = {}, b = {};\nvoid() f = { a(); b(); };");
    assert_eq!(errors(&parsed), vec![]);
    let first = parsed.tree().unwrap().declarations().next().unwrap();
    let bodies = first
        .declarators()
        .filter(|d| matches!(d.initializer(), Some(ast::Initializer::Body(_))))
        .count();
    assert_eq!(bodies, 2);
    assert_eq!(
        errors(&parse(b"void() f = {}\nfloat a;")),
        vec![(Code::E0204, "expected ;, found float".to_owned())]
    );
}

// spec QC-10.2: `.T` is a field type for any `T`, function and field types included
#[test]
fn field_types() {
    assert_eq!(type_of(".float health;"), ".float");
    assert_eq!(type_of("..float ff;"), "..float");
    assert_eq!(type_of(".void() th;"), ".void()");
    assert_eq!(type_of(".vector(float a) fv;"), ".vector(float a)");
}

// spec QC-10.3: `T(T1 n1, ...)` is a function type; parameters may have field or function types
#[test]
fn function_types() {
    assert_eq!(type_of("void() f;"), "void()");
    assert_eq!(
        type_of("entity(entity start, .string fld, string match) find;"),
        "entity(entity start, .string fld, string match)"
    );
    assert_eq!(
        type_of("void(float(float x) fn, .float fld) apply;"),
        "void(float(float x) fn, .float fld)"
    );
    assert_eq!(
        error_codes("void(float(float x) fn, .float fld) apply;"),
        vec![]
    );
}

// spec QC-10.4: `T(...)` takes any arguments, and `...` must be the whole list
#[test]
fn variadic_function_types() {
    assert_eq!(type_of("void(...) bprint;"), "void(...)");
    assert_eq!(
        errors(&parse(b"void(float a, ...) f;")),
        vec![(Code::E0203, "\"...\" is not a type".to_owned())]
    );
    assert_eq!(
        errors(&parse(b"void(..., float a) f;")),
        vec![(Code::E0204, "expected ), found ,".to_owned())]
    );
}

// spec QC-10.5: a function type cannot return a function, so `void()()` is not a type
#[test]
fn no_function_returning_function() {
    assert_eq!(
        errors(&parse(b"void()() f;")),
        vec![(Code::E0201, "not a name".to_owned())]
    );
}

// spec QC-10.9: there is no `int`, array, struct, pointer or `const` type
#[test]
fn no_other_types() {
    for source in ["int x;", "const float x;", "struct s;"] {
        let word = source.split(' ').next().unwrap();
        assert_eq!(
            errors(&parse(source.as_bytes())),
            vec![(Code::E0203, format!("\"{word}\" is not a type"))]
        );
    }
    assert_eq!(
        errors(&parse(b"float x[3];")),
        vec![(Code::E0204, "expected ;, found [".to_owned())]
    );
}

// spec QC-27.7, QCD-6.5: a function type with more than eight parameters is an error, E0505
#[test]
fn more_than_eight_parameters() {
    let params = |n: usize| {
        (0..n)
            .map(|i| format!("float p{i}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let eight = format!("void({}) f;", params(LIMIT_MAX_PARMS));
    assert_eq!(error_codes(&eight), vec![]);
    let nine = format!("void({}) f;", params(LIMIT_MAX_PARMS + 1));
    let parsed = parse(nine.as_bytes());
    assert_eq!(
        errors(&parsed),
        vec![(
            Code::E0505,
            "more than eight parameters in a function type".to_owned()
        )]
    );
    let span = parsed.errors().next().unwrap().span;
    assert_eq!(&nine[span.range()], &nine[4..nine.len() - 3]);
}

// spec QC-3.3, QCD-3.2: a name of 63 bytes or more is "name too long" (E0202), 62 is accepted
#[test]
fn name_too_long() {
    let ok = "n".repeat(LIMIT_QCC_MAX_NAME - 2);
    let long = "n".repeat(LIMIT_QCC_MAX_NAME - 1);
    assert_eq!(error_codes(&format!("float {ok};")), vec![]);
    assert_eq!(error_codes(&format!("float {long};")), vec![Code::E0202]);
    assert_eq!(
        error_codes(&format!("void(float {long}) f;")),
        vec![Code::E0202]
    );
    assert_eq!(
        error_codes(&format!("float {ok};\nvoid() f = {{ {long} = 1; }};")),
        vec![Code::E0202]
    );
}

// spec QCD-3.1: a name is required: "not a name"
#[test]
fn not_a_name() {
    assert_eq!(
        errors(&parse(b"float 5;")),
        vec![(Code::E0201, "not a name".to_owned())]
    );
    assert_eq!(
        errors(&parse(b"float")),
        vec![(Code::E0201, "not a name".to_owned())]
    );
}

// spec QCD-3.3: a type is required: "\"TOKEN\" is not a type"
#[test]
fn not_a_type() {
    assert_eq!(
        errors(&parse(b".5 x;")),
        vec![(Code::E0203, "\"5\" is not a type".to_owned())]
    );
    assert_eq!(
        errors(&parse(b"void(x) f;")),
        vec![(Code::E0203, "\"x\" is not a type".to_owned())]
    );
}

// spec QC-13.1: `= {}`, `= [frame, next] {}` and `= #n` define a function; a body may be empty
#[test]
fn function_definition_forms() {
    let parsed = parse(b"void() a = {};\nvoid() b = [1, a] { };\nvoid() c = #7;");
    assert_eq!(errors(&parsed), vec![]);
    let inits: Vec<_> = parsed
        .tree()
        .unwrap()
        .declarations()
        .map(|d| d.declarators().next().unwrap().initializer().unwrap())
        .collect();
    assert!(matches!(&inits[0], ast::Initializer::Body(b) if b.state().is_none()));
    assert!(matches!(&inits[1], ast::Initializer::Body(b) if b.state().is_some()));
    assert!(matches!(&inits[2], ast::Initializer::Builtin(_)));
}

// spec QCD-3.9: `#` must be followed by an integral float literal: "Bad builtin immediate"
#[test]
fn bad_builtin_immediate() {
    for source in [
        "void() f = #1.5;",
        "void() f = #x;",
        "void() f = #'1 2 3';",
        "void() f = #;",
    ] {
        assert_eq!(
            errors(&parse(source.as_bytes())),
            vec![(Code::E0209, "Bad builtin immediate".to_owned())],
            "{source}"
        );
    }
    assert_eq!(error_codes("$frame a\nvoid() f = #$a;"), vec![]);
}

// spec QC-27.9, QCD-6.7: `#0` and negative builtin numbers are errors, E0507
#[test]
fn builtin_number_must_be_positive() {
    for source in ["void() f = #0;", "void() f = #-1;", "void() f = #-0;"] {
        assert_eq!(
            errors(&parse(source.as_bytes())),
            vec![(Code::E0507, "builtin number must be positive".to_owned())],
            "{source}"
        );
    }
    assert_eq!(error_codes("void() f = #1;"), vec![]);
}

// spec QCD-3.10: a state frame must be a float literal or frame macro reference
#[test]
fn state_frame_must_be_a_number() {
    assert_eq!(error_codes("void() n; void() f = [1, n] {};"), vec![]);
    assert_eq!(
        error_codes("$frame s\nvoid() n; void() f = [$s, n] {};"),
        vec![]
    );
    for frame in ["a", "\"x\"", "'1 2 3'", "(1)"] {
        let source = format!("void() n; void() f = [{frame}, n] {{}};");
        assert_eq!(
            errors(&parse(source.as_bytes())),
            vec![(Code::E0210, "state frame must be a number".to_owned())],
            "{source}"
        );
    }
}

// spec QC-27.8, QCD-6.6: an initialiser that is not a literal or frame macro reference is an
// error, E0506
#[test]
fn initialiser_is_not_a_constant() {
    for source in [
        "float x = 1;",
        "float x = -5;",
        "vector v = '1 2 3';",
        "string s = \"x\";",
        "$frame a b\nfloat x = $b;",
    ] {
        assert_eq!(error_codes(source), vec![], "{source}");
    }
    for source in [
        "float y; float x = y;",
        "float x = (1);",
        "float x = 1 + 2;",
        "float x = {};",
        "float x = #1;",
    ] {
        assert_eq!(
            errors(&parse(source.as_bytes())),
            vec![(Code::E0506, "initialiser is not a constant".to_owned())],
            "{source}"
        );
    }
    let source = "float x = 1 + 2;";
    let span = parse(source.as_bytes()).errors().next().unwrap().span;
    assert_eq!(&source[span.range()], "1 + 2");
}

// spec QCD-3.4: a missing `=` before a function body is "expected ;, found {"
#[test]
fn missing_equals_before_body() {
    let parsed = parse(b"void() f {};\nfloat a;");
    assert_eq!(
        errors(&parsed),
        vec![(Code::E0204, "expected ;, found {".to_owned())]
    );
    assert!(kinds(&parsed).contains(&SyntaxKind::FunctionBody));
    assert_eq!(parsed.tree().unwrap().declarations().count(), 2);
}

// spec QCD-3.4: a function initialiser must start with `{`, `[` or `#`
#[test]
fn function_initialiser_needs_a_body() {
    assert_eq!(
        errors(&parse(b"void() g; void() f = g;")),
        vec![(Code::E0204, "expected {, found g".to_owned())]
    );
}

// spec QC-10.4: after `"..." is not a type`, the rest of the parameter is skipped and the
// declaration keeps its name
#[test]
fn bad_parameter_is_skipped() {
    let parsed = parse(b"void(float a, ... x) f;\nfloat b;");
    assert_eq!(
        errors(&parsed),
        vec![(Code::E0203, "\"...\" is not a type".to_owned())]
    );
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    let name = decl.declarators().next().unwrap().name().unwrap();
    assert_eq!(name.text(), b"f");
    let skipped = parsed
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::Error)
        .unwrap();
    assert_eq!(skipped.text(), b"... x");
    assert_eq!(skipped.parent().unwrap().kind(), SyntaxKind::Param);
}
