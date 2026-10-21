// SPDX-License-Identifier: GPL-3.0-or-later

//! Expressions, operators, assignment, calls and conditions: QC-15 to QC-19 and the expression
//! errors QCD-4.2 to QCD-4.7.

use qc_diag::Code;
use qc_progs::opcode::Opcode::{
    self, AddF, AddV, Address, And, BitAnd, BitOr, Call0, Call1, Call2, Call3, Call4, Call5, Call6,
    Call7, Call8, DivF, Done, EqE, EqF, EqFnc, EqS, EqV, Ge, Gt, If, IfNot, Le, LoadEnt, LoadF,
    LoadFnc, Lt, MulF, MulFv, MulV, MulVf, NeE, NeF, NeFnc, NeS, NeV, NotEnt, NotF, NotFnc, NotS,
    NotV, Or, Return, StoreF, StoreFld, StoreV, StorepF, StorepV, SubF, SubV,
};

use super::{
    body, compile_source, errors, global, immediate, immediate_string, immediate_vector, progs, st,
    words,
};
use crate::CompileError;

/// The opcodes of `f`'s body, `DONE` excluded.
fn opcodes(source: &str) -> Vec<Opcode> {
    let p = progs(source);
    body(&p, "f")
        .iter()
        .filter_map(|s| Opcode::from_number(s.op))
        .filter(|op| *op != Done)
        .collect()
}

/// The code of the first error of a program that must not compile.
fn first_code(source: &str) -> Code {
    match compile_source(source) {
        Err(CompileError::Diagnostics(d)) => d[0].diagnostic.code,
        other => panic!("{source:?}: {other:?}"),
    }
}

// spec QC-15.9, QC-21.5: operands left to right, each operator's statement after both
// operands', every result in a new temporary.
#[test]
fn operands_come_before_their_operator() {
    let p = progs("float a, b, c;\nvoid() f = { a = b + c * a - b / c; };\n");
    let (a, b, c) = (global(&p, "a"), global(&p, "b"), global(&p, "c"));
    // The first temporary is after `f`'s own global.
    let t = global(&p, "f") + 1;
    assert_eq!(
        body(&p, "f"),
        [
            st(MulF, c, a, t),
            st(AddF, b, t, t + 1),
            st(DivF, b, c, t + 2),
            st(SubF, t + 1, t + 2, t + 3),
            st(StoreF, t + 3, a, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec QC-15.10: `&&` and `||` evaluate both operands, with no branch.
#[test]
fn and_and_or_evaluate_both_sides() {
    assert_eq!(
        opcodes("float a, b, c;\nvoid() f = { a = b && c; a = b || c; };\n"),
        [StoreF, And, StoreF, Or]
    );
}

// spec QC-16.1: each operator's opcode by its operands' types.
#[test]
fn the_operator_table_picks_by_operand_types() {
    assert_eq!(
        opcodes("float a;\nvector v;\nvoid() f = { a = v * v; v = v * a; v = a * v; };\n"),
        [MulV, StoreF, MulVf, StoreV, MulFv, StoreV]
    );
    assert_eq!(
        opcodes(
            "vector v;\nfloat a;\nvoid() f = { v = v + v; v = v - v; a = a - a; a = a / a; };\n"
        ),
        [AddV, StoreV, SubV, StoreV, SubF, StoreF, DivF, StoreF]
    );
    assert_eq!(
        opcodes(
            "float a;\nstring s;\nentity e, w;\nvector v;\nvoid() g;\nvoid() f = { a = s == s; \
             a = e == w; a = v == v; a = g == g; a = a == a; a = s != s; a = e != w; a = v != v; \
             a = g != g; a = a != a; };\nvoid() g = {};\n"
        ),
        [
            EqS, StoreF, EqE, StoreF, EqV, StoreF, EqFnc, StoreF, EqF, StoreF, NeS, StoreF, NeE,
            StoreF, NeV, StoreF, NeFnc, StoreF, NeF, StoreF,
        ]
    );
    assert_eq!(
        opcodes(
            "float a, b;\nvoid() f = { a = a <= b; a = a >= b; a = a < b; a = a > b; a = a & b; \
             a = a | b; };\n"
        ),
        [
            Le, StoreF, Ge, StoreF, Lt, StoreF, Gt, StoreF, BitAnd, StoreF, BitOr, StoreF
        ]
    );
}

// spec QC-16.1, QC-16.6, QC-15.8: `e.f` loads the field by its type, `LOAD_*` into a temporary
// of the field's type; a chain loads each in turn.
#[test]
fn a_field_access_loads_by_the_fields_type() {
    let p = progs(".entity en;\n.float hp;\nentity e;\nfloat a;\nvoid() f = { a = e.en.hp; };\n");
    let (en, hp, e, a) = (
        global(&p, "en"),
        global(&p, "hp"),
        global(&p, "e"),
        global(&p, "a"),
    );
    let t = global(&p, "f") + 1;
    assert_eq!(
        body(&p, "f"),
        [
            st(LoadEnt, e, en, t),
            st(LoadF, t, hp, t + 1),
            st(StoreF, t + 1, a, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec QC-16.2, QCD-4.2: no `vector / float`, no ordering of vectors, no equality of fields, no
// `&&` of an entity: "type mismatch for OP", E0302.
#[test]
fn operands_of_no_row_are_a_type_mismatch() {
    assert_eq!(
        errors("vector v;\nvoid() f = { v = v / 2; };\n"),
        ["type mismatch for /"]
    );
    assert_eq!(
        errors("float a;\nvector v;\nvoid() f = { a = v < v; };\n"),
        ["type mismatch for <"]
    );
    assert_eq!(
        errors(".float fl;\nfloat a;\nvoid() f = { a = fl == fl; };\n"),
        ["type mismatch for =="]
    );
    assert_eq!(
        errors("entity e;\nfloat a;\nvoid() f = { if (e && a) a = 1; };\n"),
        ["type mismatch for &&"]
    );
    assert_eq!(
        first_code("vector v;\nvoid() f = { v = v / 2; };\n"),
        Code::E0302
    );
}

// spec QC-16.1, QCD-4.2: a field access needs an entity and a field.
#[test]
fn a_field_access_needs_an_entity_and_a_field() {
    assert_eq!(
        errors("float a;\nentity e;\nvoid() f = { a = e.a; };\n"),
        ["type mismatch for ."]
    );
    assert_eq!(
        errors(".float fl;\nfloat a;\nvoid() f = { a = a.fl; };\n"),
        ["type mismatch for ."]
    );
}

// spec QC-16.4, QC-16.5: `!` emits `NOT_*` by its operand's type, a float result; a field is a
// type mismatch.
#[test]
fn not_by_the_operands_type() {
    assert_eq!(
        opcodes(
            "float a;\nstring s;\nentity e;\nvector v;\nvoid() g;\nvoid() f = { a = !a; a = !s; \
             a = !e; a = !v; a = !g; };\nvoid() g = {};\n"
        ),
        [
            NotF, StoreF, NotS, StoreF, NotEnt, StoreF, NotV, StoreF, NotFnc, StoreF
        ]
    );
    assert_eq!(
        errors(".float fl;\nfloat a;\nvoid() f = { a = !fl; };\n"),
        ["type mismatch for !"]
    );
}

// spec QC-16.7: an operator compares only its operands' basic types, so two functions of
// different types are compared with `EQ_FNC`.
#[test]
fn functions_of_two_types_compare() {
    let p = progs(
        "void() g;\nfloat(float x) h;\nfloat r;\nvoid() f = { r = g == h; };\nvoid() g = {};\n\
         float(float x) h = { return x; };\n",
    );
    let (g, h) = (global(&p, "g"), global(&p, "h"));
    assert_eq!(body(&p, "f")[0], st(EqFnc, g, h, global(&p, "f") + 1));
}

// spec QC-17.6, QC-16.7: `x = e` stores by the two basic types, so a field of another type may
// be stored; the value is `e`, so `a = b = c` stores `c` twice.
#[test]
fn assignment_stores_by_basic_type() {
    let p = progs(".float fa;\n.vector fb;\nvoid() f = { fa = fb; };\n");
    let (fa, fb) = (global(&p, "fa"), global(&p, "fb"));
    assert_eq!(body(&p, "f")[0], st(StoreFld, fb, fa, 0));
    let p = progs("float a, b, c;\nvoid() f = { a = b = c; };\n");
    let (a, b, c) = (global(&p, "a"), global(&p, "b"), global(&p, "c"));
    assert_eq!(
        body(&p, "f"),
        [st(StoreF, c, b, 0), st(StoreF, c, a, 0), st(Done, 0, 0, 0)]
    );
    assert_eq!(
        errors("float a;\nvoid() f = { a = \"x\"; };\n"),
        ["type mismatch for ="]
    );
}

// spec QC-17.3, QC-21.5: a field access on the left of `=` becomes `ADDRESS`, keeping the
// temporary its load had, and the store is `STOREP_*`; in a chain only the last access is an
// address.
#[test]
fn a_field_on_the_left_is_stored_through_its_address() {
    let p = progs(
        ".vector o;\n.entity en;\n.float hp;\nentity e;\nvector v;\n\
         void() f = { e.o_x = 1; e.o = v; e.en.hp = 3; (e.hp) = 4; };\n",
    );
    let (o, o_x, e, v) = (
        global(&p, "o"),
        global(&p, "o_x"),
        global(&p, "e"),
        global(&p, "v"),
    );
    let (en, hp) = (global(&p, "en"), global(&p, "hp"));
    // The address's temporary comes before the value's literal.
    let t = global(&p, "f") + 1;
    let one = immediate(&p, 1.0);
    assert_eq!(one, t + 1);
    let s = body(&p, "f");
    assert_eq!(s[0], st(Address, e, o_x, t));
    assert_eq!(s[1], st(StorepF, one, t, 0));
    assert_eq!(s[2], st(Address, e, o, t + 2));
    assert_eq!(s[3], st(StorepV, v, t + 2, 0));
    // The vector load's temporary takes three words.
    assert_eq!(s[4], st(LoadEnt, e, en, t + 5));
    assert_eq!(s[5], st(Address, t + 5, hp, t + 6));
    assert_eq!(s[7].op, u32::from(Address.number()));
}

// spec QC-17.7: through a field the value must have the field's basic type.
#[test]
fn a_store_through_a_field_checks_the_fields_type() {
    assert_eq!(
        errors(".vector fv;\nentity e;\nvoid() f = { e.fv = 1; };\n"),
        ["type mismatch for ="]
    );
}

// spec QC-17.2, QC-27.10: the left side of `=` must be a name or a field access, where id's
// qcc stores into a literal's global.
#[test]
fn a_literal_is_not_assigned() {
    assert_eq!(
        first_code("float a;\nvoid() f = { 5 = a; };\n"),
        Code::E0508
    );
}

// spec QC-17.4, QC-27.11: an earlier statement's field load is never made an address.
#[test]
fn an_earlier_load_is_not_rewritten() {
    let p = progs(".float fl;\nentity e;\nfloat a;\nvoid() f = { e.fl; a = 2; };\n");
    let (fl, e, a) = (global(&p, "fl"), global(&p, "e"), global(&p, "a"));
    let two = immediate(&p, 2.0);
    let s = body(&p, "f");
    assert_eq!(s[0], st(LoadF, e, fl, global(&p, "f") + 1));
    assert_eq!(s[1], st(StoreF, two, a, 0));
}

// spec QC-18.3, QC-18.5: each argument is evaluated and copied with `STORE_V` to slot
// `4 + 3i`, then `CALLn`; a call in a later argument overwrites the earlier slots.
#[test]
fn arguments_are_copied_to_the_shared_slots() {
    let p = progs(
        "float(float p, float q) g = #1;\nfloat(float p) h = #2;\nfloat a;\n\
         void() f = { a = g(4, h(1)); };\n",
    );
    let (g, h, a) = (global(&p, "g"), global(&p, "h"), global(&p, "a"));
    let (four, one) = (immediate(&p, 4.0), immediate(&p, 1.0));
    assert_eq!(
        body(&p, "f"),
        [
            st(StoreV, four, 4, 0),
            st(StoreV, one, 4, 0),
            st(Call1, h, 0, 0),
            st(StoreV, 1, 7, 0),
            st(Call2, g, 0, 0),
            st(StoreF, 1, a, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec QC-18.4, QC-18.6: the value of a call is global 1, so two calls in one expression add
// the same global.
#[test]
fn calls_return_through_one_global() {
    let p = progs("float() g = #1;\nfloat() h = #2;\nfloat a;\nvoid() f = { a = g() + h(); };\n");
    let (g, h, a) = (global(&p, "g"), global(&p, "h"), global(&p, "a"));
    assert_eq!(
        body(&p, "f"),
        [
            st(Call0, g, 0, 0),
            st(Call0, h, 0, 0),
            st(AddF, 1, 1, a + 2),
            st(StoreF, a + 2, a, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec QC-18.9: the return global has the type of the last call when an operator reads it, so
// a float call's value added to a vector call's is a vector sum.
#[test]
fn the_return_global_has_the_last_calls_type() {
    let p = progs(
        "float() g = #1;\nvector() h = #2;\nvector v;\nfloat a;\n\
         void() f = { v = g() + h(); a = h() * g(); };\n",
    );
    assert_eq!(
        body(&p, "f")
            .iter()
            .filter_map(|s| Opcode::from_number(s.op))
            .collect::<Vec<_>>(),
        [Call0, Call0, AddV, StoreV, Call0, Call0, MulF, StoreF, Done]
    );
}

// spec QC-18.2, QC-18.3, QCD-4.7: a variadic function takes up to eight arguments of any types;
// nine is "More than eight parameters", E0307.
#[test]
fn a_variadic_call_takes_up_to_eight_arguments() {
    let p = progs("void(...) g = #1;\nvoid() f = { g(1, \"x\", '1 2 3'); g(1,2,3,4,5,6,7,8); };\n");
    let g = global(&p, "g");
    let s = body(&p, "f");
    assert_eq!(s[0], st(StoreV, immediate(&p, 1.0), 4, 0));
    assert_eq!(s[1], st(StoreV, immediate_string(&p, "x"), 7, 0));
    assert_eq!(
        s[2],
        st(StoreV, immediate_vector(&p, [1.0, 2.0, 3.0]), 10, 0)
    );
    assert_eq!(s[3], st(Call3, g, 0, 0));
    assert_eq!(s[12], st(Call8, g, 0, 0));
    assert_eq!(
        errors("void(...) g = #1;\nvoid() f = { g(1,2,3,4,5,6,7,8,9); };\n"),
        ["More than eight parameters"]
    );
    assert_eq!(
        first_code("void(...) g = #1;\nvoid() f = { g(1,2,3,4,5,6,7,8,9); };\n"),
        Code::E0307
    );
}

// spec QC-18.3: a call of `n` arguments is `CALLn`, for each `n` from 0 to 8.
#[test]
fn each_argument_count_has_its_call() {
    let mut source = String::from("void(...) g = #1;\nvoid() f = {\n");
    for n in 0..=8 {
        let args: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        source.push_str(&format!("g({});\n", args.join(", ")));
    }
    source.push_str("};\n");
    let calls: Vec<Opcode> = opcodes(&source)
        .into_iter()
        .filter(|op| *op != StoreV)
        .collect();
    assert_eq!(
        calls,
        [
            Call0, Call1, Call2, Call3, Call4, Call5, Call6, Call7, Call8
        ]
    );
}

// spec QC-16.7, QCD-4.2: a field access's error is at its `.`.
#[test]
fn a_field_access_error_is_at_the_dot() {
    let source = "float a;\nentity e;\nvoid() f = { a = e.a; };\n";
    let Err(CompileError::Diagnostics(d)) = compile_source(source) else {
        panic!("compiled");
    };
    let span = d[0].diagnostic.span;
    assert_eq!(&source[span.start..span.end], ".");
}

// spec QC-18.8, QCD-4.3, QCD-4.4, QCD-4.5, QCD-4.6: the callee must be a function; a fixed
// list takes no more arguments than parameters, and with any argument no fewer, each of the
// parameter's type.
#[test]
fn a_call_checks_its_arguments() {
    let cases = [
        (
            "void(float a) g = #1;\nvoid() f = { g(1, 2); };\n",
            Code::E0304,
            "too many parameters",
        ),
        (
            "void(float a, float b) g = #1;\nvoid() f = { g(1); };\n",
            Code::E0305,
            "too few parameters",
        ),
        (
            "void(float a) g = #1;\nvoid() f = { g(\"x\"); };\n",
            Code::E0306,
            "type mismatch on parm 0",
        ),
        (
            "void(float a, float b) g = #1;\nvoid() f = { g(1, \"x\"); };\n",
            Code::E0306,
            "type mismatch on parm 1",
        ),
        (
            "float a;\nvoid() f = { a(); };\n",
            Code::E0303,
            "not a function",
        ),
    ];
    for (source, code, message) in cases {
        assert_eq!(errors(source), [message], "{source}");
        assert_eq!(first_code(source), code, "{source}");
    }
}

// spec QC-18.8: a call with no arguments is not checked against a fixed list, as id 1.01's
// `droptofloor()` calls a function of two parameters.
#[test]
fn a_call_with_no_arguments_is_not_counted() {
    let p = progs("float(float a, float b) g = #2;\nvoid() f = { g(); };\n");
    assert_eq!(body(&p, "f")[0], st(Call0, global(&p, "g"), 0, 0));
}

// spec QC-18.8, QC-11.12: an argument's type must be the parameter's exactly, and a vector
// field's component is no `.float`.
#[test]
fn a_field_component_is_no_float_field_argument() {
    assert_eq!(
        errors(".vector fv;\nvoid(.float x) g = #1;\nvoid() f = { g(fv_x); };\n"),
        ["type mismatch on parm 0"]
    );
}

// spec QC-18.7: a call through a field loads the function, then calls the loaded value.
#[test]
fn a_call_through_a_field() {
    let p = progs(".void() th;\nentity e;\nvoid() f = { e.th(); };\n");
    let (th, e) = (global(&p, "th"), global(&p, "e"));
    assert_eq!(
        body(&p, "f"),
        [
            st(LoadFnc, e, th, e + 2),
            st(Call0, e + 2, 0, 0),
            st(Done, 0, 0, 0)
        ]
    );
}

// spec QC-19.1: no implicit conversions: a string is no float.
#[test]
fn there_are_no_conversions() {
    assert_eq!(
        errors("float a;\nvoid() f = { a = \"1\"; };\n"),
        ["type mismatch for ="]
    );
    assert_eq!(
        errors("void(float a) g = #1;\nvoid() f = { g('1 0 0'); };\n"),
        ["type mismatch on parm 0"]
    );
}

// spec QC-19.2, QC-19.3: a condition of any type tests its first word; a vector's is its x.
#[test]
fn a_condition_of_any_type_tests_its_first_word() {
    let p = progs(
        "string s;\nentity e;\nvoid() g;\nvector v;\nfloat a;\n\
         void() f = { if (s) a = 1; while (e) a = 2; do a = 3; while (g); if (v) a = 4; };\n\
         void() g = {};\n",
    );
    let s = body(&p, "f");
    assert_eq!(s[0], st(IfNot, global(&p, "s"), 2, 0));
    assert_eq!(s[2], st(IfNot, global(&p, "e"), 3, 0));
    assert_eq!(s[6], st(If, global(&p, "g"), -1, 0));
    assert_eq!(s[7], st(IfNot, global(&p, "v"), 2, 0));
    assert_eq!(global(&p, "v"), global(&p, "v_x"));
}

// spec QC-19.4, QC-16.5: an empty string literal is a global holding the offset of an empty
// string, not the null string 0.
#[test]
fn an_empty_string_literal_is_not_null() {
    let p = progs("float a;\nvoid() f = { if (\"\") a = 1; a = !\"\"; };\n");
    let empty = immediate_string(&p, "");
    assert_ne!(words(&p, empty, 1), [0]);
    assert_eq!(body(&p, "f")[0], st(IfNot, empty, 2, 0));
    assert_eq!(body(&p, "f")[2], st(NotS, empty, 0, immediate(&p, 1.0) + 1));
}

// spec QC-19.5, QC-13.8: a return value is not checked against the return type.
#[test]
fn a_return_value_is_not_checked() {
    let p = progs("float() f = { return \"x\"; };\nfloat() g = { return; };\n");
    assert_eq!(
        body(&p, "f")[0],
        st(Return, immediate_string(&p, "x"), 0, 0)
    );
    assert_eq!(body(&p, "g")[0], st(Return, 0, 0, 0));
}
