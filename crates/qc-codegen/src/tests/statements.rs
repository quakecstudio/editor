// SPDX-License-Identifier: GPL-3.0-or-later

//! Statements and function bodies: QC-13.1, QC-13.6, QC-13.7, QC-14 and QC-22.

use qc_progs::opcode::Opcode::{AddF, Done, Goto, If, IfNot, Lt, Return, State, StoreF, SubF};

use super::{body, global, immediate, progs, st};

// spec QC-14.4, QC-22.1, QC-22.2: `if (e) s1 else s2` is `IFNOT e` to s2, s1, `GOTO` past s2;
// IFNOT's branch is operand b, GOTO's operand a, each relative to the branch statement.
#[test]
fn if_else_branches_past_each_arm() {
    let p = progs("float a, b;\nvoid() f = { if (a) b = 1; else b = 2; };\n");
    let (a, b) = (global(&p, "a"), global(&p, "b"));
    let (one, two) = (immediate(&p, 1.0), immediate(&p, 2.0));
    assert_eq!(
        body(&p, "f"),
        [
            st(IfNot, a, 3, 0),
            st(StoreF, one, b, 0),
            st(Goto, 2, 0, 0),
            st(StoreF, two, b, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec QC-14.4: `else` binds to the nearest `if`; an `if` with no `else` branches past its
// statement.
#[test]
fn else_binds_to_the_nearest_if() {
    let p = progs("float a, b, c;\nvoid() f = { if (a) if (b) c = 1; else c = 2; };\n");
    let (a, b, c) = (global(&p, "a"), global(&p, "b"), global(&p, "c"));
    let (one, two) = (immediate(&p, 1.0), immediate(&p, 2.0));
    assert_eq!(
        body(&p, "f"),
        [
            st(IfNot, a, 5, 0),
            st(IfNot, b, 3, 0),
            st(StoreF, one, c, 0),
            st(Goto, 2, 0, 0),
            st(StoreF, two, c, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec QC-14.5, QC-22.1: `while (e) s` is the condition, `IFNOT` past the loop, the body, and
// a `GOTO` back to the condition's first statement.
#[test]
fn while_tests_first_and_jumps_back() {
    let p = progs("float a;\nvoid() f = { while (a) a = a - 1; };\n");
    let a = global(&p, "a");
    let one = immediate(&p, 1.0);
    let t = one + 1;
    assert_eq!(
        body(&p, "f"),
        [
            st(IfNot, a, 4, 0),
            st(SubF, a, one, t),
            st(StoreF, t, a, 0),
            st(Goto, -3, 0, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec QC-14.6, QC-22.1: `do s while (e);` is the body, the condition, then `IF` back to the
// body's first statement.
#[test]
fn do_while_tests_last_and_jumps_back() {
    let p = progs("float a;\nvoid() f = { do a = a + 1; while (a < 3); };\n");
    let a = global(&p, "a");
    let one = immediate(&p, 1.0);
    let three = immediate(&p, 3.0);
    let (t, u) = (one + 1, three + 1);
    assert_eq!(
        body(&p, "f"),
        [
            st(AddF, a, one, t),
            st(StoreF, t, a, 0),
            st(Lt, a, three, u),
            st(If, u, -3, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec QC-14.3: `return;` is `RETURN` with no operand, `return e;` is `RETURN e`.
#[test]
fn return_with_and_without_a_value() {
    let p = progs("float() f = { return 1; };\nvoid() g = { return; };\n");
    let one = immediate(&p, 1.0);
    assert_eq!(body(&p, "f"), [st(Return, one, 0, 0), st(Done, 0, 0, 0)]);
    assert_eq!(body(&p, "g"), [st(Return, 0, 0, 0), st(Done, 0, 0, 0)]);
}

// spec QC-14.9: an expression statement emits its statements, none for a lone name.
#[test]
fn an_expression_statement_with_no_effect_is_emitted() {
    let p = progs("float a;\nvoid() f = { a; a + 1; };\n");
    let a = global(&p, "a");
    let one = immediate(&p, 1.0);
    assert_eq!(
        body(&p, "f"),
        [st(AddF, a, one, one + 1), st(Done, 0, 0, 0)]
    );
}

// spec QC-13.1, QC-13.7: an empty body is allowed and is a `DONE`; every body ends with one.
#[test]
fn a_body_ends_with_done() {
    let p = progs("void() f = {};\nfloat a;\nvoid() g = { { a = 1; } };\n");
    assert_eq!(body(&p, "f"), [st(Done, 0, 0, 0)]);
    let g = body(&p, "g");
    assert_eq!(g.last(), Some(&st(Done, 0, 0, 0)));
    assert_eq!(g.len(), 2);
}

// spec QC-13.6, QC-22.1: a state function's first statement is `STATE frame, next`, the frame a
// shared literal and the next function a global `void()`.
#[test]
fn a_state_header_is_the_first_statement() {
    let p = progs("void() th2;\n$frame s1 s2\nvoid() th1 = [$s2, th2] { };\nvoid() th2 = {};\n");
    let frame = immediate(&p, 1.0);
    let th2 = global(&p, "th2");
    assert_eq!(
        body(&p, "th1"),
        [st(State, frame, th2, 0), st(Done, 0, 0, 0)]
    );
}

// spec QC-13.6, QC-12.6: a next function not yet declared is declared at the header, a global
// after the function's parameters.
#[test]
fn a_new_next_function_is_declared_at_the_header() {
    let p = progs("$frame s1\nvoid(float x) th1 = [$s1, th3] { };\nvoid() th3 = {};\n");
    let (x, th3) = (global(&p, "x"), global(&p, "th3"));
    assert_eq!(th3, x + 2, "x, then the frame's literal 0, then th3");
    let zero = immediate(&p, 0.0);
    assert_eq!(body(&p, "th1")[0], st(State, zero, th3, 0));
}
