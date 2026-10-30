// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's constructs on the vanilla opcodes: `docs/spec/dialect-fteqcc-rerelease.md`. The
//! expected statements follow FTEQCC's lowering as each statement quotes it; a temporary is
//! read from the statement that made it.

use qc_progs::opcode::Opcode::{
    self, AddF, And, BitAnd, BitOr, Call1, Call2, Call3, DivF, Done, EqF, Goto, If, IfNot, Lt,
    MulF, MulV, MulVf, NotV, Return, StoreF, StoreS, StoreV, SubF, SubV,
};
use qc_progs::{ProgsFile, Statement};

use super::{
    body, compile_fteqcc, fte_progs, function, global, immediate, immediate_string,
    immediate_vector, st, string, words,
};
use crate::{CompileError, Internal};

/// The opcodes of `statements`, in order.
fn opcodes(statements: &[Statement]) -> Vec<u32> {
    statements.iter().map(|s| s.op).collect()
}

fn ops(list: &[Opcode]) -> Vec<u32> {
    list.iter().map(|o| u32::from(o.number())).collect()
}

/// The words of the global `name`.
fn global_words(p: &ProgsFile, name: &str, n: usize) -> Vec<u32> {
    words(p, global(p, name), n)
}

/// The string table offset of `text`, as a word.
fn string_word(p: &ProgsFile, text: &str) -> u32 {
    let mut at = 0;
    while at < p.strings.len() {
        if string(p, i32::try_from(at).unwrap()) == text {
            return u32::try_from(at).unwrap();
        }
        at += 1;
    }
    panic!("no string {text}");
}

const STRUCT: &str = "typedef struct { string m; float f; } P;\n";

// spec FTE-13.10: an array takes its elements' words in turn, written from its aggregate
// initialiser; a vector array makes no component definitions
#[test]
fn an_array_is_its_elements_in_consecutive_globals() {
    let p = fte_progs(
        "string s[] = { \"a\", \"b\" };\nfloat f[3] = { 1, 2, 3 };\nvector v[2];\nfloat after;\n",
    );
    assert_eq!(
        global_words(&p, "s", 2),
        [string_word(&p, "a"), string_word(&p, "b")]
    );
    assert_eq!(
        global_words(&p, "f", 3),
        [1.0_f32.to_bits(), 2.0_f32.to_bits(), 3.0_f32.to_bits()]
    );
    assert_eq!(global(&p, "after"), global(&p, "v") + 6);
    assert!(p.global_defs.iter().all(|d| string(&p, d.s_name) != "v_x"));
}

// spec FTE-13.6: a constant index is the element's global, with no statement
#[test]
fn a_constant_index_is_the_elements_global() {
    let p = fte_progs("float f[3] = { 1, 2, 3 };\nfloat g;\nvoid() t = { g = f[2]; f[1] = g; };\n");
    let (f, g) = (global(&p, "f"), global(&p, "g"));
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, f + 2, g, 0),
            st(StoreF, g, f + 1, 0),
            st(Done, 0, 0, 0)
        ]
    );
}

// spec FTE-13.6: a constant index outside the array is not lowered
#[test]
fn a_constant_index_outside_the_array_is_not_lowered() {
    let result = compile_fteqcc("float f[3];\nfloat g;\nvoid() t = { g = f[3]; };\n");
    assert!(
        matches!(
            result,
            Err(CompileError::Internal {
                internal: Internal::Unlowered,
                ..
            })
        ),
        "{result:?}"
    );
}

// spec FTE-13.12: a subscript by a value calls the array's getter with the index
#[test]
fn a_subscript_by_a_value_calls_the_getter() {
    let p = fte_progs(
        "void(string e) error = #10;\nstring s[] = { \"a\", \"b\" };\nstring r;\n\
         void(float i) t = { r = s[i]; };\n",
    );
    let (i, r) = (global(&p, "i"), global(&p, "r"));
    let getter = global(&p, "ArrayGet*s");
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, i, 4, 0),
            st(Call1, getter, 0, 0),
            st(StoreS, 1, r, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec FTE-13.12: a struct element's member is read at the index times the element's words
// plus the member's word; a whole element a word at a time
#[test]
fn a_struct_elements_words_are_read_one_at_a_time() {
    let p = fte_progs(&format!(
        "void(string e) error = #10;\n{STRUCT}P ps[] = {{ {{ \"x\", 1 }}, {{ \"y\", 2 }} }};\n\
         float g;\nP q;\nvoid(float i) t = {{ g = ps[i].f; }};\nvoid(float i) u = {{ q = ps[i]; }};\n"
    ));
    let getter = global(&p, "ArrayGet*ps");
    let (two, one, g) = (immediate(&p, 2.0), immediate(&p, 1.0), global(&p, "g"));
    let t = body(&p, "t");
    let i = global(&p, "i");
    assert_eq!(opcodes(&t), ops(&[MulF, AddF, StoreF, Call1, StoreF, Done]));
    assert_eq!((t[0].a, t[0].b), (i, two));
    assert_eq!((t[1].a, t[1].b), (t[0].c, one));
    assert_eq!(t[2], st(StoreF, t[1].c, 4, 0));
    assert_eq!(t[3], st(Call1, getter, 0, 0));
    assert_eq!(t[4], st(StoreF, 1, g, 0));
    let u = body(&p, "u");
    assert_eq!(
        opcodes(&u),
        ops(&[
            MulF, StoreF, Call1, StoreF, AddF, StoreF, Call1, StoreF, StoreF, StoreF, Done
        ])
    );
    let temp = u[3].b;
    assert_eq!(u[7].b, temp + 1);
    let q = global(&p, "q");
    assert_eq!(u[8], st(StoreF, temp, q, 0));
    assert_eq!(u[9], st(StoreF, temp + 1, q + 1, 0));
}

// spec FTE-13.12: a vector array's index in words is divided by 3 for the getter
#[test]
fn a_vector_elements_index_is_divided_by_three() {
    let p = fte_progs(
        "void(string e) error = #10;\nvector va[2];\nvector r;\nvoid(float i) t = { r = va[i]; };\n",
    );
    let t = body(&p, "t");
    assert_eq!(opcodes(&t), ops(&[MulF, DivF, StoreF, Call1, StoreV, Done]));
    let three = immediate(&p, 3.0);
    assert_eq!((t[0].b, t[1].a, t[1].b), (three, t[0].c, three));
    assert_eq!(t[4], st(StoreV, 1, global(&p, "r"), 0));
}

// spec FTE-13.8: a getter checks the index is not negative, returns the word the ladder finds,
// else calls `error` and returns the last word
#[test]
fn a_getter_is_a_ladder_with_a_bounds_error() {
    let p = fte_progs(
        "void(string e) error = #10;\nstring s[] = { \"a\", \"b\" };\nstring r;\n\
         void(float i) t = { r = s[i]; };\n",
    );
    let getter = function(&p, "ArrayGet*s");
    assert_eq!(
        (getter.numparms, getter.parm_size[0], getter.locals),
        (1, 1, 1)
    );
    let index = getter.parm_start;
    let s = global(&p, "s");
    let g = body(&p, "ArrayGet*s");
    let (zero, one, two) = (immediate(&p, 0.0), immediate(&p, 1.0), immediate(&p, 2.0));
    let message = immediate_string(&p, "bounds check failed\n");
    let error = global(&p, "error");
    assert_eq!(
        g,
        [
            st(Lt, index, zero, g[0].c),
            st(If, g[0].c, 7, 0),
            st(Lt, index, one, g[2].c),
            st(IfNot, g[2].c, 2, 0),
            st(Return, s, 0, 0),
            st(Lt, index, two, g[5].c),
            st(IfNot, g[5].c, 2, 0),
            st(Return, s + 1, 0, 0),
            st(StoreS, message, 4, 0),
            st(Call1, error, 0, 0),
            st(Return, s + 1, 0, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec FTE-13.8: more than four slots are halved with a test that skips the lower half
#[test]
fn a_getter_of_more_than_four_slots_skips_a_half() {
    let p = fte_progs(
        "void(string e) error = #10;\nfloat f[6];\nfloat r;\nvoid(float i) t = { r = f[i]; };\n",
    );
    let g = body(&p, "ArrayGet*f");
    let index = function(&p, "ArrayGet*f").parm_start;
    // After the bounds test: `LT index, 4` and `IFNOT` past the nine statements of slots 0 to 2.
    assert_eq!(
        (g[2].op, g[2].a, g[2].b),
        (u32::from(Lt.number()), index, immediate(&p, 4.0))
    );
    assert_eq!(g[3], st(IfNot, g[2].c, 10, 0));
    assert_eq!(g[13], st(Lt, index, immediate(&p, 4.0), g[13].c));
    // Two halves of nine statements, then the error call, the last return and DONE.
    assert_eq!(g.len(), 2 + 2 + 9 + 9 + 2 + 2);
    assert_eq!(g[g.len() - 2], st(Return, global(&p, "f") + 5, 0, 0));
}

// spec FTE-13.8: with `sprintf` defined the bounds error formats the index and the slots
#[test]
fn a_getter_formats_its_bounds_error_with_sprintf() {
    let p = fte_progs(
        "void(string e) error = #10;\nstring(...) sprintf = #11;\nfloat f[2];\nfloat r;\n\
         void(float i) t = { r = f[i]; };\n",
    );
    let g = body(&p, "ArrayGet*f");
    let index = function(&p, "ArrayGet*f").parm_start;
    let format = immediate_string(&p, "bounds check failed (0 <= %g < %g is false)\n");
    let tail = &g[g.len() - 8..];
    assert_eq!(
        tail,
        [
            st(StoreS, format, 4, 0),
            st(StoreF, index, 7, 0),
            st(StoreF, immediate(&p, 2.0), 10, 0),
            st(Call3, global(&p, "sprintf"), 0, 0),
            st(StoreS, 1, 4, 0),
            st(Call1, global(&p, "error"), 0, 0),
            st(Return, global(&p, "f") + 1, 0, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec FTE-13.9: `length` is a float immediate of the element count
#[test]
fn an_arrays_length_is_a_float_immediate() {
    let p = fte_progs(
        "string s[] = { \"a\", \"b\", \"c\" };\nfloat n;\nvoid() t = { n = s.length; };\n",
    );
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, immediate(&p, 3.0), global(&p, "n"), 0),
            st(Done, 0, 0, 0)
        ]
    );
}

// spec FTE-14.5: a struct takes its layout's words, and a member is its word
#[test]
fn a_struct_member_is_a_word_of_the_struct() {
    let p = fte_progs(&format!(
        "{STRUCT}P p;\nfloat after;\nfloat g;\nvoid() t = {{ g = p.f; p.f = 3; }};\n"
    ));
    let (p_ofs, g) = (global(&p, "p"), global(&p, "g"));
    assert_eq!(global(&p, "after"), p_ofs + 2);
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, p_ofs + 1, g, 0),
            st(StoreF, immediate(&p, 3.0), p_ofs + 1, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec FTE-14.6: a struct is copied by STORE_V for each three words and STORE_F for the rest
#[test]
fn a_struct_is_copied_by_word_groups() {
    let p =
        fte_progs("typedef struct { vector v; float f; } Q;\nQ a;\nQ b;\nvoid() t = { b = a; };\n");
    let (a, b) = (global(&p, "a"), global(&p, "b"));
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreV, a, b, 0),
            st(StoreF, a + 3, b + 3, 0),
            st(Done, 0, 0, 0)
        ]
    );
}

// spec FTE-14.7: a struct's initialiser writes each member at its word, an array one group per
// element
#[test]
fn a_struct_initialiser_writes_its_members_in_order() {
    let p = fte_progs(&format!(
        "{STRUCT}P ps[] = {{ {{ \"x\", 1 }}, {{ \"y\", 2 }} }};\n"
    ));
    assert_eq!(
        global_words(&p, "ps", 4),
        [
            string_word(&p, "x"),
            1.0_f32.to_bits(),
            string_word(&p, "y"),
            2.0_f32.to_bits()
        ]
    );
}

// spec FTE-15.5: each enum item is a constant global made in order with its value
#[test]
fn enum_items_are_initialised_globals_in_order() {
    let p = fte_progs("enum { A, B = 5, C };\n");
    let (a, b, c) = (global(&p, "A"), global(&p, "B"), global(&p, "C"));
    assert_eq!((b, c), (a + 1, a + 2));
    assert_eq!(
        words(&p, a, 3),
        [0.0_f32.to_bits(), 5.0_f32.to_bits(), 6.0_f32.to_bits()]
    );
}

// spec FTE-15.6: a named enum's type is its base type
#[test]
fn a_named_enum_is_its_base_type() {
    let p = fte_progs("enum E { A, B };\nfloat(E e) f = { return e + B; };\n");
    let f = body(&p, "f");
    assert_eq!(opcodes(&f), ops(&[AddF, Return, Done]));
    assert_eq!((f[0].a, f[0].b), (global(&p, "e"), global(&p, "B")));
}

// spec FTE-15.7: an `enum : int` item read as a float is its value converted
#[test]
fn an_int_enum_item_is_its_float_value() {
    let p = fte_progs("enum : int { A, B };\nfloat g;\nvoid() t = { g = B; };\n");
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, immediate(&p, 1.0), global(&p, "g"), 0),
            st(Done, 0, 0, 0)
        ]
    );
}

// spec FTE-19.7: `a ^ b` is `(a & ~b) | (b & ~a)`, each `~x` as `-1 - x`; `^=` stores it
#[test]
fn xor_is_emulated() {
    let p = fte_progs("float a, b, c;\nvoid() t = { c = a ^ b; a ^= b; };\n");
    let (a, b, c) = (global(&p, "a"), global(&p, "b"), global(&p, "c"));
    let minus_one = immediate(&p, -1.0);
    let t = body(&p, "t");
    assert_eq!(t[0], st(SubF, minus_one, b, t[0].c));
    assert_eq!(t[1], st(BitAnd, a, t[0].c, t[1].c));
    assert_eq!(t[2], st(SubF, minus_one, a, t[2].c));
    assert_eq!(t[3], st(BitAnd, b, t[2].c, t[3].c));
    // FTEF-29.13: the last operator writes `c`, as the pinned FTEQCC writes it.
    assert_eq!(t[4], st(BitOr, t[1].c, t[3].c, c));
    assert_eq!(
        opcodes(&t[5..]),
        ops(&[SubF, BitAnd, SubF, BitAnd, BitOr, Done])
    );
    assert_eq!(t[9].c, a);
}

// spec FTE-19.8: `a % b` is `a - b * (a/b | a/b)` without a `mod` function
#[test]
fn modulo_is_emulated() {
    let p = fte_progs("float a, b, c;\nvoid() t = { c = a % b; };\n");
    let (a, b) = (global(&p, "a"), global(&p, "b"));
    let t = body(&p, "t");
    assert_eq!(t[0], st(DivF, a, b, t[0].c));
    assert_eq!(t[1], st(BitOr, t[0].c, t[0].c, t[1].c));
    assert_eq!(t[2], st(MulF, b, t[1].c, t[2].c));
    // FTEF-29.13: the subtraction writes `c`, as the pinned FTEQCC writes it.
    assert_eq!(t[3], st(SubF, a, t[2].c, global(&p, "c")));
}

// spec FTE-19.8: `a % b` calls the program's `mod(a, b)` when it defines one
#[test]
fn modulo_calls_mod() {
    let p = fte_progs(
        "float(float v, float m) mod = { return v; };\nfloat a, b, c;\nvoid() t = { c = a % b; };\n",
    );
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, global(&p, "a"), 4, 0),
            st(StoreF, global(&p, "b"), 7, 0),
            st(Call2, global(&p, "mod"), 0, 0),
            st(StoreF, 1, global(&p, "c"), 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec FTE-19.9: `v / f` is `v * (1 / f)`
#[test]
fn vector_division_is_emulated() {
    let p = fte_progs("vector v, r;\nfloat f;\nvoid() t = { r = v / f; };\n");
    let t = body(&p, "t");
    assert_eq!(t[0], st(DivF, immediate(&p, 1.0), global(&p, "f"), t[0].c));
    // FTEF-29.13: the product writes `r`, as the pinned FTEQCC writes it.
    assert_eq!(t[1], st(MulVf, global(&p, "v"), t[0].c, global(&p, "r")));
}

// spec FTE-2.1: a `for` loop's declaration, its condition tested by IFNOT past the loop, its
// step, and GOTO back to the condition; the step `i++`, whose value is not read, is one ADD_F
// (FTEF-29.13), as the pinned FTEQCC writes it
#[test]
fn a_for_loop_tests_its_condition() {
    let p = fte_progs("float n;\nvoid() t = { for (float i = 0; i < 3; i++) n = i; };\n");
    let t = body(&p, "t");
    assert_eq!(
        opcodes(&t),
        ops(&[StoreF, Lt, IfNot, StoreF, AddF, Goto, Done])
    );
    assert_eq!(t[2].b, 4);
    assert_eq!(t[5].a, -4);
    assert_eq!((t[4].a, t[4].c), (t[0].b, t[0].b), "i is stepped in place");
}

// spec FTE-2.4: `break` is a GOTO past the loop and `continue` a GOTO to its step
#[test]
fn break_and_continue_leave_and_restart_a_loop() {
    let p = fte_progs(
        "float n;\nvoid() t = { for (;;) { break; } };\nvoid() u = { for (;; n++) { continue; } };\n",
    );
    assert_eq!(
        body(&p, "t"),
        [st(Goto, 2, 0, 0), st(Goto, -1, 0, 0), st(Done, 0, 0, 0)]
    );
    // FTEF-29.13: the step `n++` is one ADD_F, as the pinned FTEQCC writes it.
    let u = body(&p, "u");
    assert_eq!(opcodes(&u), ops(&[Goto, AddF, Goto, Done]));
    assert_eq!((u[0].a, u[2].a), (1, -2));
}

// spec FTE-3.4: a switch jumps to its tests after the body, each case an EQ_F and IF back to
// its label, then a GOTO to `default`; `break` goes past the switch
#[test]
fn a_switch_tests_its_cases_after_its_body() {
    let p = fte_progs(
        "float a, b;\nvoid() t = { switch (a) { case 1: b = 1; break; default: b = 2; } };\n",
    );
    let (a, b) = (global(&p, "a"), global(&p, "b"));
    let (one, two) = (immediate(&p, 1.0), immediate(&p, 2.0));
    let t = body(&p, "t");
    assert_eq!(
        t,
        [
            st(Goto, 5, 0, 0),
            st(StoreF, one, b, 0),
            st(Goto, 6, 0, 0),
            st(StoreF, two, b, 0),
            st(Goto, 4, 0, 0),
            st(EqF, a, one, t[5].c),
            st(If, t[5].c, -5, 0),
            st(Goto, -4, 0, 0),
            st(Done, 0, 0, 0),
        ]
    );
}

// spec FTE-3.5: a call's value switched on is copied out of the return global before the jump to
// the tests, so a call in the body does not change what the cases test
#[test]
fn a_switch_holds_a_calls_value() {
    let p = fte_progs(
        "float() f = #1;\nvoid() g = #2;\nvoid() t = { switch (f()) { case 1: g(); break; } };\n",
    );
    let one = immediate(&p, 1.0);
    let t = body(&p, "t");
    assert_eq!(t[0].op, u32::from(Opcode::Call0.number()));
    assert_eq!(t[1], st(StoreF, 1, t[1].b, 0));
    assert_eq!(t[2].op, u32::from(Goto.number()));
    let test = t.iter().find(|s| s.op == u32::from(EqF.number())).unwrap();
    assert_eq!((test.a, test.b), (t[1].b, one));
}

// spec FTE-5.3: `x++` copies the old value to a temporary, stores the temporary plus 1 back,
// and is the old value; a `x--` whose value is not read is one ADD_F (FTEF-29.13), as the
// pinned FTEQCC writes both
#[test]
fn postfix_increment_is_the_old_value() {
    let p = fte_progs("float a, b;\nvoid() t = { b = a++; a--; };\n");
    let (a, b) = (global(&p, "a"), global(&p, "b"));
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, a, t[0].b, 0));
    assert_eq!(t[1], st(AddF, t[0].b, immediate(&p, 1.0), a));
    assert_eq!(t[2], st(StoreF, t[0].b, b, 0));
    assert_eq!(t[3], st(AddF, a, immediate(&p, -1.0), a));
    assert_eq!(t[4], st(Done, 0, 0, 0));
}

// spec FTE-19.4: `-x` is `0 - x`, of a vector `'0 0 0' - v`; `~x` is `-1 - x`; `++x` stores
// `x + 1` and is it
#[test]
fn prefix_operators_are_subtractions_and_stores() {
    let p =
        fte_progs("float a, b;\nvector v, w;\nvoid() t = { b = -a; w = -v; b = ~a; b = ++a; };\n");
    let (a, b, v) = (global(&p, "a"), global(&p, "b"), global(&p, "v"));
    let t = body(&p, "t");
    // FTEF-29.13: each operator writes its target, as the pinned FTEQCC writes them.
    assert_eq!(t[0], st(SubF, immediate(&p, 0.0), a, b));
    assert_eq!((t[1].op, t[1].b), (u32::from(SubV.number()), v));
    assert_eq!(t[2], st(SubF, immediate(&p, -1.0), a, b));
    assert_eq!(t[3], st(AddF, a, immediate(&p, 1.0), a));
    assert_eq!(t[4], st(StoreF, a, b, 0));
}

// spec FTE-8.3: `c ? a : b` is IFNOT over the first arm's store and a GOTO over the second's,
// both to one temporary; `c ?: b` stores the condition as the first arm
#[test]
fn a_ternary_stores_either_arm_to_one_temporary() {
    let p = fte_progs("float a, b, c;\nvoid() t = { c = a ? b : 1; c = a ?: b; };\n");
    let (a, b, c) = (global(&p, "a"), global(&p, "b"), global(&p, "c"));
    let t = body(&p, "t");
    let r = t[1].b;
    assert_eq!(
        t[..5],
        [
            st(IfNot, a, 3, 0),
            st(StoreF, b, r, 0),
            st(Goto, 2, 0, 0),
            st(StoreF, immediate(&p, 1.0), r, 0),
            st(StoreF, r, c, 0),
        ]
    );
    assert_eq!(t[6], st(StoreF, a, t[6].b, 0));
    assert_eq!(t[8], st(StoreF, b, t[6].b, 0));
}

// spec FTEF-23.10: under `ifvector` a vector condition is NOT_V and IF, so every component
// counts
#[test]
fn a_vector_condition_is_tested_by_not_v() {
    let p = fte_progs("vector v;\nfloat a;\nvoid() t = { if (v) a = 1; };\n");
    let t = body(&p, "t");
    assert_eq!(t[0], st(NotV, global(&p, "v"), 0, t[0].c));
    assert_eq!(t[1], st(If, t[0].c, 2, 0));
}

// spec FTE-19.10: `v && a` is the vector's MUL_V with itself, then AND
#[test]
fn logic_of_a_vector_is_its_dot_product() {
    let p = fte_progs("vector v;\nfloat a, b;\nvoid() t = { if (v && a) b = 1; };\n");
    let (v, a) = (global(&p, "v"), global(&p, "a"));
    let t = body(&p, "t");
    assert_eq!(t[0], st(MulV, v, v, t[0].c));
    assert_eq!(t[1], st(And, t[0].c, a, t[1].c));
}

// spec FTE-19.1: `a op= b` stores `a op b` to `a`; `&~=` stores `a - (a & b)`, each operator
// writing `a` (FTEF-29.13)
#[test]
fn compound_assignments_store_back() {
    let p = fte_progs("float a, b;\nvoid() t = { a += b; a &~= b; };\n");
    let (a, b) = (global(&p, "a"), global(&p, "b"));
    let t = body(&p, "t");
    // FTEF-29.13: each store folds into its operator, as the pinned FTEQCC writes them.
    assert_eq!(t[0], st(AddF, a, b, a));
    assert_eq!(t[1], st(BitAnd, a, b, t[1].c));
    assert_eq!(t[2], st(SubF, a, t[1].c, a));
}

// spec FTE-9.3: `v.y` is the vector's second word
#[test]
fn a_vector_component_is_its_word() {
    let p = fte_progs("vector v;\nfloat a;\nvoid() t = { a = v.y; v.z = a; a = v[0]; };\n");
    let (v, a) = (global(&p, "v"), global(&p, "a"));
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, v + 1, a, 0),
            st(StoreF, a, v + 2, 0),
            st(StoreF, v, a, 0),
            st(Done, 0, 0, 0)
        ]
    );
}

// spec FTEF-32.4: a local's initialiser is a store each time the function runs
#[test]
fn a_local_initialiser_is_a_store() {
    let p = fte_progs("void() t = { local float a = 2; };\n");
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, immediate(&p, 2.0), global(&p, "a"), 0));
}

// spec FTEF-34.9: a global's initialiser that is an expression is the value it folds to
#[test]
fn a_global_initialiser_is_its_folded_value() {
    let p = fte_progs("const float a = 2 * 3 + 1;\n");
    assert_eq!(global_words(&p, "a", 1), [7.0_f32.to_bits()]);
}

// spec FTE-17.1: a C-style definition's parameters are the names after its own
#[test]
fn c_style_parameters_are_the_functions_first_locals() {
    let p = fte_progs("float f(float x, float y) { return y; };\n");
    let f = function(&p, "f");
    assert_eq!((f.numparms, f.parm_start), (2, global(&p, "x")));
    assert_eq!(body(&p, "f")[0], st(Return, global(&p, "y"), 0, 0));
}

// spec FTEF-32.3: under FTEQCC an argument past the last parameter is read and ignored, and
// fewer arguments leave the rest
// spec FTE-18.4: every argument is read before any is stored to its slot
#[test]
fn extra_arguments_are_read_and_dropped() {
    let p = fte_progs("void(float x) f = {};\nfloat a;\nvoid() t = { f(a, a + 1); f(); };\n");
    let t = body(&p, "t");
    assert_eq!(
        opcodes(&t),
        ops(&[AddF, StoreV, Call1, Opcode::Call0, Done])
    );
    assert_eq!(t[1], st(StoreV, global(&p, "a"), 4, 0));
}

// spec FTE-18.4: a call's value is copied out of the return global before a later argument
// calls again, so neither call overwrites the other's slot
#[test]
fn a_call_in_an_argument_is_copied_before_the_next() {
    let p = fte_progs(
        "float(float x) g = { return x; };\nvoid(float x, float y) f = {};\n\
         void() t = { f(g(1), g(2)); };\n",
    );
    let g = global(&p, "g");
    let t = body(&p, "t");
    assert_eq!(
        opcodes(&t),
        ops(&[
            StoreV, Call1, StoreF, StoreV, Call1, StoreV, StoreV, Call2, Done
        ])
    );
    assert_eq!(t[1], st(Call1, g, 0, 0));
    let copy = t[2].b;
    assert_eq!(t[2], st(StoreF, 1, copy, 0));
    assert_eq!(t[5], st(StoreV, copy, 4, 0));
    assert_eq!(t[6], st(StoreV, 1, 7, 0));
}

// spec FTE-22.3: a name used before its definition is read resolves to that definition
#[test]
fn a_name_used_before_its_definition_is_made_there() {
    let p = fte_progs("#pragma autoproto\nvoid() t = { later = 1; };\nfloat later;\n");
    assert_eq!(
        body(&p, "t")[0],
        st(StoreF, immediate(&p, 1.0), global(&p, "later"), 0)
    );
}

// spec FTE-9.4: a store to an entity's vector field's component goes through ADDRESS of the
// component's field; `+=` reads it by LOAD_F, before the ADDRESS (FTE-6.4), as the pinned
// FTEQCC writes it
#[test]
fn an_entity_vectors_component_is_stored_through_its_field() {
    let p = fte_progs(
        ".vector origin;\nentity e;\nvoid() t = { e.origin.y = 5; e.origin[2] += 1; };\n",
    );
    let (e, y, z) = (
        global(&p, "e"),
        global(&p, "origin_y"),
        global(&p, "origin_z"),
    );
    let t = body(&p, "t");
    assert_eq!(t[0], st(Opcode::Address, e, y, t[0].c));
    assert_eq!(t[1], st(Opcode::StorepF, immediate(&p, 5.0), t[0].c, 0));
    assert_eq!(t[2], st(Opcode::LoadF, e, z, t[2].c));
    assert_eq!(t[3], st(AddF, t[2].c, immediate(&p, 1.0), t[3].c));
    assert_eq!(t[4], st(Opcode::Address, e, z, t[4].c));
    assert_eq!(t[5], st(Opcode::StorepF, t[3].c, t[4].c, 0));
}

// spec FTE-6.4: `e.f += x` reads the entity once, the field after the right side, and makes its
// ADDRESS after the value, as the pinned FTEQCC writes `find().health += 1`
#[test]
fn a_compound_store_to_a_field_reads_the_entity_once() {
    let p = fte_progs(".float health;\nentity() find = #1;\nvoid() t = { find().health += 1; };\n");
    assert_eq!(
        opcodes(&body(&p, "t")),
        ops(&[
            Opcode::Call0,
            Opcode::LoadF,
            AddF,
            Opcode::Address,
            Opcode::StorepF,
            Done
        ])
    );
}

// spec FTE-6.4: a store to a field makes its ADDRESS after the stored value, so the right side's
// call comes first, and a compound store or `++` reads the field after the right side, as the
// pinned FTEQCC writes each of these
#[test]
fn a_field_store_makes_its_address_after_the_value() {
    let p = fte_progs(
        "entity self;\n.float health;\n.vector origin;\nfloat() g = #2;\n\
         void() u = { self.health = g(); };\nvoid() w = { self.health += g(); };\n\
         void() x = { self.origin_x = g(); };\nvoid() y = { self.health++; };\n",
    );
    let store = ops(&[Opcode::Call0, Opcode::Address, Opcode::StorepF, Done]);
    assert_eq!(opcodes(&body(&p, "u")), store);
    assert_eq!(opcodes(&body(&p, "x")), store);
    assert_eq!(
        opcodes(&body(&p, "w")),
        ops(&[
            Opcode::Call0,
            Opcode::LoadF,
            AddF,
            Opcode::Address,
            Opcode::StorepF,
            Done
        ])
    );
    let y = opcodes(&body(&p, "y"));
    assert_eq!(y.first(), Some(&u32::from(Opcode::LoadF.number())));
    assert_eq!(
        y.get(y.len() - 3..),
        Some(ops(&[Opcode::Address, Opcode::StorepF, Done]).as_slice())
    );
}

// spec FTE-13.6: a named constant index is a constant index too
#[test]
fn a_named_constant_index_is_the_elements_global() {
    let p = fte_progs(
        "const float SLOT = 1;\nfloat a[3];\nfloat g;\nvoid() t = { a[SLOT] = 4; g = a[SLOT]; };\n",
    );
    let (a, g) = (global(&p, "a"), global(&p, "g"));
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, immediate(&p, 4.0), a + 1, 0),
            st(StoreF, a + 1, g, 0),
            st(Done, 0, 0, 0)
        ]
    );
}

// spec FTE-13.8: every subscript of one array calls the one getter
#[test]
fn an_arrays_subscripts_share_its_getter() {
    let p = fte_progs(
        "void(string e) error = #10;\nfloat f[2];\nfloat g;\n\
         void(float i) t = { g = f[i]; g = f[i + 1]; };\n",
    );
    let getters = p
        .global_defs
        .iter()
        .filter(|d| string(&p, d.s_name) == "ArrayGet*f")
        .count();
    assert_eq!(getters, 1);
}

// spec FTE-13.8: four slots are halved with no skipping test
#[test]
fn a_getter_of_four_slots_has_no_skip() {
    let p = fte_progs(
        "void(string e) error = #10;\nfloat f[4];\nfloat r;\nvoid(float i) t = { r = f[i]; };\n",
    );
    assert_eq!(body(&p, "ArrayGet*f").len(), 2 + 12 + 2 + 2);
}

// spec FTE-15.8: an integer global initialised by a number holds the integer
// spec FTE-15.7: an integer constant read as a float is its value converted
#[test]
fn an_integer_constant_holds_its_integer() {
    let p = fte_progs(
        "const __uint32 U = 3;\nconst int I = -2;\nfloat g;\nvoid() t = { g = U; g = I; };\n",
    );
    assert_eq!(global_words(&p, "U", 1), [3]);
    assert_eq!(
        global_words(&p, "I", 1),
        [u32::from_ne_bytes((-2_i32).to_ne_bytes())]
    );
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, immediate(&p, 3.0), global(&p, "g"), 0));
    assert_eq!(t[1], st(StoreF, immediate(&p, -2.0), global(&p, "g"), 0));
}

// spec FTE-2.4: `continue` in a switch in a loop goes to the loop, not the switch
#[test]
fn continue_in_a_switch_belongs_to_the_loop() {
    let p = fte_progs("float a;\nvoid() t = { while (a) { switch (a) { case 1: continue; } } };\n");
    let t = body(&p, "t");
    // IFNOT a, the switch's GOTO to its tests, then the `continue`'s GOTO to the loop's top.
    assert_eq!(t[2], st(Goto, -2, 0, 0));
}

// spec FTE-13.1: a local `vector v = {x, y, z}` stores each element to its component
#[test]
fn a_local_vector_initialiser_stores_its_components() {
    let p = fte_progs("void() t = { vector v = {1, 2, 3}; };\n");
    let v = global(&p, "v");
    assert_eq!(
        body(&p, "t"),
        [
            st(StoreF, immediate(&p, 1.0), v, 0),
            st(StoreF, immediate(&p, 2.0), v + 1, 0),
            st(StoreF, immediate(&p, 3.0), v + 2, 0),
            st(Done, 0, 0, 0)
        ]
    );
}

// spec FTE-19.4: `--x` stores `x - 1`; `~v` is `'-1 -1 -1' - v`
#[test]
fn prefix_decrement_and_vector_not() {
    let p = fte_progs("float a;\nvector v, w;\nvoid() t = { --a; w = ~v; };\n");
    let t = body(&p, "t");
    // FTEF-29.13: each operator writes its target, as the pinned FTEQCC writes them.
    let a = global(&p, "a");
    assert_eq!(t[0], st(AddF, a, immediate(&p, -1.0), a));
    let minus = super::immediate_vector(&p, [-1.0; 3]);
    assert_eq!(t[1], st(SubV, minus, global(&p, "v"), global(&p, "w")));
}

// spec FTE-19.1: each `op=` stores its operator's result back
#[test]
fn each_compound_assignment_uses_its_operator() {
    let p = fte_progs("float a, b;\nvoid() t = { a -= b; a *= b; a /= b; a &= b; a |= b; };\n");
    assert_eq!(
        opcodes(&body(&p, "t")),
        ops(&[SubF, MulF, DivF, BitAnd, BitOr, Done])
    );
}

// spec FTE-9.1: `v.x` is the first word; on an entity, `.x` is a field named `x`
#[test]
fn x_is_a_component_of_a_vector_and_a_field_of_an_entity() {
    let p =
        fte_progs(".float x;\nentity e;\nvector v;\nfloat a;\nvoid() t = { a = v.x; a = e.x; };\n");
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, global(&p, "v"), global(&p, "a"), 0));
    assert_eq!(
        t[1],
        st(Opcode::LoadF, global(&p, "e"), global(&p, "x"), t[1].c)
    );
}

// spec FTE-9.4: `.x` and `.z` of an entity's vector field are stored through `v_x` and `v_z`
#[test]
fn each_component_of_an_entity_vector_is_its_own_field() {
    let p =
        fte_progs(".vector origin;\nentity e;\nvoid() t = { e.origin.x = 1; e.origin.z = 2; };\n");
    let t = body(&p, "t");
    assert_eq!((t[0].a, t[0].b), (global(&p, "e"), global(&p, "origin_x")));
    assert_eq!((t[2].a, t[2].b), (global(&p, "e"), global(&p, "origin_z")));
}

// spec FTE-2.4: `continue` in a switch in a `do` loop goes to the loop's condition
#[test]
fn continue_in_a_switch_in_a_do_loop_belongs_to_the_loop() {
    let p =
        fte_progs("float a;\nvoid() t = { do { switch (a) { case 1: continue; } } while (a); };\n");
    let t = body(&p, "t");
    let condition = t
        .iter()
        .rposition(|s| s.op == u32::from(If.number()))
        .unwrap();
    // The `continue` after the switch's GOTO to its tests jumps to the condition.
    assert_eq!(t[1], st(Goto, i32::try_from(condition).unwrap() - 1, 0, 0));
}

// spec FTE-13.12: an initialised `var` is no constant, so its subscript calls the getter
#[test]
fn a_variable_index_calls_the_getter() {
    let p = fte_progs(
        "void(string e) error = #10;\nvar float k = 1;\nfloat a[2];\nfloat g;\n\
         void() t = { g = a[k]; };\n",
    );
    assert!(opcodes(&body(&p, "t")).contains(&u32::from(Call1.number())));
}

// spec FTE-13.10: an array initialised with fewer values than elements leaves the rest 0
#[test]
fn a_short_array_initialiser_leaves_the_rest_zero() {
    let p = fte_progs("float f[3] = { 1 };\n");
    assert_eq!(global_words(&p, "f", 3), [1.0_f32.to_bits(), 0, 0]);
}

// spec FTE-18.4: the first argument of a precache builtin is recorded under FTEQCC too
#[test]
fn an_fteqcc_precache_is_recorded() {
    let compiled = compile_fteqcc(
        "void(string s) precache_model = #20;\nvoid() t = { precache_model(\"progs/a.mdl\"); };\n",
    )
    .unwrap();
    assert_eq!(compiled.precaches.models.len(), 1);
}

// spec FTE-23.17: a builtin's function is named its engine name, or with none the definition's
// name, as the rerelease's `bprint = #0:ex_bprint` is found by an engine as `ex_bprint`
#[test]
fn a_builtin_function_is_named_its_engine_name() {
    let p = fte_progs(
        "void(string s, ...) bprint = #0:ex_bprint;\nvoid(string s) dprint = #25;\n\
         void(string s) sprint = :\"ex_\" \"sprint\";\nvoid(string s) cprint = #\"\":ex_cp;\n",
    );
    let builtin = |number: i32| {
        let found: Vec<String> = p
            .functions
            .iter()
            .skip(1)
            .filter(|f| f.first_statement == -number)
            .map(|f| string(&p, f.s_name).to_owned())
            .collect();
        found
    };
    assert_eq!(builtin(0), ["ex_bprint", "ex_sprint", "ex_cp"]);
    assert_eq!(builtin(25), ["dprint"]);
}

// spec FTE-23.18: under FTEQCC a builtin's record has no parameter or local slots, its named
// parameters' count, variadic or not, and no parameter sizes
#[test]
fn an_fteqcc_builtin_record_has_no_parameter_slots() {
    let p = fte_progs(
        "void(string s, ...) bprint = #0:ex_bprint;\nfloat(float d, vector v) walk = #5;\n\
         void(float a, float b, float c, float d, float e, float f, float g, float h, float i) \
         nine = #6;\n",
    );
    let record = |number: i32| {
        p.functions
            .iter()
            .skip(1)
            .find(|f| f.first_statement == -number)
            .unwrap()
    };
    for (number, numparms) in [(0, 1), (5, 2), (6, 8)] {
        let f = record(number);
        assert_eq!(
            (f.parm_start, f.locals, f.numparms, f.parm_size),
            (0, 0, numparms, [0; 8]),
            "#{number}"
        );
    }
}

// spec FTE-18.5: a call's value as a left operand is copied out of the return global before the
// right operand's call, so `f() + g()` adds the two calls' values; under id's qcc the second
// call's value is added to itself (QC-18.6)
#[test]
fn a_calls_value_as_a_left_operand_survives_the_next_call() {
    let source = "float() f = #1;\nfloat() g = #2;\nfloat x;\nvoid() t = { x = f() + g(); };\n";
    let p = fte_progs(source);
    let t = body(&p, "t");
    assert_eq!(
        opcodes(&t),
        ops(&[Opcode::Call0, Opcode::StoreF, Opcode::Call0, AddF, Done])
    );
    assert_eq!(t[1].a, 1, "the copy reads the return global");
    assert_eq!(t[3].a, t[1].b, "the sum reads the copy");
    assert_eq!(t[3].b, 1, "and the second call's value");
    assert_eq!(t[3].c, global(&p, "x"), "and writes x (FTEF-29.13)");
    let vanilla = super::progs(source);
    let v = body(&vanilla, "t");
    assert_eq!(opcodes(&v)[..3], ops(&[Opcode::Call0, Opcode::Call0, AddF]));
    assert_eq!(
        (v[2].a, v[2].b),
        (1, 1),
        "qcc adds the return global to itself"
    );
}

// spec FTE-21.6: under FTEQCC a function's record counts its parameters among its locals, so a
// nested call restores them; id's qcc counts none for a function with no `local` (QC-21.4)
#[test]
fn an_fteqcc_record_counts_its_parameters_among_its_locals() {
    let source = "void(float d, vector v) f = { };\nvoid(float d) g = { local float x; x = d; };\n";
    let p = fte_progs(source);
    let f = function(&p, "f");
    assert_eq!((f.parm_start, f.locals), (global(&p, "d"), 4));
    assert_eq!(function(&p, "g").locals, 2);
    let vanilla = super::progs(source);
    assert_eq!(function(&vanilla, "f").locals, 0);
}

// spec FTEF-19.76: a field defined with another field as its value names the same entity
// field, and takes no field space of its own
#[test]
fn a_field_defined_with_a_field_names_the_same_field() {
    let p = fte_progs(".float length;\n.float t_length = length;\n.float after;\nvoid() t = {};\n");
    let word = |name: &str| p.globals[usize::try_from(global(&p, name)).unwrap()].bits();
    assert_eq!(word("t_length"), word("length"));
    assert_eq!(
        word("after"),
        word("length") + 1,
        "no slot was taken for t_length"
    );
    let fields: Vec<(String, u32)> = p
        .field_defs
        .iter()
        .map(|f| (string(&p, f.s_name).to_owned(), f.ofs))
        .collect();
    assert!(
        fields.contains(&("t_length".to_owned(), word("length"))),
        "{fields:?}"
    );
}

// spec FTEF-19.76: a vector field defined with another names its three words, each component
// its counterpart's
#[test]
fn a_vector_field_defined_with_another_names_its_words() {
    let p = fte_progs(
        ".vector fog_color;\n.vector fadecolor = fog_color;\n.float after;\nvoid() t = {};\n",
    );
    let word = |name: &str| p.globals[usize::try_from(global(&p, name)).unwrap()].bits();
    assert_eq!(word("fadecolor"), word("fog_color"));
    for axis in ["_x", "_y", "_z"] {
        assert_eq!(
            word(&format!("fadecolor{axis}")),
            word(&format!("fog_color{axis}"))
        );
    }
    assert_eq!(word("fog_color_z") - word("fog_color_x"), 2);
    assert_eq!(
        word("after"),
        word("fog_color") + 3,
        "no slots were taken for fadecolor"
    );
}

// spec FTEF-34.15: a constant whose initialiser folded is that value where it is named, so a
// later constant made from it folds too
#[test]
fn a_folded_constant_folds_where_it_is_named() {
    let p = fte_progs(
        "static const float A = 8;\nstatic const float P = 1<<A;\nstatic const float R = P - 1;\n\
         float(float x) g = { return x & R; };\n",
    );
    let value =
        |name: &str| f32::from_bits(p.globals[usize::try_from(global(&p, name)).unwrap()].bits());
    assert_eq!(value("P"), 256.0);
    assert_eq!(value("R"), 255.0);
}

// spec FTEF-31.4: the address of an entity's field is its OP_ADDRESS, a pointer to the field's
// type, so its store to a pointer folds into it (FTEF-29.13)
#[test]
fn the_address_of_an_entitys_field_is_its_address() {
    let p = fte_progs(
        "entity world;\n.float sounds;\nfloat *cd;\nvoid() t = { cd = &world.sounds; };\n",
    );
    let (w, f, cd) = (global(&p, "world"), global(&p, "sounds"), global(&p, "cd"));
    let t = body(&p, "t");
    assert_eq!(t[0], st(Opcode::Address, w, f, cd));
    assert_eq!(t[1].op, u32::from(Done.number()));
}

// spec FTEF-6.18: a float or vector stored through a pointer is its OP_STOREP_F or OP_STOREP_V
// to the pointer
#[test]
fn a_store_through_a_pointer_is_its_storep() {
    let p = fte_progs("float *p; vector *q; float f; vector v;\nvoid() t = { *p = f; *q = v; };\n");
    let g = |name| global(&p, name);
    let t = body(&p, "t");
    assert_eq!(t[0], st(Opcode::StorepF, g("f"), g("p"), 0));
    assert_eq!(t[1], st(Opcode::StorepV, g("v"), g("q"), 0));
}

// spec FTE-13.10: an aggregate initialiser's value may be a constant's name, its words copied
#[test]
fn an_array_initialiser_copies_named_constants() {
    let p = fte_progs(
        "float A = 4; vector V = '1 2 3';\nconst float f[2] = { A, 7 };\n\
         const vector v[1] = { V };\nvoid() t = {};\n",
    );
    let at = |name: &str, i: i32| {
        let ofs = global(&p, name).checked_add(i).unwrap();
        f32::from_bits(p.globals[usize::try_from(ofs).unwrap()].bits())
    };
    assert_eq!((at("f", 0), at("f", 1)), (4.0, 7.0));
    assert_eq!((at("v", 0), at("v", 1), at("v", 2)), (1.0, 2.0, 3.0));
}

// spec FTEF-30.7: a vector constructor of constants is the vector immediate, two null components
// the unit vector times the third by MUL_VF, and otherwise each component stored by STORE_F, a
// missing component the float 0
#[test]
fn a_vector_constructor_builds_its_vector() {
    let p = fte_progs(
        "float a, b; vector v;\nvoid() t = { v = [1, 2]; v = [0, a]; v = [a, b, 3]; };\n",
    );
    let g = |name| global(&p, name);
    let t = body(&p, "t");
    assert_eq!(
        t[0],
        st(StoreV, immediate_vector(&p, [1.0, 2.0, 0.0]), g("v"), 0)
    );
    assert_eq!(
        t[1],
        st(
            Opcode::MulVf,
            immediate_vector(&p, [0.0, 1.0, 0.0]),
            g("a"),
            g("v")
        )
    );
    let temp = t[2].b;
    assert_eq!(t[2], st(StoreF, g("a"), temp, 0));
    assert_eq!(t[3], st(StoreF, g("b"), temp + 1, 0));
    assert_eq!(t[4], st(StoreF, immediate(&p, 3.0), temp + 2, 0));
    assert_eq!(t[5], st(StoreV, temp, g("v"), 0));
}

// spec FTEF-30.13: a call's value in a vector constructor is copied out of the return global
// when a later component makes a call
#[test]
fn a_vector_constructor_holds_a_call_before_another() {
    let p = fte_progs("float() f = #1;\nvector v;\nvoid() t = { v = [f(), f()]; };\n");
    let t = body(&p, "t");
    assert_eq!(t[0].op, u32::from(Opcode::Call0.number()));
    assert_eq!(t[1], st(StoreF, 1, t[1].b, 0));
    assert_eq!(t[2].op, u32::from(Opcode::Call0.number()));
    assert_eq!(t[3].a, t[1].b);
    assert_eq!(t[4].a, 1);
}

// spec FTE-24.1: more than 65530 globals make FTEQCC's program the 32-bit layout
#[test]
fn a_program_of_many_globals_is_32_bit() {
    let p = fte_progs("float big[65531];\nvoid() t = {};\n");
    assert_eq!(p.format, qc_progs::Format::Fte32);
    assert!(qc_progs::write(&p).is_ok());
    assert_eq!(fte_progs("void() t = {};\n").format, qc_progs::Format::V6);
}

// spec FTE-24.2: a global past 32767 in a version 6 program is its operand's low 16 bits
#[test]
fn a_global_past_32767_is_its_low_bits_in_version_6() {
    let p = fte_progs("float big[40000];\nfloat x;\nvoid() t = { x = 1; };\n");
    assert_eq!(p.format, qc_progs::Format::V6);
    let x = global(&p, "x");
    assert!(x > 0x8000);
    let store = body(&p, "t")[0];
    assert_eq!(store.b, x.wrapping_sub(0x1_0000));
    assert!(qc_progs::write(&p).is_ok());
}

// spec FTEF-19.31: a null in an aggregate initialiser, `__NULL__`, is the zero of the element's
// type
#[test]
fn a_null_in_an_aggregate_is_zero() {
    let p = fte_progs("const string s[2] = { \"a\", __NULL__ };\nvoid() t = {};\n");
    let at = global(&p, "s");
    let word = |i: i32| p.globals[usize::try_from(at + i).unwrap()].bits();
    assert_ne!(word(0), 0);
    assert_eq!(word(1), 0);
}

// spec FTEF-30.6: a cast between two types of one basic type takes no statement, the value read
// as the cast's type
#[test]
fn a_cast_within_a_basic_type_is_the_value() {
    let p = fte_progs(
        "var void(float a) g;\nvoid(float a, float b) h = {};\nvoid() t = { g = (void(float a)) h; };\n",
    );
    let t = body(&p, "t");
    assert_eq!(
        t[0],
        st(Opcode::StoreFnc, global(&p, "h"), global(&p, "g"), 0)
    );
    assert_eq!(t[1].op, u32::from(Done.number()));
}

// spec FTEF-23.4: with no row for a function and a float, a null operand converts to the
// function's type, so `g != __NULL__` is NE_FNC
#[test]
fn a_null_compared_with_a_function_takes_its_type() {
    let p = fte_progs("var void() g;\nfloat x;\nvoid() t = { x = g != __NULL__; x = 0 == g; };\n");
    let t = body(&p, "t");
    let null = immediate(&p, 0.0);
    assert_eq!(
        t[0],
        st(Opcode::NeFnc, global(&p, "g"), null, global(&p, "x"))
    );
    assert_eq!(
        t[1],
        st(Opcode::EqFnc, null, global(&p, "g"), global(&p, "x"))
    );
}

// spec FTEF-29.2: a null stored to a function, a global's or an entity's field, is stored by
// the target's type, its word 0
#[test]
fn a_null_is_stored_by_the_targets_type() {
    let p = fte_progs(
        "entity self;\n.void() touch;\nvar void() g;\nvoid() t = { g = __NULL__; self.touch = __NULL__; };\n",
    );
    let t = body(&p, "t");
    let null = immediate(&p, 0.0);
    assert_eq!(t[0], st(Opcode::StoreFnc, null, global(&p, "g"), 0));
    assert_eq!(t[1].op, u32::from(Opcode::Address.number()));
    assert_eq!(t[2], st(Opcode::StorepFnc, null, t[1].c, 0));
}

// spec FTE-20.5: a literal never shares a variable's word, so an element of an array is still
// named by a constant index whose value a variable holds
#[test]
fn a_literal_never_shares_a_variables_word() {
    let p = fte_progs(
        "var float two = 2;\nstring a[10];\nfloat x;\nvoid() t = { a[2] = \"\"; x = 2; };\n",
    );
    let two = global(&p, "two");
    let t = body(&p, "t");
    assert_ne!(t[1].a, two);
    assert_eq!(words(&p, t[1].a, 1), [2.0_f32.to_bits()]);
}

// spec FTEF-32.13: a constant local's aggregate initialiser is its value at compile time, with
// no statement
#[test]
fn a_constant_locals_aggregate_is_its_value() {
    let p = fte_progs("float x;\nvoid() t = { const float tab[] = { 4, 5 }; x = tab[1]; };\n");
    let tab = global(&p, "tab");
    assert_eq!(words(&p, tab, 2), [4.0_f32, 5.0].map(f32::to_bits));
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, tab + 1, global(&p, "x"), 0));
}

// spec FTE-13.11: a store by an index that is a value calls the array's setter with the index
// and the value, after the value is read
#[test]
fn a_store_by_a_value_index_calls_the_setter() {
    let p = fte_progs(
        "void(string e) error = #10;\nfloat a[3];\nfloat i;\nvoid() t = { a[i] = 5; };\n",
    );
    let setter = global(&p, "ArraySet*a");
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, global(&p, "i"), 4, 0));
    assert_eq!(t[1], st(StoreF, immediate(&p, 5.0), 7, 0));
    assert_eq!(t[2], st(Call2, setter, 0, 0));
    assert_eq!(t[3].op, u32::from(Done.number()));
}

// spec FTE-13.11: the setter tests both bounds, halves its slots and stores the value to the
// slot the index names, then calls error
#[test]
fn a_setter_stores_the_slot_its_index_names() {
    let p = fte_progs(
        "void(string e) error = #10;\nfloat a[3];\nfloat i;\nvoid() t = { a[i] = 5; };\n",
    );
    let f = function(&p, "ArraySet*a");
    assert_eq!((f.numparms, f.parm_size[0], f.parm_size[1]), (2, 1, 1));
    let (index, value) = (f.parm_start, f.parm_start + 1);
    let a = global(&p, "a");
    let s = body(&p, "ArraySet*a");
    assert_eq!(s[0], st(Lt, index, immediate(&p, 0.0), s[0].c));
    assert_eq!(s[1], st(If, s[0].c, 15, 0));
    assert_eq!(s[2], st(Opcode::Ge, index, immediate(&p, 3.0), s[2].c));
    assert_eq!(s[3], st(If, s[2].c, 13, 0));
    for (slot, at) in [(0, 4), (1, 8), (2, 12)] {
        let bound = immediate(&p, f32::from(u8::try_from(slot + 1).unwrap()));
        assert_eq!(s[at], st(Lt, index, bound, s[at].c));
        assert_eq!(s[at + 1], st(IfNot, s[at].c, 3, 0));
        assert_eq!(s[at + 2], st(StoreF, value, a + slot, 0));
        assert_eq!(s[at + 3], st(Return, 0, 0, 0));
    }
    assert_eq!(s[16].a, immediate_string(&p, "bounds check failed\n"));
    assert_eq!(s[17], st(Opcode::Call1, global(&p, "error"), 0, 0));
    assert_eq!(s[18].op, u32::from(Done.number()));
}

// spec FTE-13.11: a vector array's setter takes the element's index, the words divided by 3,
// and stores by STORE_V
#[test]
fn a_vector_arrays_setter_takes_the_elements_index() {
    let p = fte_progs(
        "void(string e) error = #10;\nvector a[2];\nfloat i;\nvoid() t = { a[i] = '1 2 3'; };\n",
    );
    let t = body(&p, "t");
    assert_eq!(
        t[0],
        st(Opcode::MulF, global(&p, "i"), immediate(&p, 3.0), t[0].c)
    );
    assert_eq!(t[1], st(DivF, t[0].c, immediate(&p, 3.0), t[1].c));
    assert_eq!(t[2], st(StoreF, t[1].c, 4, 0));
    assert_eq!(
        t[3],
        st(StoreV, immediate_vector(&p, [1.0, 2.0, 3.0]), 7, 0)
    );
    let f = function(&p, "ArraySet*a");
    let s = body(&p, "ArraySet*a");
    assert!(s.contains(&st(StoreV, f.parm_start + 1, global(&p, "a") + 3, 0)));
}

// spec FTEF-17.17: `autocvar(name, default)` is the global `autocvar_name`, given the default's
// value, with no statement of its own
#[test]
fn autocvar_is_its_global_given_the_default() {
    let p = fte_progs("float x;\nvoid() t = { x = autocvar(sv_x, 4); };\n");
    let cvar = global(&p, "autocvar_sv_x");
    assert_eq!(words(&p, cvar, 1), [4.0_f32.to_bits()]);
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, cvar, global(&p, "x"), 0));
}

// spec FTE-19.11: a shift of two constants folds, each truncated to an int
#[test]
fn a_shift_of_constants_folds() {
    let p = fte_progs("float x;\nvoid() t = { x = 1 << 23; x = 255.9 >> 4; };\n");
    let t = body(&p, "t");
    assert_eq!(
        t[0],
        st(StoreF, immediate(&p, 8_388_608.0), global(&p, "x"), 0)
    );
    assert_eq!(t[1], st(StoreF, immediate(&p, 15.0), global(&p, "x"), 0));
}

// spec FTE-19.11: a shift by a value is floor(a * pow(2, b)) with the program's pow and floor,
// and `>>` divides
#[test]
fn a_shift_by_a_value_is_floor_of_a_power() {
    let p = fte_progs(
        "float(float a, float b) pow = #1;\nfloat(float a) floor = #2;\nfloat x, n;\n\
         void() t = { x = 3 << n; x = 3 >> n; };\n",
    );
    let (pow, floor) = (global(&p, "pow"), global(&p, "floor"));
    let (three, n) = (immediate(&p, 3.0), global(&p, "n"));
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, immediate(&p, 2.0), 4, 0));
    assert_eq!(t[1], st(StoreF, n, 7, 0));
    assert_eq!(t[2], st(Call2, pow, 0, 0));
    assert_eq!(t[3], st(MulF, three, 1, t[3].c));
    assert_eq!(t[4], st(StoreF, t[3].c, 4, 0));
    assert_eq!(t[5], st(Opcode::Call1, floor, 0, 0));
    assert_eq!(t[6], st(StoreF, 1, global(&p, "x"), 0));
    assert_eq!(t[10], st(DivF, three, 1, t[10].c));
}

// spec FTE-19.11: with no pow, a shift by a value calls bitshift, `>>` with the count negated
#[test]
fn a_shift_without_pow_calls_bitshift() {
    let p = fte_progs(
        "float(float a, float b) bitshift = #1;\nfloat x, n;\nvoid() t = { x = 3 >> n; };\n",
    );
    let t = body(&p, "t");
    assert_eq!(t[0], st(SubF, immediate(&p, 0.0), global(&p, "n"), t[0].c));
    assert_eq!(t[1], st(StoreF, immediate(&p, 3.0), 4, 0));
    assert_eq!(t[2], st(StoreF, t[0].c, 7, 0));
    assert_eq!(t[3], st(Call2, global(&p, "bitshift"), 0, 0));
}

// spec FTEF-32.14: in a body a call of a constant FTEQCC folds by its name makes no call
#[test]
fn a_folded_builtin_call_makes_no_call() {
    let p = fte_progs(
        "float(float f) floor = #1;\nfloat(float f) sqrt = #2;\nfloat x;\n\
         void() t = { x = floor(2.5); x = sqrt(x); };\n",
    );
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, immediate(&p, 2.0), global(&p, "x"), 0));
    assert_eq!(t[1], st(StoreV, global(&p, "x"), 4, 0));
}

// spec FTEF-15.11: a wrap definition takes over its function's number, the wrapped body moves
// to `prior*NAME` at the end of the table, and `prior` is a constant of that record's number
#[test]
fn a_wrap_calls_the_body_it_moved() {
    let p = fte_progs(
        "void() a = #1;\nvoid() b = #2;\nvoid() f = { a(); };\n\
         __wrap void() f = { prior(); b(); };\n",
    );
    let number =
        usize::try_from(p.globals[usize::try_from(global(&p, "f")).unwrap()].bits()).unwrap();
    let wrapper = &p.functions[number];
    assert_eq!(string(&p, wrapper.s_name), "f");
    let moved = p.functions.last().unwrap();
    assert_eq!(string(&p, moved.s_name), "prior*f");
    let first = usize::try_from(moved.first_statement).unwrap();
    assert_eq!(
        p.statements[first],
        st(Opcode::Call0, global(&p, "a"), 0, 0)
    );
    let start = usize::try_from(wrapper.first_statement).unwrap();
    let call = p.statements[start];
    assert_eq!(call.op, u32::from(Opcode::Call0.number()));
    let prior = usize::try_from(p.globals[usize::try_from(call.a).unwrap()].bits()).unwrap();
    assert_eq!(prior, p.functions.len() - 1);
    assert_eq!(
        p.statements[start + 1],
        st(Opcode::Call0, global(&p, "b"), 0, 0)
    );
}

// spec FTE-19.12: `v & w` of vectors is BITAND_F of each component into a vector temporary, and
// `v ^ w` is `(v & ~w) | (w & ~v)` with each `~` as SUB_V from '-1 -1 -1'
#[test]
fn vector_bit_operators_work_by_component() {
    let p = fte_progs("vector v, w, r;\nvoid() t = { r = v & w; r = v ^ w; };\n");
    let (v, w) = (global(&p, "v"), global(&p, "w"));
    let t = body(&p, "t");
    let c = t[0].c;
    for i in 0..3 {
        let s = t[usize::try_from(i).unwrap()];
        assert_eq!(s, st(BitAnd, v + i, w + i, c + i));
    }
    assert_eq!(t[3], st(StoreV, c, global(&p, "r"), 0));
    let minus_one = immediate_vector(&p, [-1.0; 3]);
    assert_eq!(t[4], st(SubV, minus_one, w, t[4].c));
    assert_eq!(t[5], st(BitAnd, v, t[4].c, t[5].c));
    assert_eq!(t[8], st(SubV, minus_one, v, t[8].c));
    assert_eq!(t[12].op, u32::from(BitOr.number()));
}

// spec FTEF-18.5: `a = b, c;` stores `b` and then reads `c`, whose value is discarded
#[test]
fn a_sequence_runs_each_expression_in_turn() {
    let p = fte_progs("float a, b, c;\nfloat() f = #1;\nvoid() t = { a = b, f(); };\n");
    let t = body(&p, "t");
    assert_eq!(t[0], st(StoreF, global(&p, "b"), global(&p, "a"), 0));
    assert_eq!(t[1], st(Opcode::Call0, global(&p, "f"), 0, 0));
}

// spec FTE-22.7: with autoprototyping, a definition used before it is read is made in the first
// pass, in its definition's place among the others
#[test]
fn autoprototyping_makes_definitions_in_their_written_order() {
    let p =
        fte_progs("#pragma autoproto\nvoid() t = { later = 1; };\nfloat first;\nfloat later;\n");
    assert!(global(&p, "first") < global(&p, "later"));
}

// spec FTE-13.13: an array writes no definition of its own, one for each element, and an array
// of structs each element's members after it
#[test]
fn an_array_is_written_as_its_elements() {
    let p = fte_progs(
        "typedef struct { string m; float f; } pose;\nstring s[2];\npose poses[2];\nvoid() t = {};\n",
    );
    let names: Vec<String> = p
        .global_defs
        .iter()
        .map(|d| string(&p, d.s_name))
        .filter(|n| n.starts_with('s') || n.starts_with("poses"))
        .collect();
    assert_eq!(
        names,
        [
            "s[0]",
            "s[1]",
            "poses[0]",
            "poses[0].m",
            "poses[0].f",
            "poses[1]",
            "poses[1].m",
            "poses[1].f"
        ]
    );
    let first = p
        .global_defs
        .iter()
        .find(|d| string(&p, d.s_name) == "poses[1]")
        .unwrap();
    assert_eq!(first.type_bits & 0x7fff, 14);
    assert_eq!(
        i32::try_from(first.ofs).unwrap(),
        global(&p, "poses[0]") + 2
    );
}

// spec FTE-18.6: a variadic function first copies the slots after its named parameters into a
// static `__va_list` of 24 vectors, a slot past the eighth from its `$parmN` global
#[test]
fn a_variadic_function_copies_its_slots_to_va_list() {
    let p = fte_progs("float(float a, ...) f = { return a; };\n");
    let s = body(&p, "f");
    let list = global(&p, "__va_list");
    for (i, statement) in (0_i32..).zip(&s[..7]) {
        assert_eq!(*statement, st(StoreV, 7 + 3 * i, list + 3 * i, 0));
    }
    assert_eq!(s[7], st(StoreV, global(&p, "$parm8"), list + 21, 0));
    assert_eq!(s[23], st(StoreV, global(&p, "$parm24"), list + 69, 0));
    assert_eq!(s[24].op, u32::from(Return.number()));
    let elements = p
        .global_defs
        .iter()
        .filter(|d| string(&p, d.s_name).starts_with("__va_list["))
        .count();
    assert_eq!(elements, 24);
}

// spec FTE-13.13: a struct value is written as its members, with no definition of its own
#[test]
fn a_struct_value_is_written_as_its_members() {
    let p = fte_progs("typedef struct { string m; float f; } pose;\npose p;\nvoid() t = {};\n");
    let names: Vec<String> = p
        .global_defs
        .iter()
        .map(|d| string(&p, d.s_name))
        .filter(|n| n.starts_with('p'))
        .collect();
    assert_eq!(names, ["p.m", "p.f"]);
}

// spec FTE-13.14: a parameter of a field type makes no field definition
#[test]
fn a_field_parameter_makes_no_field_definition() {
    let p = fte_progs(".string target;\nvoid(.string field) f = {};\n");
    let fields: Vec<String> = p
        .field_defs
        .iter()
        .skip(1)
        .map(|d| string(&p, d.s_name))
        .collect();
    assert_eq!(fields, ["target"]);
}

// spec QC-11.7: a local vector's components are at its own words, not a field's components of
// the same names
#[test]
fn a_local_vectors_components_are_its_own() {
    let p = fte_progs(
        ".vector velocity;\nvoid() t = { vector velocity; velocity = '1 2 3'; velocity_z = 200; };\n",
    );
    let t = body(&p, "t");
    let local = t[0].b;
    assert_eq!(
        t[0],
        st(StoreV, immediate_vector(&p, [1.0, 2.0, 3.0]), local, 0)
    );
    assert_eq!(t[1], st(StoreF, immediate(&p, 200.0), local + 2, 0));
}
