// SPDX-License-Identifier: GPL-3.0-or-later

//! Type tests (QC-10).

use std::rc::Rc;

use qc_syntax::ast;

use qc_syntax::{Dialect, QC_MAX_NESTING};

use crate::db::Db;
use crate::ir::{Fragment, Immediate, ImmediateKind};
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
    use qc_preproc::Op;
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
    assert_eq!(at("a + 1", "+"), Some(Op::AddF), "ADD_F");
    assert_eq!(at("v * a", "*"), Some(Op::MulVf), "MUL_VF");
    assert_eq!(
        at("v / a", "/"),
        Some(Op::DivVf),
        "DIV_VF, emulated on the id target"
    );
    assert_eq!(at("a < 2", "<"), Some(Op::LtF), "LT_F");
    assert_eq!(
        at("a = a + 1", "="),
        Some(Op::StoreF),
        "an assignment keeps the store of its target's type"
    );
}

// spec QC-16.1: under id's rules each binary operator keeps the opcode of its row, the first
// whose operand types are the operands', and lowers to that opcode's one statement of its two
// operands into a new temporary
#[test]
fn a_vanilla_operator_keeps_its_row_and_lowers_to_its_statement() {
    use crate::ir::{Destination, Fragment, Input, Operand, Statement, Temp, TempId};
    use qc_preproc::Op;
    let source = "float a; vector v; string s; entity e; void() g = { a = a * a; a = v * v; \
                  v = a * v; v = v * a; v = v - v; a = a / a; a = s != s; a = e == e; \
                  a = a && a; a = a | a; };";
    let index = super::index_of(source);
    let g = index
        .defs
        .iter()
        .position(|d| d.name == "g")
        .map(DefId::new)
        .unwrap();
    let body = &index.bodies[&g];
    let span = |text: &str| {
        let start = source.find(text).unwrap() + 2;
        let len = text.len() - 4;
        Span::new(start, start + len)
    };
    for (text, op) in [
        ("a * a", Op::MulF),
        ("v * v", Op::MulV),
        ("a * v", Op::MulFv),
        ("v * a", Op::MulVf),
        ("v - v", Op::SubV),
        ("a / a", Op::DivF),
        ("s != s", Op::NeS),
        ("e == e", Op::EqE),
        ("a && a", Op::AndF),
        ("a | a", Op::BitorF),
    ] {
        assert_eq!(body.opcode_at(span(text)), Some(op), "{text}");
        let fragment = Fragment {
            statements: vec![Statement {
                op,
                a: Some(Operand::Input(Input::Left)),
                b: Some(Operand::Input(Input::Right)),
                c: Destination::Value,
            }],
            immediates: Vec::new(),
            temps: vec![Temp::Made(0)],
            value: Some(Operand::Temp(TempId::new(0))),
            inputs: [None, None],
        };
        assert_eq!(body.fragment_at(span(text)), Some(&fragment), "{text}");
    }
}

/// The float immediate of `value`.
fn float_immediate(value: f32) -> Immediate {
    Immediate {
        kind: ImmediateKind::Float,
        words: [value.to_bits(), 0, 0],
    }
}

// spec FTEF-23.4: a binary operator lowers to the statement of the opcode chosen for it, its
// operands the operator's inputs, its value the statement's temporary
// spec FTEF-26.2: an operator of two constants folds, to an immediate with no statement
// spec FTEF-27.4: an emulated `%` is its emulation's statements
// spec FTE-19.9: `v / f` is `v * (1 / f)`
// spec FTE-19.12: `v & w` of vectors is BITAND_F of each word into a vector temporary
#[test]
fn a_binary_operator_lowers_to_a_fragment() {
    use crate::ir::{
        Destination, Fragment, Immediate, ImmediateKind, Input, Operand, Statement, Temp, TempId,
        Whole,
    };
    use qc_preproc::Op;
    let source = "float a; vector v; void() t = { a = a + 1; v = v * a; a = 2 + 3; a = a % 2; \
                  v = v / a; v = v & v; };";
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
    let constant = |value| Some(float_immediate(value));
    let one = |op: Op, a: Operand, b: Operand, inputs| Fragment {
        statements: vec![Statement {
            op,
            a: Some(a),
            b: Some(b),
            c: Destination::Value,
        }],
        immediates: Vec::new(),
        temps: vec![Temp::Made(0)],
        value: Some(Operand::Temp(TempId::new(0))),
        inputs,
    };
    assert_eq!(
        at("a + 1", "+"),
        Some(&one(
            Op::AddF,
            Operand::Input(Input::Left),
            Operand::Input(Input::Right),
            [None, None]
        )),
        "ADD_F: a literal operand is an input too"
    );
    assert_eq!(
        at("v * a", "*"),
        Some(&one(
            Op::MulVf,
            Operand::Input(Input::Left),
            Operand::Input(Input::Right),
            [None, None]
        )),
        "MUL_VF"
    );
    assert_eq!(
        at("2 + 3", "+"),
        Some(&Fragment {
            statements: Vec::new(),
            immediates: vec![Immediate {
                kind: ImmediateKind::Float,
                words: [5.0_f32.to_bits(), 0, 0],
            }],
            temps: Vec::new(),
            value: Some(float(5.0)),
            inputs: [constant(2.0), constant(3.0)],
        }),
        "the folded value is an immediate the lowering made"
    );
    // FTEF-27.4: with no `mod` function, `a % b` is `a - b * (a / b)`, the quotient floored by
    // `BITOR` of itself, each step a statement of the fragment.
    let made = |op: Op, a: Operand, b: Operand| Statement {
        op,
        a: Some(a),
        b: Some(b),
        c: Destination::Value,
    };
    assert_eq!(
        at("a % 2", "%"),
        Some(&Fragment {
            statements: vec![
                made(
                    Op::DivF,
                    Operand::Input(Input::Left),
                    Operand::Input(Input::Right)
                ),
                made(
                    Op::BitorF,
                    Operand::Temp(TempId::new(0)),
                    Operand::Temp(TempId::new(0))
                ),
                made(
                    Op::MulF,
                    Operand::Input(Input::Right),
                    Operand::Temp(TempId::new(1))
                ),
                made(
                    Op::SubF,
                    Operand::Input(Input::Left),
                    Operand::Temp(TempId::new(2))
                ),
            ],
            immediates: Vec::new(),
            temps: (0..4).map(Temp::Made).collect(),
            value: Some(Operand::Temp(TempId::new(3))),
            inputs: [None, None],
        }),
        "DIV_F, BITOR, MUL_F, SUB_F"
    );
    // FTE-19.9: `v / f` is `v * (1 / f)`, the constant 1 an immediate the lowering made.
    assert_eq!(
        at("v / a", "/"),
        Some(&Fragment {
            statements: vec![
                made(Op::DivF, float(1.0), Operand::Input(Input::Right)),
                made(
                    Op::MulVf,
                    Operand::Input(Input::Left),
                    Operand::Temp(TempId::new(0))
                ),
            ],
            immediates: vec![Immediate {
                kind: ImmediateKind::Float,
                words: [1.0_f32.to_bits(), 0, 0],
            }],
            temps: vec![Temp::Made(0), Temp::Made(1)],
            value: Some(Operand::Temp(TempId::new(1))),
            inputs: [None, None],
        }),
        "DIV_F of 1, MUL_VF"
    );
    // FTE-19.12: `v & w` of vectors is a vector temporary and `BITAND_F` of each word into it.
    let word = |i: u8| Statement {
        op: Op::BitandF,
        a: Some(Operand::Word(Whole::Input(Input::Left), i)),
        b: Some(Operand::Word(Whole::Input(Input::Right), i)),
        c: Destination::Word(TempId::new(0), i),
    };
    assert_eq!(
        at("v & v", "&"),
        Some(&Fragment {
            statements: (0..3).map(word).collect(),
            immediates: Vec::new(),
            temps: vec![Temp::Vector(0)],
            value: Some(Operand::Temp(TempId::new(0))),
            inputs: [None, None],
        }),
        "three BITAND_F"
    );
}

// spec FTE-19.8: with a `mod` function defined, `a % b` is a call of it: each operand stored
// to its parameter, then `CALL2` of the function, the value its return
#[test]
fn a_helper_call_is_its_fragment() {
    use crate::ir::{Destination, Input, Operand, Statement};
    use qc_preproc::Op;
    let source =
        "float(float a, float b) mod = { return a; };\nfloat a; void() t = { a = a % 2; };";
    let index = super::fte_index_of(source);
    let function = index
        .defs
        .iter()
        .position(|d| d.name == "mod")
        .map(DefId::new)
        .unwrap();
    let fragment = fte_fragment_at(source, "a % 2", "%");
    let statement = |op, a, b| Statement {
        op,
        a,
        b,
        c: Destination::Nothing,
    };
    assert_eq!(
        fragment.statements,
        [
            statement(
                Op::StoreF,
                Some(Operand::Input(Input::Left)),
                Some(Operand::Parameter(0))
            ),
            statement(
                Op::StoreF,
                Some(Operand::Input(Input::Right)),
                Some(Operand::Parameter(1))
            ),
            statement(Op::Call2, Some(Operand::Def(function)), None),
        ]
    );
    assert_eq!(fragment.value, Some(Operand::Return));
}

// spec FTEF-26.13: a word of a constant vector is a constant, so an operator of it and a
// constant folds, to an immediate with no statement
#[test]
fn a_constant_vectors_component_is_a_constant() {
    use crate::ir::{Immediate, ImmediateKind, Operand};
    let source = "float a; void() t = { a = ('7 16384 3').y & 1; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let start = source.find(" & 1").unwrap() + 1;
    let fragment = index.bodies[&t]
        .fragment_at(Span::new(start, start + 1))
        .unwrap();
    assert!(fragment.statements.is_empty(), "{fragment:?}");
    assert_eq!(
        fragment.value,
        Some(Operand::Immediate(Immediate {
            kind: ImmediateKind::Float,
            words: [0.0_f32.to_bits(), 0, 0],
        }))
    );
}

// spec FTE-19.7: `a ^ b` is `(a & ~b) | (b & ~a)`, each `~` `SUB_F` of the immediate -1 and its
// operand, a vector's `SUB_V` of '-1 -1 -1'
#[test]
fn xor_lowers_with_complements_of_minus_one() {
    use crate::ir::{Destination, Immediate, ImmediateKind, Input, Operand, Statement};
    use qc_preproc::Op;
    let source = "float a, b; vector v, w; void() t = { a = a ^ b; v = v ^ w; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let at = |text: &str| {
        let start = source.find(text).unwrap() + 2;
        index.bodies[&t]
            .fragment_at(Span::new(start, start + 1))
            .cloned()
            .unwrap()
    };
    let minus_one = |kind: ImmediateKind| Immediate {
        kind,
        words: match kind {
            ImmediateKind::Vector => [(-1.0_f32).to_bits(); 3],
            ImmediateKind::Float | ImmediateKind::Integer => [(-1.0_f32).to_bits(), 0, 0],
        },
    };
    let floats = at("a ^ b");
    assert_eq!(
        floats.statements.first(),
        Some(&Statement {
            op: Op::SubF,
            a: Some(Operand::Immediate(minus_one(ImmediateKind::Float))),
            b: Some(Operand::Input(Input::Right)),
            c: Destination::Value,
        }),
        "SUB_F of -1 and b"
    );
    assert_eq!(floats.immediates, [minus_one(ImmediateKind::Float); 2]);
    let vectors = at("v ^ w");
    assert_eq!(
        vectors.statements.first(),
        Some(&Statement {
            op: Op::SubV,
            a: Some(Operand::Immediate(minus_one(ImmediateKind::Vector))),
            b: Some(Operand::Input(Input::Right)),
            c: Destination::Value,
        }),
        "SUB_V of '-1 -1 -1' and w"
    );
}

/// The fragment of the operator `operator` at the first `text` in `t` of `source`, under FTEQCC.
fn fte_fragment_at(source: &str, text: &str, operator: &str) -> Fragment {
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let start = source
        .find(text)
        .unwrap()
        .saturating_add(text.find(operator).unwrap());
    index.bodies[&t]
        .fragment_at(Span::new(start, start.saturating_add(operator.len())))
        .unwrap()
        .clone()
}

// spec FTEF-32.17: a call of a constant vector's component is not folded, so an operator of it
// is a statement; a call of a constant read directly folds, and so does the operator
#[test]
fn a_call_of_a_vector_component_is_not_folded() {
    let source = "float(float f) floor = #37; float a; vector v; \
                  void() t = { a = floor(('4 0 0').x) & 1; a = floor(4) & 1; \
                  a = floor(('4 0 0')[0]) | 1; };";
    let component = fte_fragment_at(source, ") & 1", "&");
    assert_eq!(component.statements.len(), 1, "{component:?}");
    let indexed = fte_fragment_at(source, ") | 1", "|");
    assert_eq!(indexed.statements.len(), 1, "{indexed:?}");
    let direct = fte_fragment_at(source, "(4) & 1", "&");
    assert!(direct.statements.is_empty(), "{direct:?}");
}

// spec FTEF-13.3: a character constant is its character's code, so `a - '0'` subtracts 48 and
// `'0' + 1` folds to 49
#[test]
fn a_character_constant_is_its_code() {
    use crate::ir::{Immediate, ImmediateKind, Operand};
    let source = "float a; void() t = { a = a - '0'; a = '0' + 1; };";
    let subtract = fte_fragment_at(source, "a - '0'", "-");
    assert_eq!(
        subtract.statements.len(),
        1,
        "a - 48 is no identity: {subtract:?}"
    );
    let folded = fte_fragment_at(source, "'0' + 1", "+");
    assert_eq!(
        folded.value,
        Some(Operand::Immediate(Immediate {
            kind: ImmediateKind::Float,
            words: [49.0_f32.to_bits(), 0, 0],
        }))
    );
}

// spec FTE-19.10: `&&` and `||` of operands that are not both floats are `AND` and `OR` of
// their words, a vector first its dot product with itself under `vectorlogic`
#[test]
fn logic_of_other_types_reads_their_truths() {
    use crate::ir::{Destination, Input, Operand, Statement, TempId};
    use qc_preproc::Op;
    let source = "vector v; entity e; float a; void() t = { a = a && v; a = a || e; };";
    let statement = |op, a, b| Statement {
        op,
        a: Some(a),
        b: Some(b),
        c: Destination::Value,
    };
    let (left, right) = (Operand::Input(Input::Left), Operand::Input(Input::Right));
    assert_eq!(
        fte_fragment_at(source, "a && v", "&&").statements,
        [
            statement(Op::MulV, right, right),
            statement(Op::AndF, left, Operand::Temp(TempId::new(0))),
        ]
    );
    assert_eq!(
        fte_fragment_at(source, "a || e", "||").statements,
        [statement(Op::OrF, left, right)]
    );
}

// spec FTEF-30.1: `!e` lowers to the `NOT_*` of its operand's type, its operand the fragment's
// input; FTEF-26.3: of a constant it folds to the float of its truth, with no statement
#[test]
fn a_not_lowers_to_a_fragment() {
    use crate::ir::{Destination, Input, Operand, Statement};
    use qc_preproc::Op;
    let source = "float a; string s; void() t = { a = !a; a = !s; a = !0; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let at = |text: &str| {
        let start = source.find(text).unwrap();
        index.bodies[&t]
            .fragment_at(Span::new(start, start + text.len()))
            .cloned()
    };
    let not = |op| Statement {
        op,
        a: Some(Operand::Input(Input::Left)),
        b: None,
        c: Destination::Value,
    };
    assert_eq!(at("!a").unwrap().statements, [not(Op::NotF)]);
    assert_eq!(at("!s").unwrap().statements, [not(Op::NotS)]);
    let folded = at("!0").unwrap();
    assert!(folded.statements.is_empty());
    assert_eq!(folded.value, Some(Operand::Immediate(float_immediate(1.0))));
}

// spec FTEF-30.4: `-e` lowers to the subtraction of its operand from its type's zero
// spec FTEF-27.9: `~e` of a float on the vanilla opcodes is `-1 - e`
#[test]
fn a_negation_and_a_complement_lower_to_fragments() {
    use crate::ir::{Destination, Input, Operand, Statement};
    use qc_preproc::Op;
    let source = "float a; void() t = { a = -a; a = ~a; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let at = |text: &str| {
        let start = source.find(text).unwrap();
        index.bodies[&t]
            .fragment_at(Span::new(start, start + text.len()))
            .cloned()
    };
    let minus = |from: f32| Statement {
        op: Op::SubF,
        a: Some(Operand::Immediate(float_immediate(from))),
        b: Some(Operand::Input(Input::Left)),
        c: Destination::Value,
    };
    assert_eq!(at("-a").unwrap().statements, [minus(0.0)]);
    assert_eq!(at("~a").unwrap().statements, [minus(-1.0)]);
}

// spec QC-16.4: under id's rules `!e` lowers to the `NOT_*` of its operand's type
#[test]
fn a_vanilla_not_lowers_to_its_statement() {
    use crate::ir::{Destination, Input, Operand, Statement};
    use qc_preproc::Op;
    let source = "float a; vector v; string s; entity e; void() g = { a = !a; a = !v; a = !s; \
                  a = !e; a = !g; };";
    let index = super::index_of(source);
    let g = index
        .defs
        .iter()
        .position(|d| d.name == "g")
        .map(DefId::new)
        .unwrap();
    for (text, op) in [
        ("!a", Op::NotF),
        ("!v", Op::NotV),
        ("!s", Op::NotS),
        ("!e", Op::NotEnt),
        ("!g", Op::NotFnc),
    ] {
        let start = source.find(text).unwrap();
        let fragment = index.bodies[&g]
            .fragment_at(Span::new(start, start + 2))
            .unwrap();
        assert_eq!(
            fragment.statements,
            [Statement {
                op,
                a: Some(Operand::Input(Input::Left)),
                b: None,
                c: Destination::Value,
            }],
            "{text}"
        );
    }
}

// spec QC-16.1: a field read is the `LOAD_*` of the field's type, of the entity and the field,
// the fragment kept by the `.`'s span, under id's rules and FTEQCC's (FTEF-34.9)
#[test]
fn a_field_read_lowers_to_its_load() {
    use crate::ir::{Destination, Input, Operand, Statement};
    use qc_preproc::Op;
    let source = ".float hp; .vector org; .string nm; entity e; float a; vector v; string s; \
                  void() t = { a = e.hp; v = e.org; s = e.nm; };";
    let load = |op| Statement {
        op,
        a: Some(Operand::Input(Input::Left)),
        b: Some(Operand::Input(Input::Right)),
        c: Destination::Value,
    };
    for index in [super::index_of(source), super::fte_index_of(source)] {
        let t = index
            .defs
            .iter()
            .position(|d| d.name == "t")
            .map(DefId::new)
            .unwrap();
        for (text, op) in [
            ("e.hp", Op::LoadF),
            ("e.org", Op::LoadV),
            ("e.nm", Op::LoadS),
        ] {
            let dot = source.find(text).unwrap() + 1;
            let fragment = index.bodies[&t]
                .fragment_at(Span::new(dot, dot + 1))
                .unwrap();
            assert_eq!(fragment.statements, [load(op)], "{text}");
        }
    }
}

// spec FTEF-30.7: `[x, y, z]` of variables stores each component to its word of a vector
// temporary, the fragment kept by the constructor's span
#[test]
fn a_vector_constructor_stores_its_components() {
    use crate::ir::{Destination, Input, Operand, Statement, Temp, TempId, Whole};
    use qc_preproc::Op;
    let source = "float a; float b; vector v; void() t = { v = [a, b, a]; };";
    let fragment = fte_fragment_at(source, "[a, b, a]", "[a, b, a]");
    let store = |n: u8| Statement {
        op: Op::StoreF,
        a: Some(Operand::Input(Input::Argument(n))),
        b: Some(Operand::Word(Whole::Temp(TempId::new(0)), n)),
        c: Destination::Nothing,
    };
    assert_eq!(fragment.statements, [store(0), store(1), store(2)]);
    assert_eq!(fragment.temps, [Temp::Vector(0)]);
    assert_eq!(fragment.value, Some(Operand::Temp(TempId::new(0))));
}

// The checked body keeps the opcodes of the statements it generated, in order, its closing
// `DONE` included (QC-13.7): the model of the whole body the intermediate layer is built from
// (`docs/design/back-end.md`)
#[test]
fn a_checked_body_keeps_the_statements_it_generated() {
    use qc_preproc::Op;
    let source = "float a; vector v; void() t = { a = a + 1; v = v * a; a = !a; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    assert_eq!(
        index.bodies[&t]
            .generated()
            .iter()
            .map(|statement| statement.op)
            .collect::<Vec<_>>(),
        [Op::AddF, Op::MulVf, Op::NotF, Op::Done]
    );
}

// QC-14.4, QC-14.5, FTEF-34.16: the generated body holds its jumps: an `if`'s `IFNOT`, an
// `IF` where the test was `!x` of a float, a `while`'s test and `GOTO` back, a `return`'s
// `RETURN`
#[test]
fn a_generated_body_holds_its_jumps() {
    use qc_preproc::Op;
    let source =
        "float a; void() t = { if (a) a = 1; if (!a) a = 2; while (a) a = a - 1; return; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    assert_eq!(
        index.bodies[&t]
            .generated()
            .iter()
            .map(|statement| statement.op)
            .collect::<Vec<_>>(),
        [
            Op::IfnotI,
            Op::StoreF,
            Op::IfI,
            Op::StoreF,
            Op::IfnotI,
            Op::SubF,
            Op::Goto,
            Op::Return,
            Op::Done,
        ]
    );
}

// QC-14.4, QC-14.5: each jump names its target, the generated statement it goes to, and a
// conditional jump its condition: an `if`'s `IFNOT` past its arm, a `while`'s test to its end,
// `break` to the loop's end, `continue` and the loop's `GOTO` back to its test
#[test]
fn a_generated_body_names_its_jumps() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float a; void() t = { if (a) a = 1; while (a) { if (a) break; continue; } };";
    let index = super::fte_index_of(source);
    let def = |name: &str| {
        index
            .defs
            .iter()
            .position(|d| d.name == name)
            .map(DefId::new)
            .unwrap()
    };
    let a = Some(Name::Def(def("a"), 0));
    let jumps: Vec<_> = index.bodies[&def("t")]
        .generated()
        .iter()
        .map(|statement| (statement.op, statement.a, statement.b))
        .filter(|(op, _, _)| matches!(op, Op::IfnotI | Op::Goto))
        .collect();
    let to = |at| Some(Name::Jump(at));
    assert_eq!(
        jumps,
        [
            (Op::IfnotI, a, to(2)),
            (Op::IfnotI, a, to(7)),
            (Op::IfnotI, a, to(5)),
            (Op::Goto, to(7), None),
            (Op::Goto, to(2), None),
            (Op::Goto, to(2), None),
        ]
    );
}

// FTE-18.13, FTEF-29.13: a call's value a later call copied out of the return global is
// named by its copy where an argument store reads it; a postfix's value and its step read the
// copy of the old value, and an element at a constant index is stored to its own word
#[test]
fn a_generated_body_names_copies_of_values() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float(float x) f; float(float a, float b) g; float k; float r[3]; \
        void() t = { k = g(f(1), f(2)); k = (k--) > 0; r[2] = k; };";
    let index = super::fte_index_of(source);
    let def = |name: &str| {
        index
            .defs
            .iter()
            .position(|d| d.name == name)
            .map(DefId::new)
            .unwrap()
    };
    let generated = index.bodies[&def("t")].generated();
    let made = |at: usize| u32::try_from(at).ok().map(Name::Made);
    let copy = generated
        .iter()
        .position(|s| s.op == Op::StoreF && s.a == Some(Name::Return))
        .unwrap();
    assert_eq!(generated[copy].b, made(copy));
    let first = generated
        .iter()
        .position(|s| s.b == Some(Name::Parameter(0)) && s.a == made(copy));
    assert!(first.is_some(), "the copy is stored to g's first parameter");
    let old = generated
        .iter()
        .position(|s| s.op == Op::StoreF && s.a == Some(Name::Def(def("k"), 0)))
        .unwrap();
    assert_eq!(generated[old].b, made(old));
    let step = &generated[old.wrapping_add(1)];
    assert_eq!((step.op, step.a), (Op::AddF, made(old)));
    let test = generated.iter().find(|s| s.op == Op::GtF).unwrap();
    assert_eq!(test.a, made(old));
    let element = generated.iter().rev().find(|s| s.op == Op::StoreF).unwrap();
    assert_eq!(element.b, Some(Name::Def(def("r"), 2)));
}

// FTEF-19.78: an update of an element at a variable index reads it by `ArrayGet`, the index
// stored to its parameter and held for the store, and stores it by `ArraySet`: the index and
// the value to its parameters, then `CALL2`; a plain read folds the index into the parameter
#[test]
fn a_generated_body_updates_an_element_by_its_array_functions() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float i, k; float arr[4]; void() t = { arr[i * 2] -= 1; k = arr[i * 3]; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let generated = index.bodies[&t].generated();
    let ops: Vec<_> = generated.iter().map(|s| s.op).collect();
    assert_eq!(
        ops,
        [
            Op::MulF,
            Op::StoreF,
            Op::Call1,
            Op::SubF,
            Op::StoreF,
            Op::StoreF,
            Op::Call2,
            Op::MulF,
            Op::Call1,
            Op::StoreF,
            Op::Done,
        ]
    );
    let made = |at: u32| Some(Name::Made(at));
    let parameter = |slot| Some(Name::Parameter(slot));
    assert_eq!((generated[1].a, generated[1].b), (made(0), parameter(0)));
    assert_eq!((generated[4].a, generated[4].b), (made(0), parameter(0)));
    assert_eq!((generated[5].a, generated[5].b), (made(3), parameter(1)));
    assert_eq!(generated[7].c, parameter(0));
    let arr = index
        .defs
        .iter()
        .position(|d| d.name == "arr")
        .map(DefId::new)
        .unwrap();
    assert_eq!(generated[2].a, Some(Name::ArrayGet(arr)));
    assert_eq!(generated[6].a, Some(Name::ArraySet(arr)));
}

// FTE-18.6: a variadic function's body starts, before its `STATE`, with `STORE_V` of each of
// the 24 parameter slots after its named ones to the element of its `__va_list`
#[test]
fn a_generated_variadic_body_starts_with_its_va_list() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "void() n; void(float a, ...) t = [1, n] { };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let generated = index.bodies[&t].generated();
    let stores: Vec<_> = generated
        .iter()
        .take_while(|s| s.op == Op::StoreV)
        .map(|s| (s.a, s.b))
        .collect();
    assert_eq!(stores.len(), 24);
    assert_eq!(
        stores.first(),
        Some(&(Some(Name::Parameter(1)), Some(Name::VaList(0))))
    );
    assert_eq!(
        stores.last(),
        Some(&(Some(Name::Parameter(24)), Some(Name::VaList(69))))
    );
    assert_eq!(generated[24].op, Op::State);
}

// FTEF-24.2, FTEF-26.2: a vector field's component reads the vector by `LOAD_V` and names its
// word; a global's component is the global's word; a field read once is not read again when
// `x + 0` folds to it
#[test]
fn a_generated_body_names_vector_components_and_reads_a_field_once() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "entity self; .vector fv; .float f; vector v; float g; \
        void() t = { g = (self.fv).y + 1; g = v.z * 2; g = self.f + 0; };";
    let index = super::fte_index_of(source);
    let def = |name: &str| {
        index
            .defs
            .iter()
            .position(|d| d.name == name)
            .map(DefId::new)
            .unwrap()
    };
    let statements: Vec<_> = index.bodies[&def("t")]
        .generated()
        .iter()
        .map(|s| (s.op, s.a))
        .collect();
    let field = Some(Name::Def(def("self"), 0));
    assert_eq!(
        statements,
        [
            (Op::LoadV, field),
            (Op::AddF, Some(Name::MadeWord(0, 1))),
            (Op::MulF, Some(Name::Def(def("v"), 2))),
            (Op::LoadF, field),
            (Op::Done, None),
        ]
    );
}

// FTE-18.7: a parameter after the arguments that takes its default is stored its default
// before the call, which counts it
#[test]
fn a_generated_call_stores_its_defaults() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float(float a, string b = \"x\") f; void() t = { f(1); };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let statements: Vec<_> = index.bodies[&t]
        .generated()
        .iter()
        .map(|s| (s.op, s.a, s.b))
        .collect();
    assert_eq!(
        statements[1],
        (Op::StoreF, Some(Name::Default(1)), Some(Name::Parameter(1)))
    );
    assert_eq!(statements[2].0, Op::Call2);
}

// FTEF-30.2: under `ifvector` a vector's test is `NOT_V` then the other jump: `if (v)`'s
// `IFNOT` an `IF` of `!v`, and `if (!v)`'s `IF` an `IFNOT` of it
#[test]
fn a_generated_vector_test_flips_its_jump() {
    use qc_preproc::Op;
    let source = "#pragma flag enable ifvector\nvector v; float k; \
        void() t = { if (v) k = 1; if (!v) k = 2; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let ops: Vec<_> = index.bodies[&t].generated().iter().map(|s| s.op).collect();
    assert_eq!(
        ops,
        [
            Op::NotV,
            Op::IfI,
            Op::StoreF,
            Op::NotV,
            Op::IfnotI,
            Op::StoreF,
            Op::Done
        ]
    );
}

// FTE-6.4, FTEF-29.5: a postfix on a field stores the stepped value through the field's
// `ADDRESS` by `STOREP_F`, as an assignment does
#[test]
fn a_generated_postfix_on_a_field_stores_through_its_address() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "entity self; .float f; void() t = { self.f--; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let statements: Vec<_> = index.bodies[&t]
        .generated()
        .iter()
        .map(|s| (s.op, s.a, s.b))
        .collect();
    let made = |at| Some(Name::Made(at));
    assert_eq!(statements[2].0, Op::Address);
    assert_eq!(statements[3], (Op::StorepF, made(1), made(2)));
}

// FTEF-30.3: `c ? a : b` tests `c` by an `IFNOT` to the last arm, past the first arm's store and
// the `GOTO` to the end; both arms store to the first arm's temporary, the ternary's value
#[test]
fn a_generated_body_names_a_ternary() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float a, b, k; void() t = { k = a ? b : a * b; };";
    let index = super::fte_index_of(source);
    let def = |name: &str| {
        index
            .defs
            .iter()
            .position(|d| d.name == name)
            .map(DefId::new)
            .unwrap()
    };
    let named = |name| Some(Name::Def(def(name), 0));
    let statements: Vec<_> = index.bodies[&def("t")]
        .generated()
        .iter()
        .map(|s| (s.op, s.a, s.b, s.c))
        .collect();
    let made = Some(Name::Made(1));
    assert_eq!(
        statements,
        [
            (Op::IfnotI, named("a"), Some(Name::Jump(3)), None),
            (Op::StoreF, named("b"), made, None),
            (Op::Goto, Some(Name::Jump(4)), None, None),
            (Op::MulF, named("a"), named("b"), made),
            (Op::StoreF, made, named("k"), None),
            (Op::Done, None, None, None),
        ]
    );
}

// QC-13.6, FTEF-29.13: the generated body names STATE's think function, a local's initialiser
// store the local, a string literal argument its bytes, and `random`'s call its builtin, its scaling
// `low + (high - low) * value` reading the return global
#[test]
fn a_generated_body_names_states_locals_strings_and_random() {
    use crate::ir::{Immediate, ImmediateKind, Name};
    use qc_preproc::Op;
    let float = |value: f32| Immediate {
        kind: ImmediateKind::Float,
        words: [value.to_bits(), 0, 0],
    };
    let source = "float() random; void(string s) print; void() n; \
        void() t = [1, n] { float l = 0; print(\"\\xff\"); l = random(2, 5); };";
    let index = super::fte_index_of(source);
    let def = |name: &str| {
        index
            .defs
            .iter()
            .position(|d| d.name == name)
            .map(DefId::new)
            .unwrap()
    };
    let body = &index.bodies[&def("t")];
    let generated = body.generated();
    let named = |name| Some(Name::Def(def(name), 0));
    let at = |op| generated.iter().position(|s| s.op == op).unwrap();
    let state = &generated[at(Op::State)];
    assert_eq!(state.b, named("n"));
    let local = &generated[at(Op::StoreF)];
    assert_eq!(local.b, named("l"));
    let string = &generated[at(Op::Call1).wrapping_sub(1)];
    let Some(Name::String(index)) = string.a else {
        panic!("the string is unnamed: {string:?}");
    };
    assert_eq!(body.string(index), Some(&b"\xff"[..]));
    assert_eq!(body.string(index.wrapping_add(9)), None);
    let call = at(Op::Call0);
    assert_eq!(generated[call].a, named("random"));
    let made = |offset: usize| {
        u32::try_from(call.wrapping_add(offset))
            .ok()
            .map(Name::Made)
    };
    let scaling: Vec<_> = generated[call.wrapping_add(1)..call.wrapping_add(4)]
        .iter()
        .map(|s| (s.op, s.a, s.c))
        .collect();
    assert_eq!(
        scaling,
        [
            (Op::SubF, Some(Name::Immediate(float(5.0))), made(1)),
            (Op::MulF, Some(Name::Return), made(2)),
            (Op::AddF, made(2), made(3)),
        ]
    );
}
