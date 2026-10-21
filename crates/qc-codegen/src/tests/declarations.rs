// SPDX-License-Identifier: GPL-3.0-or-later

//! Definitions, functions, literals and layout: QC-11.6, QC-11.10, QC-11.11, QC-13.2 to
//! QC-13.10, QC-20, QC-21 and QC-27.12.

use qc_progs::PROGS_DEF_SAVEGLOBAL;
use qc_progs::opcode::Opcode::{AddF, StoreF, StoreS, StoreV};

use super::{body, function, global, immediate, immediate_string, progs, st, string, words};

/// The global definitions named `name`, as (type bits, offset).
fn defs_named(p: &qc_progs::ProgsFile, name: &str) -> Vec<(u32, u32)> {
    p.global_defs
        .iter()
        .filter(|d| string(p, d.s_name) == name)
        .map(|d| (d.type_bits, d.ofs))
        .collect()
}

// spec QC-21.1, QC-21.2, QC-21.3, QC-21.6: definitions from global 28 in the order met, a
// vector taking its components' words; a field's global holds its entity offset; a global is
// saved when it is not initialised, not a function or field, and not local.
#[test]
fn the_layout_of_the_language_vector() {
    let p = progs(
        "float a = 1;\nvector v;\nstring s = \"hi\";\n.float fl;\n.vector fv;\nentity e;\n\
         float end_sys_globals;\n.float end_sys_fields;\n\
         void(float p, vector q) f = { local float l; local vector lv; a = p; };\n",
    );
    assert_eq!(global(&p, "a"), 28);
    assert_eq!(
        [
            global(&p, "v"),
            global(&p, "v_x"),
            global(&p, "v_y"),
            global(&p, "v_z")
        ],
        [29, 29, 30, 31]
    );
    assert_eq!([global(&p, "fl"), global(&p, "fv")], [33, 34]);
    assert_eq!(words(&p, 33, 1), [0], "fl is field 0");
    assert_eq!(words(&p, 34, 4), [1, 1, 2, 3], "fv and its components");
    assert_eq!(global(&p, "end_sys_fields"), 40);
    assert_eq!(words(&p, 40, 1), [4]);
    assert_eq!(p.entityfields, 5);
    let f = function(&p, "f");
    assert_eq!((f.parm_start, f.locals), (42, 8));
    assert_eq!(
        [
            global(&p, "p"),
            global(&p, "q"),
            global(&p, "l"),
            global(&p, "lv")
        ],
        [42, 43, 46, 47]
    );
    let saved = |name: &str| defs_named(&p, name)[0].0 & PROGS_DEF_SAVEGLOBAL != 0;
    assert!(!saved("a"), "initialised");
    assert!(saved("v") && saved("v_x"), "uninitialised globals");
    assert!(!saved("fl") && !saved("f"), "a field and a function");
    assert!(!saved("p") && !saved("l"), "a parameter and a local");
}

// spec QC-21.4: `locals` counts from the first parameter to the end of the last `local`,
// temporaries before it included; with no `local` it is 0.
#[test]
fn locals_end_at_the_last_local_declaration() {
    let p = progs("float a;\nvoid(float p, vector q) f = { a = p + 1; };\n");
    assert_eq!(function(&p, "f").locals, 0);
    let p = progs("float a;\nvoid(float p) f = { a = p + 1; local float l; l = 2; };\n");
    // a 28, f 29, p 30, the literal 1 at 31, the sum at 32, then l.
    assert_eq!(global(&p, "l"), 33, "after the sum's temporary at 32");
    assert_eq!(body(&p, "f")[0], st(AddF, 30, 31, 32));
    let f = function(&p, "f");
    assert_eq!((f.parm_start, f.locals), (30, 4));
}

// spec QC-21.5: a temporary is never reused, not even across statements.
#[test]
fn temporaries_are_never_reused() {
    let p = progs("float a;\nvoid() f = { a = a + a; a = a + a; };\n");
    let s = body(&p, "f");
    assert_eq!(s[0].c, 30);
    assert_eq!(s[2].c, 31);
}

// spec QC-11.6, QC-20.2: an initialised global is a constant: its value is in the global area,
// it is not saved, and a literal of the same value is the constant.
#[test]
fn an_initialised_global_is_a_shared_constant() {
    let p = progs("float CONST = 3;\nfloat a;\nvoid() f = { a = 3; };\n");
    let k = global(&p, "CONST");
    assert_eq!(words(&p, k, 1), [3.0_f32.to_bits()]);
    assert_eq!(body(&p, "f")[0], st(StoreF, k, global(&p, "a"), 0));
    assert_eq!(defs_named(&p, "CONST")[0].0, 2, "a float, not saved");
}

// spec QC-11.10, QC-20.2: a local initialiser is stored once in the global area, and a later
// literal of its value is the local.
#[test]
fn a_local_initialiser_is_a_constant_too() {
    let p = progs("float a;\nvoid() f = { local float l = 5; a = 5; };\n");
    let l = global(&p, "l");
    assert_eq!(words(&p, l, 1), [5.0_f32.to_bits()]);
    assert_eq!(body(&p, "f")[0], st(StoreF, l, global(&p, "a"), 0));
}

// spec QC-11.11: a vector's initialiser does not initialise its components, so a component may
// be initialised over the vector's word.
#[test]
fn a_vector_initialiser_leaves_its_components() {
    let p = progs("vector v = '1 2 3';\nfloat v_x = 2;\n");
    let v = global(&p, "v");
    assert_eq!(words(&p, v, 3), [2.0_f32, 2.0, 3.0].map(f32::to_bits));
    assert_ne!(
        defs_named(&p, "v_y")[0].0 & PROGS_DEF_SAVEGLOBAL,
        0,
        "v_y is saved"
    );
}

// spec QC-20.1, QC-20.3, QC-20.4: a literal is a global named `IMMEDIATE`; `0` and `-0` share
// one, holding the first met.
#[test]
fn zero_and_negative_zero_share_a_global() {
    let p = progs("float a;\nvoid() f = { a = -0; a = 0; };\n");
    let s = body(&p, "f");
    assert_eq!(s[0].a, s[1].a);
    assert_eq!(words(&p, s[0].a, 1), [(-0.0_f32).to_bits()]);
    assert_eq!(defs_named(&p, "IMMEDIATE"), [(2, 30)]);
}

// spec QC-20.2: a vector literal reuses a constant of equal components.
#[test]
fn a_vector_literal_reuses_an_equal_constant() {
    let p = progs("vector V = '1 2 3';\nvector w;\nvoid() f = { w = '1 2 3'; };\n");
    assert_eq!(
        body(&p, "f")[0],
        st(StoreV, global(&p, "V"), global(&p, "w"), 0)
    );
}

// spec QC-20.2: a literal reuses only a constant initialised before it.
#[test]
fn a_later_constant_is_not_reused() {
    let p = progs("float a;\nvoid() f = { a = 4; };\nfloat k = 4;\n");
    assert_ne!(body(&p, "f")[0].a, global(&p, "k"));
}

// spec QC-20.1, QC-20.5: a string literal's text is copied once, when its global is made.
#[test]
fn a_string_literal_is_copied_once() {
    let p = progs("string s;\nvoid() f = { s = \"y\"; s = \"y\"; s = \"a\\nb\\\"c\"; };\n");
    let s = body(&p, "f");
    assert_eq!(s[0], s[1]);
    assert_eq!(s[0].op, u32::from(StoreS.number()));
    let y = immediate_string(&p, "y");
    assert_eq!(s[0].a, y);
    let copies = p.strings.windows(2).filter(|w| w == b"y\0").count();
    assert_eq!(copies, 1);
    assert_eq!(immediate_string(&p, "a\nb\"c"), s[2].a);
}

// spec QC-20.6, QC-27.12, QC-27.18: a string initialiser holds the offset of its own text,
// where id's qcc stores the lexer's stale word; a later literal of the text is the constant.
#[test]
fn a_string_constant_holds_its_own_text() {
    let p = progs("string s1 = \"x\";\nstring s2;\nvoid() f = { s2 = \"x\"; };\n");
    let s1 = global(&p, "s1");
    let ofs = i32::try_from(words(&p, s1, 1)[0]).unwrap();
    assert_eq!(string(&p, ofs), "x");
    assert_eq!(body(&p, "f")[0].a, s1);
}

// spec QC-13.2: the parameters a body uses are named in its defining declaration's type.
#[test]
fn a_body_uses_its_own_parameter_names() {
    let p = progs("float(float x) f;\nfloat(float y) f = { return y; };\n");
    let y = global(&p, "y");
    assert_eq!(body(&p, "f")[0].a, y);
    assert!(defs_named(&p, "x").is_empty());
}

// spec QC-13.3, QC-13.10: `#n` is builtin `n`, `first_statement` `-n`, the literal's float
// value; a builtin makes no parameter or local storage.
#[test]
fn a_builtin_has_a_negative_first_statement() {
    let p = progs("void() o = #7;\nfloat(float a, vector b) q = #8;\nvoid() r = #16777217;\n");
    assert_eq!(function(&p, "o").first_statement, -7);
    let q = function(&p, "q");
    assert_eq!(
        (q.first_statement, q.numparms, q.locals, q.parm_size),
        (-8, 2, 0, [1, 3, 0, 0, 0, 0, 0, 0])
    );
    assert_eq!(q.parm_start, 30, "the next free global");
    assert_eq!(function(&p, "r").first_statement, -16_777_216);
    assert!(defs_named(&p, "a").is_empty());
}

// spec QC-13.9: a function record holds its first statement, parameters, locals, name and
// file; a variadic function's `numparms` is -1.
#[test]
fn a_function_record() {
    let p = progs("void(float p, vector q) f = {};\nvoid(...) g = #1;\n");
    let f = function(&p, "f");
    assert_eq!(f.first_statement, 1);
    assert_eq!((f.numparms, f.parm_size), (2, [1, 3, 0, 0, 0, 0, 0, 0]));
    assert_eq!(string(&p, f.s_name), "f");
    assert_eq!(string(&p, f.s_file), "t.qc");
    assert_eq!(function(&p, "g").numparms, -1);
}

// spec QC-13.5, QC-12.3: a parameter that is an earlier global reuses it; one below the
// previous parameter is "bad parm order".
#[test]
fn parameters_may_reuse_globals() {
    let p = progs("float x;\nvoid(float x) g = { x = 2; };\n");
    let x = global(&p, "x");
    assert_eq!(body(&p, "g")[0].b, x);
    assert_eq!(defs_named(&p, "x").len(), 1);
    assert_eq!(
        super::errors("float b;\nvoid(float a, float b) f = {};\n"),
        ["bad parm order"]
    );
}

// spec QC-21.2, QC-11.7: a local vector's components are its words, and a component declared
// again is the same definition.
#[test]
fn a_local_vectors_components() {
    let p = progs("void() f = { local vector lv = '1 2 3'; local float lv_x; lv_x = 4; };\n");
    let lv = global(&p, "lv");
    assert_eq!(global(&p, "lv_x"), lv);
    assert_eq!(body(&p, "f")[0].b, lv);
    assert_eq!(defs_named(&p, "lv_x").len(), 1);
    assert_eq!(immediate(&p, 4.0), lv + 3);
}
