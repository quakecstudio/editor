// SPDX-License-Identifier: GPL-3.0-or-later

//! The flag family of the builtin differential (P2-121): programs whose outcome a flag that
//! changes in the middle of a file decides (`docs/spec/dialect-fteqcc.md` FTEF-21): the flags an
//! `extern "C"` or `extern "QC"` block sets and restores, the operator priorities a `#pragma flag`
//! that names `cpriority` sets, `qcfuncs`, `assumevar`, `subscope`, `attributes` and
//! `assumeint` as `#pragma flag` and the command line set them, and the defaults `#pragma
//! noref`, `nosave` and `defaultstatic` set. Each program binds `f` first, so `f` is bound only
//! when FTEQCC compiled the rest. Programs whose outcome a warning decides are compiled with
//! FTEQCC's `-Werror` and read with every warning of the profile an error.

use qc_syntax::Dialect;

use super::directives::BIND;
use super::{Case, WERROR_OPTIONS};

/// Programs compiled with no option.
const FLAG_PROGRAMS: &[&str] = &[
    // FTEF-21.17: `"C"` lexes its strings with `qcfuncs` off, so `\9` is "Bad character code";
    // `"QC"` with it on.
    "extern \"C\" { string s = \"\\9\"; }\n",
    "extern \"QC\" { string s = \"\\9\"; }\n",
    "#pragma flag disable qcfuncs\nextern \"QC\" { string s = \"\\9\"; }\n",
    "string s = \"\\9\";\n",
    // FTEF-21.3: the flags come back at the block's end, after the token that follows it.
    "#pragma flag disable qcfuncs\nextern \"QC\" { float y; }\nstring s = \"\\9\";\n",
    "extern \"C\" { float y; } string s = \"\\9\";\n",
    "extern \"C\" { float y; }\nstring s = \"\\9\";\n",
    "extern \"C\" float y; string s = \"\\9\";\n",
    "extern \"C\" {\n#pragma flag enable qcfuncs\n}\nstring s = \"\\9\";\n",
    "extern \"C\" {\n#pragma flag enable qcfuncs\nstring s = \"\\9\";\n}\n",
    "extern \"C\" {\n#pragma flag enable utf8\n}\nstring s = \"\\xff\";\n",
    // FTEF-21.17, FTEF-21.11: `"C"` turns `assumevar` on, in any case, its string's parts
    // joined; any other string sets nothing.
    "extern \"C\" { float x = 5; float y = x; }\n",
    "extern \"C\" float x = 5;\nfloat y = x;\n",
    "extern \"c\" { float x = 5; float y = x; }\n",
    "extern \"\" \"C\" { float x = 5; float y = x; }\n",
    "extern \"C++\" { float x = 5; float y = x; }\n",
    "extern \"QC\" { float x = 5; float y = x; }\n",
    "extern \"C\" { float x = 5; }\nfloat y = x;\n",
    "extern \"C\" { float x = 5; }\nfloat x = 6;\n",
    "extern \"C\" { extern \"QC\" { float x = 5; float y = x; } }\n",
    "extern \"C\" { extern \"QC\" { float x = 5; } float y = x; }\n",
    "#pragma flag enable assumevar\nfloat x = 5;\nfloat y = x;\n",
    "#pragma flag enable assumevar\nconst float x = 5;\nfloat y = x;\n",
    "float x = 5;\nfloat y = x;\n",
    // FTEF-21.17: the block's forms: `{ }`, one definition after any `;`, `extern` first only,
    // an unended block, and a block of locals in a body.
    "extern \"C\" {\n",
    "extern \"C\" ;\nfloat x = 5;\nfloat y = x;\n",
    "const extern \"C\" { float x = 5; }\n",
    "extern \"C\" { }\n",
    "void() g = { extern \"C\" { float y = 1; } y = 2; };\n",
    "void() g = { local extern \"C\" { float y = 1; } y = 2; };\n",
    "void() g = { extern \"C\" { y = 2; } };\n",
    // FTEF-21.12: `"C"`, and `subscope`, end a block's locals with it.
    "extern \"C\" { void g() = { { float x; } x = 1; }; }\n",
    "void() g = { { float x; } x = 1; };\n",
    "#pragma flag enable subscope\nvoid() g = { { float x; } x = 1; };\n",
    "#pragma flag enable subscope\nvoid() g = { float x; string x; x = \"a\"; };\n",
    "void() g = { float x; string x; };\n",
    "#pragma flag enable subscope\nvoid() g = { float x; string x; };\n",
    "#pragma flag enable subscope\nvoid() g = { float x; { float x; } x = 1; };\n",
    "void() g = { { float x;\n#pragma flag enable subscope\n} x = 1; };\n",
    "#pragma flag enable subscope\nvoid() g = { { float x;\n#pragma flag disable subscope\n} x = 1; };\n",
    "#pragma flag enable subscope\nvoid() g = { for (float i = 0;;) {} i = 1; };\n",
    "void() g = { for (float i = 0;;) {} i = 1; };\n",
    // FTEF-21.7: with `qcfuncs` off a declaration's `*` is each declarator's.
    "extern \"C\" { float *a, b = 1.5; }\n",
    "float *a, b = 1.5;\n",
    "#pragma flag disable qcfuncs\nfloat *a, b = 1.5;\n",
    "extern \"C\" { .void() think; }\n",
    ".void() think;\n",
    "extern \"C\" { void(float() cb) g; }\n",
    "void(float() cb) g = #2;\n",
    "extern \"C\" { class C { float x; }; }\n",
    "extern \"C\" { void(float a) g; }\nvoid(float a) g = #2;\n",
    "extern \"C\" { float (*fp)(float); }\nfloat(float) fp;\n",
    "extern \"C\" { float (*fp)(float) = 0; }\n",
    "float (*fp)(float);\n",
    "float a, (*fp)(float);\n",
    "extern \"C\" { typedef void (*cb)(float); }\n",
    "extern \"C\" { typedef float *fp; fp a; }\nfloat *a;\n",
    "float a, (float x) g;\nfloat(float x) g = #2;\n",
    // FTEF-21.8: `()` is variadic, a first parameter with no type an int, an array a pointer.
    "extern \"C\" { void h(); }\nvoid h(float x) = #1;\n",
    "extern \"C\" { void h(); }\nvoid(...) h = #1;\n",
    "void h(float x) = #1;\nextern \"C\" { void h(); }\n",
    "void h(float x) = #1;\nvoid h();\n",
    "extern \"C\" { void h(x) = #1; }\nvoid h(int a) = #1;\n",
    "extern \"C\" { void h(x, y) = #1; }\n",
    "extern \"C\" { void h(float a[2]); }\nvoid h(float *a) = #1;\n",
    "void h(float a[]);\nvoid h(float *a) = #1;\n",
    // FTEF-21.10: with `qcfuncs` off a function declared in a body is a global, and an `extern`
    // on a type that is no function's is dropped without a word.
    "void() g = { void h(); };\n",
    "extern \"C\" { void g() = { void h(); }; }\n",
    "extern \"C\" { void g() = { float h(); }; float h() = { return 1; }; }\n",
    "extern \"C\" { extern void h(); }\n",
    "extern void h(float x);\n",
    "extern void h(float x);\nvoid h(float x) = {};\n",
    "extern void(float x) h;\nvoid(float x) h = {};\n",
    // FTEF-21.9: attributes after a qualifier, and at file level with `attributes` on.
    "float y;\nvar [[alias(y)]] float x;\n",
    "float y;\nvar [[alias(\"y\")]] float x;\n",
    "var [[alias(y)]] float x;\n",
    "float y;\nvar [[alias(y)]] float x = 1;\n",
    "float y;\nvar [[alias(y)]] string x;\n",
    "[[eraseable]] float x;\n",
    "#pragma flag enable attributes\n[[eraseable]] float x;\n",
    "const [[]] float x;\n",
    "const [] float x;\n",
    "const [[__deprecated(5)]] float x;\n",
    "const [[__deprecated(\"a\")]] float x = 1;\n",
    "void() g = { local [[eraseable]] float x; };\n",
    "void() g = { static [[eraseable]] float x; };\n",
    "#pragma flag enable attributes\nvoid() g = { [[eraseable]] float x; };\n",
    "const [[accumulate]] void() h = { };\nconst [[accumulate]] void() h = { };\n",
    "void() h = { };\nvoid() h = { };\n",
    // FTEF-21.13: `assumeint` makes an enum's names integers.
    "#pragma flag enable assumeint\nenum { A, B };\nstring s = B;\n",
    "enum { A, B };\nstring s = B;\n",
    "extern \"C\" { enum { A, B }; }\nstring s = B;\n",
    // FTEF-17.13: a pointer is compared by what it points to.
    "float a;\nextern \"C\" { float *a; }\n",
    "extern \"C\" { float *a; }\nfloat a;\n",
    "extern \"C\" { float *a; }\nextern \"C\" { float *a; }\n",
    "extern \"C\" { float *a; }\nextern \"C\" { string *a; }\n",
    "extern \"C\" { float **a; }\nextern \"C\" { float *a; }\n",
    "extern \"C\" { float **a; }\nextern \"C\" { float **a; }\n",
    "typedef float t;\nextern \"C\" { float *a; }\nextern \"C\" { t *a; }\n",
    "extern \"C\" { float *a; }\nextern \"C\" { vector *a; }\n",
    "extern \"C\" { float *a; }\nentity a;\n",
    "void h(float a[2]);\nvoid h(float *a) = #1;\n",
    "void h(float a[]);\nvoid h(float *a) = #1;\n",
    "extern \"C\" { void h(float a[2]); }\nvoid h(float *a) = #1;\n",
    "float *p;\nentity p;\n",
    "extern \"C\" { .float *a; }\nextern \"C\" { .float *a; }\n",
    "extern \"C\" { .float *a; }\nextern \"C\" { .string *a; }\n",
    "extern \"C\" { void (*fp)(float); }\nextern \"C\" { void (*fp)(float); }\n",
    "extern \"C\" { void (*fp)(float); }\nextern \"C\" { void (*fp)(string); }\n",
    "extern \"C\" { void (*fp)(float); }\nvoid(float) fp;\n",
    "extern \"C\" { float *a; }\nextern \"C\" { float a; }\n",
    "float *a;\nfloat a;\n",
    "#pragma flag disable qcfuncs\nfloat *a;\nfloat a;\n",
    "#pragma flag disable qcfuncs\nfloat *a, b;\nfloat b;\n",
    "#pragma flag disable qcfuncs\nfloat *a, b;\nfloat *b;\n",
    // FTEF-21.14: a typedef's `static`, `noref` and `nosave` are no error when a pragma makes
    // them the default.
    "#pragma defaultstatic 1\ntypedef float t;\n",
    "static typedef float t;\n",
    "#pragma defaultstatic 1\ntypedef static float t;\n",
    "#pragma noref 1\nnoref typedef float t;\n",
    "noref typedef float t;\n",
    "#pragma nosave 1\nnosave typedef float t;\n",
    "nosave typedef float t;\n",
    "typedef static x;\n",
    "typedef x;\nx y;\n",
    "#pragma defaultstatic 1\nvoid() g = { float x = 1; float x = 2; };\n",
];

/// Programs whose outcome a warning decides, compiled with every warning an error. Each
/// global is referenced, so that it has no "no references" warning.
const FLAG_WARNINGS: &[&str] = &[
    // FTEF-21.6: `!` and `~` warn of each operator their operand takes, by the priorities.
    "float a, b;\nfloat() h = { return !a == b; };\n",
    "float a, b;\nfloat() h = { return !a && b; };\n",
    "float a, b;\nfloat() h = { return !a * b; };\n",
    "float a, b;\nfloat() h = { return ~a & b; };\n",
    "float a, b;\nfloat() h = { return !(a) == b; };\n",
    "float a, b;\nfloat() h = { return !(a == b); };\n",
    "float a, b;\nfloat() h = { return -a == b; };\n",
    // FTEF-21.4: a pragma that names `cpriority` sets the priorities; an `extern "C"` block
    // sets only the flag.
    "#pragma flag enable cpriority\nfloat a, b;\nfloat() h = { return !a == b; };\n",
    "#pragma flag enable cpriority\nfloat a, b;\nfloat() h = { return ~a & b; };\n",
    "extern \"C\" { float a, b; float() h = { return !a == b; }; }\n",
    "extern \"C\" {\n#pragma flag enable cpriority\n}\nfloat a, b;\nfloat() h = { return !a == b; };\n",
    "#pragma flag enable cpriority\n#pragma flag disable cpriority\nfloat a, b;\n\
     float() h = { return !a == b; };\n",
    "#pragma flag enable cpriority\nfloat a, b, c;\nfloat() h = { return a ? b : c; };\n",
    // FTEF-21.9: an unknown attribute, and `accumulate` twice.
    "#pragma flag enable attributes\n[[foo]] float x;\nfloat() h = { return x; };\n",
    "#pragma flag enable attributes\n[[foo(1, 2)]] float x;\nfloat() h = { return x; };\n",
    "const [[accumulate, accumulate]] void() h = { };\nvoid() k = { h(); };\n",
    "const __accumulate [[accumulate]] void() h = { };\nvoid() k = { h(); };\n",
    // FTEF-21.10: the warning for `extern` on what is no function, with `qcfuncs` on only.
    "extern float x;\nfloat() h = { return x; };\n",
    "extern \"C\" { extern float x; }\nfloat() h = { return x; };\n",
    // FTEF-21.14: a typedef under `defaultstatic` has two storage classes.
    "#pragma defaultstatic 1\ntypedef float t;\n",
    // FTEF-21.7: C's function pointer is a variable, which a declaration without it makes
    // constant.
    "extern \"C\" { float (*fp)(float); }\nfloat(float) fp;\nfloat() h = { return fp(1); };\n",
];

/// Programs with the command-line options they are compiled with (FTEF-14.72).
const FLAG_OPTIONS: &[(&[&str], &str)] = &[
    (
        &["-Fcpriority", "-Werror", "-Wno-Q208"],
        "float a, b;\nfloat() h = { return !a == b; };\n",
    ),
    (&["-Fno-qcfuncs"], "float *a, b = 1.5;\n"),
    (&["-Fassumevar"], "float x = 5;\nfloat y = x;\n"),
    (&["-Fsubscope"], "void() g = { { float x; } x = 1; };\n"),
    (&["-Fattributes"], "[[eraseable]] float x;\n"),
    (
        &["-Fcpriority", "-Werror", "-Wno-Q208"],
        "#pragma flag disable cpriority\nfloat a, b;\nfloat() h = { return !a == b; };\n",
    ),
];

/// Every case of the family, `f` bound before each program.
pub(super) fn cases() -> Vec<Case> {
    let program = |source: &str| format!("{BIND}{source}");
    let mut cases: Vec<Case> = FLAG_PROGRAMS
        .iter()
        .map(|source| Case::new(Dialect::Fteqcc, program(source)))
        .collect();
    cases.extend(FLAG_WARNINGS.iter().map(|source| Case {
        options: WERROR_OPTIONS.map(str::to_owned).to_vec(),
        ..Case::new(Dialect::Fteqcc, program(source))
    }));
    for (options, source) in FLAG_OPTIONS {
        cases.push(Case {
            options: options.iter().map(|&o| o.to_owned()).collect(),
            ..Case::new(Dialect::Fteqcc, program(source))
        });
    }
    cases
}
