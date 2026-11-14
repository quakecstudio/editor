// SPDX-License-Identifier: GPL-3.0-or-later

//! Type tests (QC-10).

use std::rc::Rc;

use qc_syntax::ast;

use qc_syntax::{Dialect, QC_MAX_NESTING};

use crate::db::Db;
use crate::ir::{StatementIndex, StringIndex};
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

// spec QC-13.6, QC-13.7, QC-14.3, QC-14.4, QC-17.3, QC-17.7, QC-18.3, QC-18.4: a vanilla
// body is generated as `qcc` emits it: its `STATE` first, a field assignment's load turned to
// `ADDRESS` and stored through, a vector field's address keeping the two further words its
// load took, each argument stored with `STORE_V` as it is read, then `CALLn`, an `if`'s jumps,
// a `return` and the closing `DONE`
#[test]
fn a_vanilla_body_is_generated_as_qcc_emits_it() {
    use crate::ir::{Effect, Name};
    use qc_preproc::Op;
    let source = ".vector o; .float fl; entity e; float a; void(float x, vector y) f; \
        void() t = [1, t] { e.o = '1 2 3'; e.fl = a; a = e.fl + 2; f(a, e.o); \
        if (a) a = 1; else a = 2; return a; };";
    let index = super::index_of(source);
    let def = |name: &str| {
        index
            .defs
            .iter()
            .position(|d| d.name == name)
            .map(DefId::new)
            .unwrap()
    };
    let body = &index.bodies[&def("t")];
    assert!(body.followed());
    let generated = body.generated();
    assert_eq!(
        generated.iter().map(|s| s.op).collect::<Vec<_>>(),
        [
            Op::State,
            Op::Address,
            Op::StorepV,
            Op::Address,
            Op::StorepF,
            Op::LoadF,
            Op::AddF,
            Op::StoreF,
            Op::StoreV,
            Op::LoadV,
            Op::StoreV,
            Op::Call2,
            Op::IfnotI,
            Op::StoreF,
            Op::Goto,
            Op::StoreF,
            Op::Return,
            Op::Done,
        ]
    );
    let a = Some(Name::Def(def("a"), 0));
    assert_eq!(generated[0].b, Some(Name::Def(def("t"), 0)));
    assert_eq!(
        (generated[1].a, generated[1].b, generated[1].c),
        (
            Some(Name::Def(def("e"), 0)),
            Some(Name::Def(def("o"), 0)),
            Some(Name::Made(1))
        )
    );
    assert_eq!(generated[2].b, Some(Name::Made(1)));
    assert!(body.effects().contains(&(2, Effect::Temp(2))));
    assert_eq!((generated[4].a, generated[4].b), (a, Some(Name::Made(3))));
    assert_eq!(
        (generated[6].a, generated[7].a, generated[7].b),
        (Some(Name::Made(5)), Some(Name::Made(6)), a)
    );
    assert_eq!(
        (generated[8].a, generated[8].b),
        (a, Some(Name::Parameter(0)))
    );
    assert_eq!(
        (generated[10].a, generated[10].b),
        (Some(Name::Made(9)), Some(Name::Parameter(1)))
    );
    assert_eq!(generated[11].a, Some(Name::Def(def("f"), 0)));
    assert_eq!(
        (generated[12].a, generated[12].b),
        (a, Some(Name::Jump(15)))
    );
    assert_eq!(generated[14].a, Some(Name::Jump(16)));
    assert_eq!(generated[16].a, a);
}

// spec QC-27.11, QC-11.10, QC-18.6: a vanilla body leaves an earlier field load as it is
// before a plain name's assignment, initialises a local once as a global is, and reads a call's
// value from the return global
#[test]
fn a_vanilla_body_keeps_qcc_quirks_it_follows() {
    use crate::ir::{Effect, Name};
    let source = "entity e; .float fl; float a; float() g; \
        void() t = { e.fl; a = 2; };\
        void() u = { local float x = 1; a = x; };\
        void() w = { a = g() + g(); };";
    let index = super::index_of(source);
    let def = |name: &str| {
        index
            .defs
            .iter()
            .position(|d| d.name == name)
            .map(DefId::new)
            .unwrap()
    };
    let t = &index.bodies[&def("t")];
    assert!(t.followed());
    assert_eq!(
        t.generated().iter().map(|s| s.op).collect::<Vec<_>>(),
        [
            qc_preproc::Op::LoadF,
            qc_preproc::Op::StoreF,
            qc_preproc::Op::Done
        ]
    );
    let u = &index.bodies[&def("u")];
    assert!(u.followed());
    assert!(
        u.effects()
            .iter()
            .any(|(_, effect)| matches!(effect, Effect::Constant(_)))
    );
    let w = &index.bodies[&def("w")];
    assert!(w.followed());
    let add = w
        .generated()
        .iter()
        .find(|s| s.op == qc_preproc::Op::AddF)
        .unwrap();
    assert_eq!((add.a, add.b), (Some(Name::Return), Some(Name::Return)));
}

// spec FTE-3.1, FTE-3.8: a switch is generated as FTEQCC lowers it on the vanilla opcodes: a
// `GOTO` to the tests past the body, the body, a `GOTO` past the tests, then each case tested
// in order, a value by `EQ_F` and `IF`, a 0 by `IFNOT` of the value, a range by `GE_F`, `LE_F`
// and `AND_F`, and a `GOTO` to `default:`
#[test]
fn a_generated_body_holds_a_switch() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float a; void() t = { switch (a) { case 1: a = 2; break; case 0: a = 3; \
        case 4 .. 6: a = 5; default: a = 4; } };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let body = &index.bodies[&t];
    assert!(body.followed());
    let generated = body.generated();
    let jump = |at| Some(Name::Jump(at));
    assert_eq!(
        generated
            .iter()
            .map(|s| (
                s.op,
                s.a.filter(|a| matches!(a, Name::Jump(_))),
                s.b.filter(|b| matches!(b, Name::Jump(_)))
            ))
            .collect::<Vec<_>>(),
        [
            (Op::Goto, jump(7), None),
            (Op::StoreF, None, None),
            (Op::Goto, jump(15), None),
            (Op::StoreF, None, None),
            (Op::StoreF, None, None),
            (Op::StoreF, None, None),
            (Op::Goto, jump(15), None),
            (Op::EqF, None, None),
            (Op::IfI, None, jump(1)),
            (Op::IfnotI, None, jump(3)),
            (Op::GeF, None, None),
            (Op::LeF, None, None),
            (Op::AndF, None, None),
            (Op::IfI, None, jump(4)),
            (Op::Goto, jump(5), None),
            (Op::Done, None, None),
        ]
    );
    let a = Some(Name::Def(DefId::new(0), 0));
    assert_eq!((generated[8].a, generated[9].a), (Some(Name::Made(7)), a));
    assert_eq!(generated[13].a, Some(Name::Made(12)));
}

// spec FTE-6.4, FTE-18.13: a store to a field of a call's entity takes the field's `ADDRESS` of
// the return global after the value; when the value is another call, the entity is copied out
// of the return global first and the `ADDRESS` reads the copy
#[test]
fn a_generated_body_stores_through_a_calls_entity() {
    use crate::ir::{Immediate, ImmediateKind, Name};
    use qc_preproc::Op;
    let source = "entity() f; float() g; .float fl; void() t = { f().fl = 2; f().fl = g(); };";
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
    assert!(body.followed());
    let fl = Some(Name::Def(def("fl"), 0));
    assert_eq!(
        body.generated()
            .iter()
            .map(|s| (s.op, s.a, s.b))
            .collect::<Vec<_>>(),
        [
            (Op::Call0, Some(Name::Def(def("f"), 0)), None),
            (Op::Address, Some(Name::Return), fl),
            (
                Op::StorepF,
                Some(Name::Immediate(Immediate {
                    kind: ImmediateKind::Float,
                    words: [2.0_f32.to_bits(), 0, 0],
                })),
                Some(Name::Made(1))
            ),
            (Op::Call0, Some(Name::Def(def("f"), 0)), None),
            (Op::StoreF, Some(Name::Return), Some(Name::Made(4))),
            (Op::Call0, Some(Name::Def(def("g"), 0)), None),
            (Op::Address, Some(Name::Made(4)), fl),
            (Op::StorepF, Some(Name::Return), Some(Name::Made(6))),
            (Op::Done, None, None),
        ]
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

// FTEF-6, FTEF-19.78: a member of a struct array's element at an index no constant is read
// at the element's index plus the member's word, by `OP_ADD_F` of a float index
#[test]
fn a_generated_struct_element_member_adds_its_word_to_the_index() {
    use crate::ir::{Immediate, ImmediateKind, Name};
    use qc_preproc::Op;
    let source = "struct { float a; float b; } arr[4]; float i, k; void() t = { k = arr[i].b; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let generated = index.bodies[&t].generated();
    let one = Some(Name::Immediate(Immediate {
        kind: ImmediateKind::Float,
        words: [1.0_f32.to_bits(), 0, 0],
    }));
    let add = generated.iter().position(|s| s.op == Op::AddF).unwrap();
    assert_eq!(generated[add].b, one);
    assert_eq!(generated[add.wrapping_add(1)].op, Op::Call1);
}

// spec QC-27.19: each generated statement is recorded at the source statement the compiler was
// reading, the body's `DONE` at the last one, and a state header before any at none (the
// function's name)
#[test]
fn a_generated_body_records_each_statements_origin() {
    let source = "float k; void() n; void() t = [1, n] { k = 1;\n  k = k * 2; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let body = &index.bodies[&t];
    let first = source.find("k = 1").unwrap();
    let second = source.find("k = k * 2").unwrap();
    let origins: Vec<_> = (0_u32..)
        .take(body.generated().len())
        .map(|i| body.origin(StatementIndex::new(i)))
        .collect();
    assert_eq!(origins, [None, Some(first), Some(second), Some(second)]);
}

// FTE-18.10: a bare `return` of a function that returns a value returns the `ret*` local,
// which the body returns once more before its `DONE`; the body's locals are listed where they
// are declared, and each statement says which operands it uses and how many words it makes
#[test]
fn a_generated_body_lists_its_locals_and_returns_its_ret_local() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source =
        "float k; float() t = { local float a; a = k * 2; if (a) return; local vector v; };";
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
    let ops: Vec<_> = generated.iter().map(|s| (s.op, s.a)).collect();
    let ret = Some(Name::RetLocal);
    assert_eq!(
        &ops[2..],
        [(Op::Return, ret), (Op::Return, ret), (Op::Done, None)]
    );
    let locals: Vec<_> = body.locals().iter().map(|&(at, _)| at).collect();
    assert_eq!(locals, [0, 3]);
    assert_eq!(generated[0].uses(), [true, true, true]);
    assert_eq!(
        generated[0].made_words(StatementIndex::new(0)),
        None,
        "the product folds into a"
    );
    assert_eq!(generated[1].uses(), [true, true, false]);
    assert_eq!(generated[2].uses(), [false; 3]);
}

// FTEF-34.16, FTEF-30.3: a ternary's test of `!x` drops the `NOT` and jumps by `IF`, as a
// statement's test does
#[test]
fn a_generated_ternary_test_drops_a_not() {
    use qc_preproc::Op;
    let source = "float a, b, k; void() t = { k = (!a) ? b : 2; };";
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
            Op::IfI,
            Op::StoreF,
            Op::Goto,
            Op::StoreF,
            Op::StoreF,
            Op::Done
        ]
    );
}

// FTEF-24.2, FTE-18.13: a component of a call's vector is a word of the return global, and of
// the copy a jump makes of it once the jump has copied it
#[test]
fn a_generated_body_names_a_word_of_a_calls_vector() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "vector() f; float k; void() t = { k = (f()).y * 2; if ((f()).z) k = 1; };";
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
        .map(|s| (s.op, s.a))
        .collect();
    assert_eq!(statements[1], (Op::MulF, Some(Name::ReturnWord(1))));
    assert_eq!(statements[3], (Op::StoreV, Some(Name::Return)));
    assert_eq!(statements[4], (Op::IfnotI, Some(Name::MadeWord(3, 2))));
}

// FTE-18.13: an assignment of a call's value whose value is read is still the call's value
// in the return global, which a jump copies before it tests the copy
#[test]
fn a_generated_assignment_of_a_call_stays_its_value() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "string() f; string s; void() t = { while ((s = f())) s = \"\"; };";
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
        statements[2],
        (Op::StoreF, Some(Name::Return), Some(Name::Made(2)))
    );
    assert_eq!(statements[3].0, Op::IfnotI);
    assert_eq!(statements[3].1, Some(Name::Made(2)));
}

// FTEF-26.12: an operand the model knows only as opaque decides no folding, and its statement
// is generated: a store of a global array's element at a constant index
#[test]
fn a_generated_store_of_an_opaque_value_is_a_statement() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "string face; string fb[2] = {\"a\", \"b\"}; void() t = { face = fb[1]; };";
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
    assert_eq!(generated[0].op, Op::StoreS);
    assert_eq!(generated[0].a, Some(Name::Def(def("fb"), 1)));
}

// FTEF-31.4, FTEF-31.3: `&e.f` is `ADDRESS` of the entity and the field, into which a store
// to a pointer folds, and `*p = v` is `STOREP_*` of the value through the pointer
#[test]
fn a_generated_body_takes_a_fields_address_and_stores_through_it() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "entity world; .float sounds; float *cd; float k; \
        void() t = { cd = &world.sounds; *cd = k; };";
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
        .map(|s| (s.op, s.a, s.b, s.c))
        .collect();
    let named = |name| Some(Name::Def(def(name), 0));
    assert_eq!(
        statements[..2],
        [
            (Op::Address, named("world"), named("sounds"), named("cd")),
            (Op::StorepF, named("k"), named("cd"), None),
        ]
    );
}

// FTEF-29.13: a compound assignment's and a postfix's store to a definition, a constant-index
// element's too, folds into the statement that made the value
#[test]
fn a_generated_update_folds_into_its_definition() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float k; float arr[3]; void() t = { k += 2; k++; arr[1]--; };";
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
        .map(|s| (s.op, s.c))
        .collect();
    let k = Some(Name::Def(def("k"), 0));
    assert_eq!(
        statements,
        [
            (Op::AddF, k),
            (Op::AddF, k),
            (Op::AddF, Some(Name::Def(def("arr"), 1))),
            (Op::Done, None),
        ]
    );
}

// FTEF-34.4, FTEF-29.13: a local a `for`'s initialiser clause declares is stored its value
// before the loop's test
#[test]
fn a_generated_for_stores_the_local_its_clause_declares() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float k; void() t = { for (float i = 0; i < k; i++) k = 1; };";
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
    let first = &body.generated()[0];
    assert_eq!(first.op, Op::StoreF);
    let i = body.locals().first().map(|&(_, id)| Name::Def(id, 0));
    assert_eq!(first.b, i);
    assert_eq!(body.generated()[1].op, Op::LtF);
}

// FTE-13.1, FTEF-31.8: a vector local's aggregate stores each element to its component, and a
// `.vector` field at a constant index is its component's field, read by `LOAD_F`
#[test]
fn a_generated_aggregate_stores_each_component() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "entity self; .vector md; void() t = { vector v = { self.md[0], 2, 3 }; };";
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
    let v = body.locals().first().map(|&(_, id)| id).unwrap();
    assert_eq!(generated[0].op, Op::LoadF);
    assert_eq!(generated[0].b, Some(Name::Def(def("md_x"), 0)));
    let stores: Vec<_> = generated[1..4].iter().map(|s| (s.op, s.b)).collect();
    assert_eq!(
        stores,
        [
            (Op::StoreF, Some(Name::Def(v, 0))),
            (Op::StoreF, Some(Name::Def(v, 1))),
            (Op::StoreF, Some(Name::Def(v, 2))),
        ]
    );
}

// FTE-19.8, FTEF-27.4: an int converted to a float by the program's itof is the helper's
// call of the value, which the assignment then stores from the return global
#[test]
fn a_generated_conversion_calls_its_helper() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float(__int i) itof = #0; float b; void(__int x) t = { b = x; };";
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
        .map(|s| (s.op, s.a, s.b))
        .collect();
    assert_eq!(
        statements[..3],
        [
            (
                Op::StoreF,
                Some(Name::Def(def("x"), 0)),
                Some(Name::Parameter(0))
            ),
            (Op::Call1, Some(Name::Def(def("itof"), 0)), None),
            (Op::StoreF, Some(Name::Return), Some(Name::Def(def("b"), 0))),
        ]
    );
}

// FTEF-15.11: a `__wrap` body's `prior` is the function it wraps, called with the wrapper's
// own type
#[test]
fn a_generated_wrap_calls_its_prior() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "void(string m) objerror = #11; \
        __wrap void(string msg) objerror = { prior(msg); };";
    let index = super::fte_index_of(source);
    let objerror = index
        .defs
        .iter()
        .position(|d| d.name == "objerror")
        .map(DefId::new)
        .unwrap();
    let statements: Vec<_> = index.bodies[&objerror]
        .generated()
        .iter()
        .map(|s| (s.op, s.a))
        .collect();
    assert_eq!(statements[1], (Op::Call1, Some(Name::Prior)));
}

// The checker says whether it followed every statement a body generates: not a body with a
// builtin or nested function local, or an aggregate other than a vector's
#[test]
fn a_generated_body_says_whether_it_followed_every_statement() {
    let body = |source: &str| {
        let index = super::fte_index_of(source);
        let t = index
            .defs
            .iter()
            .position(|d| d.name == "t")
            .map(DefId::new)
            .unwrap();
        index.bodies[&t].followed()
    };
    assert!(body("float k; void() t = { k = random(); };"));
    assert!(!body("void() t = { local float(float x) g = #1; };"));
    assert!(body("void() t = { local void() f = { }; };"));
    assert!(body("void() t = { float a[2] = { 1, 2 }; };"));
    assert!(body("void() t = { const float a[2] = { 1, 2 }; };"));
    assert!(body("void() t = { vector v = { 1, 2, 3 }; };"));
}

// spec FTE-15.8: an integer constant initialised by a number holds the integer, so read as a
// float it folds to the number, not to its float's bits read as an integer
#[test]
fn an_integer_constant_folds_to_its_number() {
    use crate::ir::{Immediate, ImmediateKind, Name};
    let source = "const __uint32 U = 3; const int I = -2; float g; void() t = { g = U; g = I; };";
    let index = super::fte_index_of(source);
    let t = index
        .defs
        .iter()
        .position(|d| d.name == "t")
        .map(DefId::new)
        .unwrap();
    let float = |value: f32| {
        Some(Name::Immediate(Immediate {
            kind: ImmediateKind::Float,
            words: [value.to_bits(), 0, 0],
        }))
    };
    let stored: Vec<_> = index.bodies[&t].generated()[..2]
        .iter()
        .map(|s| s.a)
        .collect();
    assert_eq!(stored, [float(3.0), float(-2.0)]);
}

// FTE-19.4, FTEF-29.13: `++x` folds its store into the step; a number literal such as `0i` is
// named by the immediate the lexer reads it as, a float
#[test]
fn a_generated_prefix_folds_and_a_literal_names_its_float() {
    use crate::ir::{Immediate, ImmediateKind, Name};
    use qc_preproc::Op;
    let source = "float a; var void() g; void() t = { ++a; g = 0i; };";
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
    assert_eq!(
        (generated[0].op, generated[0].c),
        (Op::AddF, Some(Name::Def(def("a"), 0)))
    );
    let zero = Name::Immediate(Immediate {
        kind: ImmediateKind::Float,
        words: [0, 0, 0],
    });
    assert_eq!(generated[1].a, Some(zero));
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
// `low + (high - low) * value` reading the return global, the store folding into its last step
#[test]
fn a_generated_body_names_states_locals_strings_and_random() {
    use crate::ir::Name;
    use qc_preproc::Op;
    let source = "float() random; void(string s) print; void() n; float h; \
        void() t = [1, n] { float l = 0; print(\"\\xff\"); l = random(2, h); };";
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
    assert_eq!(
        body.string(StringIndex::new(index.get().wrapping_add(9))),
        None
    );
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
            (Op::SubF, named("h"), made(1)),
            (Op::MulF, Some(Name::Return), made(2)),
            (Op::AddF, made(2), named("l")),
        ]
    );
}
