// SPDX-License-Identifier: GPL-3.0-or-later

//! The programs of the compiler differential: each construct of `docs/spec/language.md` QC-13
//! to QC-26 that a program compiles to, and each expression error of QCD-4, with the
//! divergences of QC-27 that `qc-codegen` makes on purpose.

use std::borrow::Cow;

use super::{Case, Divergence};

/// A case of one file, `t.qc`.
const fn one(name: &'static str, source: &'static str) -> Case {
    Case {
        name: Cow::Borrowed(name),
        files: &[],
        source: Cow::Borrowed(source),
        divergence: None,
    }
}

/// A case `qc-codegen` compiles where id's `qcc` does otherwise, as spec statement `statement`
/// says.
const fn diverges(name: &'static str, source: &'static str, statement: &'static str) -> Case {
    refused_by_us(name, source, statement, None)
}

/// A case `qc-codegen` compiles otherwise than id's `qcc`, as `statement` says: refusing it with
/// `message`, or with `None` compiling it.
const fn refused_by_us(
    name: &'static str,
    source: &'static str,
    statement: &'static str,
    message: Option<&'static str>,
) -> Case {
    Case {
        name: Cow::Borrowed(name),
        files: &[],
        source: Cow::Borrowed(source),
        divergence: Some(Divergence {
            statement,
            ours: message,
        }),
    }
}

/// Every case.
pub(crate) const CASES: &[Case] = &[
    // QC-13: function definitions.
    one("empty-body", "void() f = {};\n"),
    one(
        "builtins",
        "void() o = #7;\nfloat(float a, vector b) p = #8;\nvoid(...) q = #9;\n",
    ),
    one(
        "builtin-rounded",
        "void() f = #16777217;\nvoid() g = #123456789;\n",
    ),
    one(
        "parameters-and-locals",
        "float a;\nvoid(float p, vector q) f = { local float l; local vector lv; a = p; };\n",
    ),
    one(
        "parameters-no-locals",
        "float a;\nvoid(float p, vector q) f = { a = p + 1; };\n",
    ),
    one(
        "temporaries-before-a-local",
        "float a;\nvoid(float p) f = { a = p + 1; local float l; l = 2; };\n",
    ),
    one(
        "return-unchecked",
        "float() f = { return \"x\"; };\nfloat() g = { return; };\n",
    ),
    one(
        "state",
        "void() th2;\n$frame s1 s2\nvoid() th1 = [$s2, th2] { };\nvoid() th2 = {};\n",
    ),
    one(
        "state-declares-next",
        "$frame s1\nvoid() th1 = [$s1, th3] { };\nvoid() th3 = {};\n",
    ),
    one(
        "state-with-parameters",
        "void() th2;\n$frame a b c\nvoid(float x) th = [$c, th2] { local float y; y = x; };\n\
         void() th2 = {};\n",
    ),
    one(
        "function-order",
        "void() b;\nvoid() a = { b(); };\nvoid() b = { a(); };\nvoid() c = #3;\n",
    ),
    one(
        "bad-parm-order",
        "float b;\nvoid(float a, float b) f = {};\n",
    ),
    // QC-14: statements.
    one(
        "if-else",
        "float a, b;\nvoid() f = { if (a) b = 1; else b = 2; };\n",
    ),
    one(
        "if-if-else",
        "float a, b, c;\nvoid() f = { if (a) if (b) c = 1; else c = 2; };\n",
    ),
    one("while", "float a;\nvoid() f = { while (a) a = a - 1; };\n"),
    one(
        "do-while",
        "float a;\nvoid() f = { do a = a + 1; while (a < 3); };\n",
    ),
    one(
        "nested-blocks",
        "float a;\nvoid() f = { { a = 1; { a = 2; } } while (a) { a = a - 1; if (a) { a = 0; } } };\n",
    ),
    one(
        "expression-statement",
        "float a;\nvoid() f = { a; a + 1; };\n",
    ),
    // QC-15: expressions and precedence.
    one(
        "precedence",
        "float a, b, c;\nvoid() f = { a = b + c * a - b / c; a = b & c | 4; a = b && c; \
         a && b = c; a = a == b == c; a = !b == c; a = !b + c; a = b || c && a; };\n",
    ),
    one(
        "field-chain",
        ".entity en;\n.float hp;\nentity e;\nfloat a;\nvoid() f = { a = e.en.hp; e.en.hp = 3; };\n",
    ),
    // QC-16: operator types.
    one(
        "vector-operators",
        "float a;\nvector v;\nvoid() f = { a = v * v; v = v * a; v = a * v; v = v + v; v = v - v; };\n",
    ),
    one(
        "equality-of-each-type",
        "float a;\nstring s;\nentity e, w;\nvector v;\nvoid() g;\n\
         void() f = { a = s == \"x\"; a = e != w; a = v == v; a = g == g; a = s != s; \
         a = v != v; a = g != g; a = e == w; a = !s; a = !e; a = !v; a = !g; a = !a; };\n\
         void() g = {};\n",
    ),
    one(
        "comparisons",
        "float a, b;\nvoid() f = { a = a < b; a = a > b; a = a <= b; a = a >= b; };\n",
    ),
    one(
        "functions-of-two-types",
        "void() g;\nfloat(float x) h;\nfloat r;\nvoid() f = { r = g == h; };\n\
         void() g = {};\nfloat(float x) h = { return x; };\n",
    ),
    one(
        "type-mismatch-div",
        "vector v;\nvoid() f = { v = v / 2; };\n",
    ),
    one(
        "type-mismatch-lt",
        "float a;\nvector v;\nvoid() f = { a = v < v; };\n",
    ),
    one(
        "type-mismatch-field-eq",
        ".float fl;\nfloat a;\nvoid() f = { a = fl == fl; };\n",
    ),
    one(
        "type-mismatch-and",
        "entity e;\nfloat a;\nvoid() f = { if (e && a) a = 1; };\n",
    ),
    one(
        "type-mismatch-not",
        ".float fl;\nfloat a;\nvoid() f = { a = !fl; };\n",
    ),
    one(
        "type-mismatch-dot",
        "float a;\nentity e;\nvoid() f = { a = e.a; };\n",
    ),
    one(
        "type-mismatch-dot-entity",
        ".float fl;\nfloat a;\nvoid() f = { a = a.fl; };\n",
    ),
    // QC-17: assignment.
    one(
        "assignment-chain",
        "float a, b, c;\nvoid() f = { a = b = c; };\n",
    ),
    one(
        "field-stores",
        "float end_sys_globals;\n.vector o;\nentity e;\nfloat a;\nvoid() f = { a = e.o_z; e.o_x = 1; e.o = '1 2 3'; \
         (e.o_y) = a; };\n",
    ),
    one(
        "field-of-another-type",
        ".float fa;\n.vector fb;\nvoid() f = { fa = fb; };\n",
    ),
    one(
        "field-of-field",
        "..float ff;\n.float g;\nentity e;\nvoid() f = { e.ff = g; };\n",
    ),
    one(
        "type-mismatch-store",
        "float a;\nvoid() f = { a = \"x\"; };\n",
    ),
    one(
        "type-mismatch-store-field",
        ".vector fv;\nentity e;\nvoid() f = { e.fv = 1; };\n",
    ),
    refused_by_us(
        "assign-to-literal",
        "float a;\nvoid() f = { 5 = a; };\n",
        "QC-27.10",
        Some("cannot assign to this expression"),
    ),
    diverges(
        "earlier-load-rewritten",
        ".float fl;\nentity e;\nfloat a;\nvoid() f = { e.fl; a = 2; };\n",
        "QC-27.11",
    ),
    // QC-18: function calls.
    one(
        "calls-share-slots",
        "float(float p, float q) g;\nfloat(float p) h;\nfloat a;\n\
         void() f = { a = g(4, h(1)); a = g(1, 2) + h(3); };\n\
         float(float p, float q) g = { return p; };\nfloat(float p) h = { return p; };\n",
    ),
    one(
        "variadic",
        "float end_sys_globals;\nvoid(...) vf = #1;\nvoid() f = { vf(1, \"x\", '1 2 3'); vf(); vf(1, 2, 3, 4, 5, 6, 7, 8); };\n",
    ),
    one(
        "call-through-field",
        ".void() th;\nentity e;\nvoid() f = { e.th(); };\n",
    ),
    one(
        "no-arguments-to-a-fixed-list",
        "float(float a, float b) g = #2;\nvoid() f = { g(); };\n",
    ),
    one(
        "return-slot-retyped",
        "float end_sys_globals;\nfloat() g = { return 1; };\nvector() h = { return '1 2 3'; };\nvector v;\nfloat a;\n\
         void() f = { v = g() + h(); a = h() * g(); };\n",
    ),
    one(
        "too-many-parameters",
        "void(float a) g = #1;\nvoid() f = { g(1, 2); };\n",
    ),
    one(
        "too-few-parameters",
        "void(float a, float b) g = #1;\nvoid() f = { g(1); };\n",
    ),
    one(
        "parm-type-mismatch",
        "void(float a) g = #1;\nvoid() f = { g(\"x\"); };\n",
    ),
    one("not-a-function", "float a;\nvoid() f = { a(); };\n"),
    one(
        "more-than-eight",
        "void(...) g = #1;\nvoid() f = { g(1, 2, 3, 4, 5, 6, 7, 8, 9); };\n",
    ),
    one(
        "field-component-argument",
        ".vector fv;\nvoid(.float x) g = #1;\nvoid() f = { g(fv_x); };\n",
    ),
    // QC-19: conditions.
    one(
        "vector-condition",
        "vector v;\nfloat a;\nvoid() f = { if (v) a = 1; };\n",
    ),
    one(
        "conditions-of-each-type",
        "string s;\nentity e;\nvoid() g;\nfloat a;\n\
         void() f = { if (s) a = 1; while (e) a = 2; do a = 3; while (g); };\nvoid() g = {};\n",
    ),
    // QC-20: literals and constants.
    one(
        "literal-reuses-constant",
        "float CONST = 3;\nfloat a;\nvoid() f = { a = 3; a = -0; a = 0; };\n",
    ),
    one(
        "zero-and-negative-zero",
        "float a;\nvoid() f = { a = 0; a = -0; };\n",
    ),
    one(
        "vector-literal-reuse",
        "vector V = '1 2 3';\nvector w;\nvoid() f = { w = '1 2 3'; };\n",
    ),
    one(
        "string-literals",
        "string s2;\nvoid() f = { s2 = \"x\"; s2 = \"y\"; s2 = \"y\"; s2 = \"a\\nb\\\"c\"; };\n",
    ),
    one(
        "local-initialiser-reused",
        "float a;\nvoid() f = { local float l = 5; a = 5; };\n",
    ),
    one(
        "later-constant-not-reused",
        "float a;\nfloat k;\nvoid() f = { a = 4; };\nfloat k2 = 4;\nvoid() g = { a = 4; };\n",
    ),
    diverges(
        "string-constant",
        "string s1 = \"x\";\nstring s2;\nvoid() f = { s2 = \"x\"; };\n",
        "QC-27.12",
    ),
    // QC-21: global and field layout.
    one(
        "layout",
        "float a = 1;\nvector v;\n.float fl;\n.vector fv;\nentity e;\nfloat end_sys_globals;\n\
         .float end_sys_fields;\nvoid(float p, vector q) f = { local float l; local vector lv; \
         a = p; };\n",
    ),
    one(
        "vector-initialiser-and-component",
        "vector v = '1 2 3';\nfloat v_x = 2;\nfloat a;\nvoid() f = { a = v_y; };\n",
    ),
    one(
        "vector-field-components",
        ".vector fv;\nentity e;\nfloat a;\nvoid() f = { a = e.fv_y; e.fv_z = a; };\n",
    ),
    one(
        "locals-reuse-globals",
        "float x;\nvoid() f = { local float x; x = 1; };\nvoid(float x) g = { x = 2; };\n",
    ),
    one(
        "local-vector",
        "void() f = { local vector lv = '1 2 3'; local float lv_x; lv_x = 4; };\n",
    ),
    one(
        "void-and-field-of-field",
        "void x;\n..float ff;\n.float g;\n",
    ),
    one(
        "field-of-void",
        ".void vf;\nentity e;\nfloat r;\nvoid() f = { r = e.vf; };\n",
    ),
    one(
        "two-functions-one-type",
        "float(float x) a = { return x; }, b = { return x + 2; };\n\
         float(float y) c = { return 1; }, d = { return 2; };\nvoid() f = { a(1); d(5); };\n",
    ),
    // QC-25: program output.
    one(
        "progdefs",
        "float time;\nvector origin;\nentity self;\nstring mapname;\nvoid() main;\n\
         float end_sys_globals;\n.float health;\n.vector angles;\n.string model;\n.entity owner;\n\
         .void() think;\n.float end_sys_fields;\nvoid() main = {};\n",
    ),
    diverges(
        "vector-literal-before-end-sys-globals",
        "vector v;\nvoid() f = { v = '1 2 3'; };\nfloat end_sys_globals;\n",
        "QC-27.13",
    ),
    diverges(
        "vector-literal-then-three-definitions",
        "vector v;\nvoid() f = { v = '1 2 3'; };\nfloat x, y, z;\n",
        "QC-27.13",
    ),
    Case {
        name: Cow::Borrowed("two-files"),
        files: &[(
            "b.qc",
            "float b;\nvoid() g = { b = \"two\" == \"two\"; };\n",
        )],
        source: Cow::Borrowed("float a;\nvoid() f = { a = 1; };\n"),
        divergence: None,
    },
    // QC-26: precache lists, which do not reach the progs.
    one(
        "precache-calls",
        "void(string s) precache_sound = #19;\nvoid(string s) precache_sound2 = #19;\n\
         void(string s) precache_model = #20;\nvoid(string s) precache_file = #68;\nstring t;\n\
         void() f = { precache_sound(\"a.wav\"); precache_sound(\"a.wav\"); \
         precache_sound2(\"b.wav\"); precache_model(\"m.mdl\"); precache_file(t); };\n",
    ),
];
