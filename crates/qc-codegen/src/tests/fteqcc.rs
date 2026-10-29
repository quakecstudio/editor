// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's constructs on the vanilla opcodes: `docs/spec/dialect-fteqcc-rerelease.md`. The
//! expected statements follow FTEQCC's lowering as each statement quotes it; a temporary is
//! read from the statement that made it.

use qc_progs::opcode::Opcode::{
    self, AddF, BitAnd, BitOr, Call1, Call2, Call3, DivF, Done, If, IfNot, Lt, MulF, MulVf, Return,
    StoreF, StoreS, StoreV, SubF,
};
use qc_progs::{ProgsFile, Statement};

use super::{
    body, compile_fteqcc, fte_progs, function, global, immediate, immediate_string, st, string,
    words,
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

// spec FTE-13.5: an array takes its elements' words in turn, written from its aggregate
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

// spec FTE-13.7: a subscript by a value calls the array's getter with the index
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

// spec FTE-13.7: a struct element's member is read at the index times the element's words
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

// spec FTE-13.7: a vector array's index in words is divided by 3 for the getter
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

// spec FTE-13.7: a store through a subscript by a value is not lowered
#[test]
fn a_store_through_a_subscript_by_a_value_is_not_lowered() {
    let result = compile_fteqcc("float f[3];\nvoid(float i) t = { f[i] = 1; };\n");
    assert!(
        matches!(
            result,
            Err(CompileError::Internal {
                internal: Internal::Unsupported,
                ..
            })
        ),
        "{result:?}"
    );
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
    assert_eq!(t[4], st(BitOr, t[1].c, t[3].c, t[4].c));
    assert_eq!(t[5], st(StoreF, t[4].c, c, 0));
    assert_eq!(
        opcodes(&t[6..]),
        ops(&[SubF, BitAnd, SubF, BitAnd, BitOr, StoreF, Done])
    );
    assert_eq!(t[11].b, a);
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
    assert_eq!(t[3], st(SubF, a, t[2].c, t[3].c));
    assert_eq!(t[4], st(StoreF, t[3].c, global(&p, "c"), 0));
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
    assert_eq!(t[1], st(MulVf, global(&p, "v"), t[0].c, t[1].c));
    assert_eq!(t[2], st(StoreV, t[1].c, global(&p, "r"), 0));
}

// spec FTE-2.1: a `for` loop's declaration, its condition tested by IFNOT past the loop, its
// step, and GOTO back to the condition
#[test]
fn a_for_loop_tests_its_condition() {
    let p = fte_progs("float n;\nvoid() t = { for (float i = 0; i < 3; i++) n = i; };\n");
    let t = body(&p, "t");
    assert_eq!(
        opcodes(&t),
        ops(&[
            StoreF,
            Lt,
            IfNot,
            StoreF,
            StoreF,
            AddF,
            StoreF,
            Opcode::Goto,
            Done
        ])
    );
    assert_eq!(t[2].b, 6);
    assert_eq!(t[7].a, -6);
}
