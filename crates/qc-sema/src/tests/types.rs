// SPDX-License-Identifier: GPL-3.0-or-later

//! Type tests (QC-10).

use std::rc::Rc;

use qc_syntax::ast;

use qc_syntax::{Dialect, QC_MAX_NESTING};

use crate::db::Db;
use crate::symbol::{DefId, FileId};
use crate::ty::{FROM_AST_CALLS, FunctionType, ParamLabels, SimpleType, Type, TypeContext};
use qc_diag::Span;

/// Lowers the type of the first file-level declaration in `source`.
fn first_decl_type(source: &str) -> Type {
    let parse = qc_syntax::parse(source.as_bytes());
    let tree = parse.tree().expect("a tree");
    let decl = tree.declarations().next().expect("a declaration");
    let ty = decl.ty().expect("a type");
    Type::from_ast(&ty, &|_| TypeContext::default())
}

// spec QC-10.1: the five simple types; a vector is three floats, the rest one word.
#[test]
fn simple_types_lower() {
    assert_eq!(first_decl_type("float a;"), Type::Simple(SimpleType::Float));
    assert_eq!(
        first_decl_type("vector a;"),
        Type::Simple(SimpleType::Vector)
    );
    assert_eq!(
        first_decl_type("string a;"),
        Type::Simple(SimpleType::String)
    );
    assert_eq!(
        first_decl_type("entity a;"),
        Type::Simple(SimpleType::Entity)
    );
    assert_eq!(first_decl_type("void a;"), Type::Simple(SimpleType::Void));
}

// spec QC-10.1: a vector occupies three words, every other type one.
#[test]
fn word_sizes() {
    assert_eq!(Type::Simple(SimpleType::Vector).word_size(), 3);
    assert_eq!(Type::Simple(SimpleType::Float).word_size(), 1);
    assert_eq!(Type::Simple(SimpleType::String).word_size(), 1);
    assert_eq!(Type::Unknown.word_size(), 1);
}

// spec QC-10.7: a variable of type void may be declared and occupies one word.
#[test]
fn void_variable_has_one_word() {
    assert_eq!(first_decl_type("void x;").word_size(), 1);
}

// spec QC-10.2: `.T` is a field type, and field types nest (`..float`).
#[test]
fn field_types_nest() {
    assert_eq!(
        first_decl_type(".float f;"),
        Type::Field(Rc::new(Type::Simple(SimpleType::Float)))
    );
    assert_eq!(
        first_decl_type("..float ff;"),
        Type::Field(Rc::new(Type::Field(Rc::new(Type::Simple(
            SimpleType::Float
        )))))
    );
}

// spec QC-10.3: a function type has a return type and parameter types; names are not part
// of the type.
#[test]
fn function_type_lowers() {
    let ty = first_decl_type("void(float p, vector q) f;");
    assert_eq!(
        ty,
        Type::Function(Rc::new(FunctionType {
            ret: Type::Simple(SimpleType::Void),
            params: vec![
                Type::Simple(SimpleType::Float),
                Type::Simple(SimpleType::Vector),
            ],
            variadic: false,
            labels: ParamLabels::default(),
        }))
    );
}

// spec QC-10.10: two function types are equal when return type, parameter count and
// parameter types match; parameter names do not matter.
#[test]
fn function_types_equal_ignoring_param_names() {
    let a = first_decl_type("void(float p) f;");
    let b = first_decl_type("void(float q) g;");
    assert_eq!(a, b);
}

// spec QC-10.10: a different parameter type makes a different type.
#[test]
fn function_types_differ_by_param_type() {
    let a = first_decl_type("void(float p) f;");
    let b = first_decl_type("void(entity p) g;");
    assert_ne!(a, b);
}

// spec QC-10.4: `(...)` is a variadic function type.
#[test]
fn variadic_function_type() {
    let ty = first_decl_type("void(...) f;");
    match ty {
        Type::Function(fty) => {
            assert!(fty.variadic);
            assert!(fty.params.is_empty());
        }
        _ => panic!("expected a function type"),
    }
}

// spec QC-10.3: a parameter type may itself be a field type.
#[test]
fn parameter_may_be_field_type() {
    let ty = first_decl_type("void(.float p) f;");
    match ty {
        Type::Function(fty) => {
            assert_eq!(
                fty.params,
                vec![Type::Field(Rc::new(Type::Simple(SimpleType::Float)))]
            );
        }
        _ => panic!("expected a function type"),
    }
}

// spec QC-10.1: the keyword spelling round-trips through SimpleType::as_str.
#[test]
fn simple_type_spelling() {
    assert_eq!(SimpleType::Float.as_str(), "float");
    assert_eq!(SimpleType::Vector.as_str(), "vector");
    assert_eq!(SimpleType::Entity.as_str(), "entity");
}

// spec QCD-1.4: the parser recovers from `. f;`, a field type with no inner type, and the
// semantic model lowers the part it left out to Unknown: the type is Field(Unknown)
#[test]
fn malformed_type_is_unknown() {
    let parse = qc_syntax::parse(b". f;");
    assert!(parse.errors().next().is_some(), "`. f;` is a syntax error");
    let tree = parse.tree().expect("a tree");
    let decl = tree.declarations().next().expect("a declaration");
    let Some(ast::Type::Field(field)) = decl.ty() else {
        panic!("`. f;` is a field type: {:?}", decl.ty());
    };
    assert!(field.inner().is_none());
    assert_eq!(
        Type::from_ast(&ast::Type::Field(field), &|_| { TypeContext::default() }),
        Type::Field(std::rc::Rc::new(Type::Unknown))
    );
}

/// The `Type::from_ast` calls that resolving the one-file module `source` in `dialect` takes.
fn from_ast_calls(source: &str, dialect: Dialect) -> usize {
    let mut db = Db::new();
    db.set_dialect(dialect);
    db.set_file_text(FileId::new(0), source.as_bytes().to_vec());
    FROM_AST_CALLS.with(|calls| calls.set(0));
    let _index = db.module_index(&[FileId::new(0)]);
    FROM_AST_CALLS.with(std::cell::Cell::get)
}

/// `void(void(...void f;`: `depth` unclosed `void(`, as a line being typed.
fn unclosed(depth: usize) -> String {
    format!("{}void f;\n", "void(".repeat(depth))
}

/// `void(void(void() a1) a2) f;`: a function type whose parameter is a function type, `depth`
/// levels deep.
fn nested(depth: usize) -> String {
    let mut ty = "void()".to_owned();
    for level in 1..depth {
        ty = format!("void({ty} a{level})");
    }
    format!("{ty} f;\n")
}

// spec QC-10.3: a function type's parameter types are lowered once each, so resolving a
// function type nested QC_MAX_NESTING deep, written whole or left unclosed, takes work linear
// in the depth: at most four calls a level, and doubling the depth at most doubles the calls,
// with a few to spare. The depths double from 8, so lowering that is not linear fails at a
// small depth rather than running for long.
#[test]
fn nested_function_types_lower_in_linear_time() {
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        for shape in [unclosed, nested] {
            let mut depth = 8;
            let mut previous: Option<usize> = None;
            while depth <= QC_MAX_NESTING {
                let calls = from_ast_calls(&shape(depth), dialect);
                assert!(
                    (depth..=4 * depth).contains(&calls),
                    "{dialect:?}: {calls} calls at depth {depth}"
                );
                if let Some(previous) = previous {
                    assert!(calls <= 2 * previous + 8, "{dialect:?}: {calls} at {depth}");
                }
                previous = Some(calls);
                depth *= 2;
            }
        }
    }
}

// spec FTEF-34.9: the check stage keeps the type of each expression it reads, by its span
#[test]
fn a_checked_body_keeps_its_expression_types() {
    let source = "float a; string s; void() g = { a = a + 1; s = \"x\"; };";
    let index = super::fte_index_of(source);
    let g = index
        .defs
        .iter()
        .position(|d| d.name == "g")
        .map(DefId::new)
        .unwrap();
    let body = &index.bodies[&g];
    let at = |text: &str| {
        let start = source.find(text).unwrap();
        body.type_at(Span::new(start, start + text.len()))
    };
    assert_eq!(at("a + 1"), Some(&Type::Simple(SimpleType::Float)));
    assert_eq!(at("\"x\""), Some(&Type::Simple(SimpleType::String)));
    assert_eq!(at("g = {"), None);
}

// spec FTEF-23.4: the check stage keeps the opcode `QCC_PR_ChooseOpcode` picks for each binary
// operator, by the operator's span, the row FTEQCC chose even when the target emulates it
#[test]
fn a_checked_body_keeps_its_operators_opcodes() {
    let source = "float a; vector v; void() g = { a = a + 1; v = v * a; v = v / a; a = a < 2; };";
    let index = super::fte_index_of(source);
    let g = index
        .defs
        .iter()
        .position(|d| d.name == "g")
        .map(DefId::new)
        .unwrap();
    let body = &index.bodies[&g];
    let at = |text: &str, operator: &str| {
        let start = source.find(text).unwrap() + text.find(operator).unwrap();
        body.opcode_at(Span::new(start, start + operator.len()))
    };
    assert_eq!(at("a + 1", "+"), Some(6), "ADD_F");
    assert_eq!(at("v * a", "*"), Some(4), "MUL_VF");
    assert_eq!(
        at("v / a", "/"),
        Some(139),
        "DIV_VF, emulated on the id target"
    );
    assert_eq!(at("a < 2", "<"), Some(22), "LT_F");
    assert_eq!(
        at("a = a + 1", "="),
        None,
        "an assignment is chosen elsewhere"
    );
}

// spec FTEF-23.4: a binary operator lowers to the statement of the opcode chosen for it, its
// operands the operator's inputs, its value the statement's temporary
// spec FTEF-26.2: an operator of two constants folds, to an immediate with no statement
// spec FTEF-27.4: an emulated `%` is its emulation's statements
#[test]
fn a_binary_operator_lowers_to_a_fragment() {
    use crate::ir::{Fragment, Immediate, ImmediateKind, Operand, Statement};
    let source =
        "float a; vector v; void() t = { a = a + 1; v = v * a; a = 2 + 3; a = a % 2; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let body = &index.bodies[&t];
    let at = |text: &str, operator: &str| {
        let start = source.find(text).unwrap() + text.find(operator).unwrap();
        body.fragment_at(Span::new(start, start + operator.len()))
    };
    let float = |value: f32| {
        Operand::Immediate(Immediate {
            kind: ImmediateKind::Float,
            words: [value.to_bits(), 0, 0],
        })
    };
    let one = |op: u16, a: Operand, b: Operand| Fragment {
        statements: vec![Statement {
            op,
            a: Some(a),
            b: Some(b),
            makes: true,
        }],
        value: Some(Operand::Temp(0)),
    };
    assert_eq!(
        at("a + 1", "+"),
        Some(&one(6, Operand::Input(0), Operand::Input(1))),
        "ADD_F: a literal operand is an input too"
    );
    assert_eq!(
        at("v * a", "*"),
        Some(&one(4, Operand::Input(0), Operand::Input(1))),
        "MUL_VF"
    );
    assert_eq!(
        at("2 + 3", "+"),
        Some(&Fragment {
            statements: Vec::new(),
            value: Some(float(5.0)),
        })
    );
    // FTEF-27.4: with no `mod` function, `a % b` is `a - b * (a / b)`, the quotient floored by
    // `BITOR` of itself, each step a statement of the fragment.
    let made = |op: u16, a: Operand, b: Operand| Statement {
        op,
        a: Some(a),
        b: Some(b),
        makes: true,
    };
    assert_eq!(
        at("a % 2", "%"),
        Some(&Fragment {
            statements: vec![
                made(5, Operand::Input(0), Operand::Input(1)),
                made(65, Operand::Temp(0), Operand::Temp(0)),
                made(1, Operand::Input(1), Operand::Temp(1)),
                made(8, Operand::Input(0), Operand::Temp(2)),
            ],
            value: Some(Operand::Temp(3)),
        }),
        "DIV_F, BITOR, MUL_F, SUB_F"
    );
}
