// SPDX-License-Identifier: GPL-3.0-or-later

//! The cast family of the builtin differential (P2-122): programs whose outcome the flags and the
//! target the semantic model's casts and conversions read decide (`docs/spec/dialect-fteqcc.md`
//! FTEF-22): `lax`, `qccx`, `typeexplicit` and `flag_undefwordsize` as `#pragma flag`, `#pragma
//! target` and the command line set them, the `extern` check of the target, and the pointer casts
//! of FTEF-19.31. Each program binds `f` first, so `f` is bound only when FTEQCC compiled the
//! rest. Programs whose outcome a warning decides are compiled with FTEQCC's `-Werror` and read
//! with every warning of the profile an error.

use qc_syntax::Dialect;

use super::directives::BIND;
use super::{Case, WERROR_OPTIONS};

/// Programs compiled with no option.
const CAST_PROGRAMS: &[&str] = &[
    // FTEF-22.1: `lax` makes a refused cast a warning, where the value stands.
    "#pragma flag enable lax\nfloat x = \"a\";\n",
    "#pragma flag enable lax\nvoid() g = 5;\n",
    "#pragma flag enable lax\n.float fl = 1;\n",
    "#pragma flag enable lax\nstring s = '1 2 3';\n",
    "#pragma flag enable lax\nvoid() g = { float x = \"a\"; };\n",
    "#pragma flag enable lax\nfloat(float a = \"a\") g = #2;\n",
    "float(float a = \"a\") g = #2;\n",
    "#pragma flag enable lax\nenum class E : float { A };\nfloat x = E::A;\n",
    "enum class E : float { A };\nfloat x = E::A;\n",
    "#pragma flag enable lax\nfloat x = \"a\";\n#pragma flag disable lax\nfloat y = \"a\";\n",
    "#pragma flag enable lax\n#pragma flag disable lax\nfloat y = \"a\";\n",
    "extern \"C\" {\n#pragma flag enable lax\n}\nfloat x = \"a\";\n",
    // FTEF-22.6: the flags at the token after the value.
    "float x =\n#pragma flag enable lax\n\"a\";\n",
    "float x = \"a\"\n#pragma flag enable lax\n;\n",
    // FTEF-22.2: `qccx` casts a float to an entity or a string and back.
    "#pragma flag enable qccx\nfloat x = \"a\";\n",
    "#pragma flag enable qccx\nentity e = 1;\n",
    "#pragma flag enable qccx\nentity e = 1.5;\n",
    "#pragma flag enable qccx\nstring s = 1;\n",
    "#pragma flag enable qccx\nentity e = 1d;\n",
    "#pragma flag enable qccx\nvector v = \"a\";\n",
    "#pragma flag enable qccx\nvoid() g = { entity e; float x = e; };\n",
    "void() g = { entity e; float x = e; };\n",
    "#pragma flag enable qccx\nfloat(float a = \"a\") g = #2;\n",
    // FTEF-22.3: `typeexplicit` and `lax` in an enum's values and a state's frames.
    "#pragma flag enable typeexplicit\nenum { A = 1i };\n",
    "#pragma flag enable typeexplicit\nenum { A = 0i };\n",
    "#pragma flag enable typeexplicit\nenum { A = 1 };\n",
    "#pragma flag enable typeexplicit\nenum { A = '1 2 3' };\n",
    "#pragma flag enable typeexplicit\n#pragma flag enable assumeint\nenum { A = 1.5 };\n",
    "#pragma flag enable typeexplicit\nfloat x = 1i;\n",
    "#pragma flag enable typeexplicit\nvoid() g = [1i, g] {};\n",
    "void() g = [1i, g] {};\n",
    "void() g = [\"a\", g] {};\n",
    "#pragma flag enable lax\nvoid() g = [\"a\", g] {};\n",
    "#pragma flag enable lax\nvoid() g = [++ \"a\" .. 2] {};\n",
    "#pragma flag enable lax\nvoid() g = [++ 1 .. '1 2 3'] {};\n",
    "enum { A = \"a\" };\n",
    "#pragma flag enable lax\nenum { A = \"a\" };\n",
    "#pragma flag enable lax\nenum { A = \"a\", B };\nfloat x = B;\nfloat x = 2;\n",
    "#pragma flag enable lax\nenum { A = \"a\", B };\nfloat x = B;\nfloat x = 1;\n",
    "#pragma flag enable assumeint\nenum { A = \"a\" };\n",
    "const string s = \"a\";\nenum { A = s };\n",
    "#pragma flag enable lax\nconst string s = \"a\";\nenum { A = s, B };\nfloat x = B;\nfloat x = 1;\n",
    "#pragma flag enable typeexplicit\nconst float c = 1;\nenum { A = c };\n",
    "float c;\nenum { A = c };\n",
    // FTEF-22.4: `flag_undefwordsize` blocks a string to a pointer, which the target sets.
    "float *p = \"a\";\n",
    "#pragma target dp\nfloat *p = \"a\";\n",
    "#pragma target dp\nvoid *p = \"a\";\n",
    "#pragma target dp\n__uint8 *p = \"a\";\n",
    "#pragma target darkplaces\nvoid() g = { float *p; string s = p; };\n",
    "#pragma flag enable undefwordsize\nfloat *p = \"a\";\n",
    "#pragma target dp\n#pragma target fte\nfloat *p = \"a\";\n",
    "#pragma target dp\n#pragma flag disable undefwordsize\nfloat *p = \"a\";\n",
    "#pragma target bogus\nfloat *p = \"a\";\n",
    // FTEF-19.31: `void*` and pointers to an 8-bit integer.
    "void *p = \"a\";\n",
    "__int8 *p = \"a\";\n",
    "void() g = { void *p; string s = p; };\n",
    "void() g = { __uint8 *p; string s = p; };\n",
    "void() g = { float *p; void *q = p; };\n",
    "void() g = { void *p; vector *q = p; };\n",
    "void() g = { void() h; void *q = h; };\n",
    "void() g = { void *p; void() q = p; };\n",
    // FTEF-22.5: `extern` and the target. `ftedebug` is the unit tests' alone: FTEQCC writes its
    // progs compressed, and the oracle is built without zlib ("ZLIB compression not supported in
    // this build").
    "extern void(float x) h;\n",
    "#pragma target fte\nextern void(float x) h;\n",
    "extern void(float x) h;\n#pragma target fte\n",
    "#pragma target fte\nextern void(float x) h;\n#pragma target id\n",
    "#pragma target fteh2\nextern void(float x) h;\n",
    "#pragma target dp\nextern void(float x) h;\n",
];

/// Programs whose outcome a warning decides, compiled with every warning an error.
const CAST_WARNINGS: &[&str] = &[
    // FTEF-22.1, FTEF-22.2: the warnings of `lax` and `qccx`.
    "#pragma flag enable lax\nfloat x = \"a\";\n",
    "#pragma flag enable qccx\nfloat x = \"a\";\n",
    "#pragma flag enable lax\nenum { A = \"a\" };\n",
    // FTEF-19.31: the casts that warn among pointers.
    "float *p = \"a\";\n",
    "__int16 *p = \"a\";\n",
    "void *p = \"a\";\n",
    "__uint8 *p = \"a\";\n",
    "void() g = { float *p; vector *q = p; };\n",
    "void() g = { float *p; float *q = p; };\n",
    "void() g = { float *p; string s = p; };\n",
];

/// Programs with the command-line options they are compiled with (FTEF-14.72, FTEF-22.4).
const CAST_OPTIONS: &[(&[&str], &str)] = &[
    (&["-Flax"], "float x = \"a\";\n"),
    (&["-Fqccx"], "float x = \"a\";\n"),
    (&["-Fqccx", "-Werror", "-Wno-Q208"], "float x = \"a\";\n"),
    (&["-std=qccx"], "float x = \"a\";\n"),
    (&["-Ftypeexplicit"], "enum { A = 1i };\n"),
    (&["-Tdp"], "float *p = \"a\";\n"),
    (&["-Tdp_1"], "float *p = \"a\";\n"),
    (&["-Fundefwordsize"], "float *p = \"a\";\n"),
    (&["-Fundefwordsize", "-Tstandard"], "float *p = \"a\";\n"),
    (&["-Tdp", "-Tfte"], "float *p = \"a\";\n"),
    (&["-std=gmqcc"], "float *p = \"a\";\n"),
    (&["-Tfte"], "extern void(float x) h;\n"),
    (&["-Tfte"], "#pragma target id\nextern void(float x) h;\n"),
    (&["-Werror", "-Wno-Q208", "-Flax"], "float x = \"a\";\n"),
];

/// Every case of the family, `f` bound before each program.
pub(super) fn cases() -> Vec<Case> {
    let program = |source: &str| format!("{BIND}{source}");
    let mut cases: Vec<Case> = CAST_PROGRAMS
        .iter()
        .map(|source| Case::new(Dialect::Fteqcc, program(source)))
        .collect();
    cases.extend(CAST_WARNINGS.iter().map(|source| Case {
        options: WERROR_OPTIONS.map(str::to_owned).to_vec(),
        ..Case::new(Dialect::Fteqcc, program(source))
    }));
    for (options, source) in CAST_OPTIONS {
        cases.push(Case {
            options: options.iter().map(|&o| o.to_owned()).collect(),
            ..Case::new(Dialect::Fteqcc, program(source))
        });
    }
    cases
}
