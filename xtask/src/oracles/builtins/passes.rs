// SPDX-License-Identifier: GPL-3.0-or-later

//! The two-pass family of the builtin differential (P2-132): programs whose outcome FTEQCC's
//! autoprototyping decides (`docs/spec/dialect-fteqcc.md` FTEF-12.18, FTEF-12.18 and FTEF-12.19):
//! a `#pragma autoproto` taken wherever it stands in the first pass, what the first pass has
//! compiled in full before it, and the state the second pass starts with: the flags, the warning
//! settings, the keywords, the operator priorities and the pragma defaults the first pass ended
//! with. Each program binds `f` first, so `f` is bound only when FTEQCC compiled the rest.
//! Programs whose outcome a warning decides are compiled with FTEQCC's `-Werror` and read with
//! every warning of the profile an error.

use qc_syntax::Dialect;

use super::directives::BIND;
use super::{Case, Project, WERROR_OPTIONS};

/// What is read before the pragma: nothing, and each kind of definition.
const BEFORE: &[&str] = &[
    "",
    "float x;\n",
    "float x = 5;\n",
    "void() g = {};\n",
    "void() g;\n",
    "void() g = #5;\n",
    "const float c = 5;\n",
    "typedef float t;\n",
    "enum { A, B };\n",
    ".float fl;\n",
    "void() k = { h(); };\n",
    "float x = 5;\nfloat y = x;\n",
    "string s = \"a\";\n",
    "void() g = {};\nvoid() g2 = { g(); };\n",
];

/// The pragmas, and none.
const PRAGMAS: &[&str] = &[
    "",
    "#pragma autoproto\n",
    "#pragma autoproto 0\n",
    "#pragma autoproto 1\n",
];

/// What is read after the pragma.
const AFTER: &[&str] = &[
    "",
    "void() k = { h(); };\nvoid() h = {};\n",
    "void() h = {};\nvoid() k = { h(); };\n",
    "float x = 6;\n",
    "float y = x;\n",
    "void() g = {};\n",
    "void() k = { h(); };\nvoid() h;\n",
    "void() h;\n",
    "float z = c;\n",
    "void(float a = c) g2 = {};\nconst float c = 5;\n",
    "void() h = {};\n#pragma autoproto 0\nvoid() m = { h(); };\n",
    "void() h = {};\n#pragma autoproto\nvoid() m = { n(); };\nvoid() n = {};\n",
];

/// The options the cross product is compiled with.
const OPTIONS: &[&[&str]] = &[&[], &["-Fautoproto"], &["-Fno-autoproto"]];

/// A program whose `-Werror` outcome F329 decides, with `x` referred to.
const REDECLARED: &str = "float x;\nfloat x = 5;\nfloat() h = { return x; };\n";

/// What the first pass ends with that the second pass starts from, or not: a pragma, and a
/// program that reads the state it sets at its first tokens, with whether its outcome is
/// decided by a warning (compiled with `-Werror`).
const STATE: &[(&str, &str, bool)] = &[
    // FTEF-12.11, FTEF-21.1: the flags carry into the second pass.
    (
        "#pragma flag enable assumevar\n",
        "float x = 5;\nfloat y = x;\n",
        false,
    ),
    (
        "#pragma flag disable assumevar\n",
        "float x = 5;\nfloat y = x;\n",
        false,
    ),
    (
        "#pragma flag enable assumevar\n",
        "const float x = 5;\nfloat y = x;\n",
        false,
    ),
    (
        "#pragma flag enable qcfuncs\n",
        "string s = \"\\9\";\n",
        false,
    ),
    (
        "#pragma flag disable qcfuncs\n",
        "string s = \"\\9\";\n",
        false,
    ),
    (
        "#pragma flag enable subscope\n",
        "void() g = { { float x; } x = 1; };\n",
        false,
    ),
    (
        "#pragma flag disable subscope\n",
        "void() g = { { float x; } x = 1; };\n",
        false,
    ),
    (
        "#pragma flag enable attributes\n",
        "[[eraseable]] float x;\n",
        false,
    ),
    (
        "#pragma flag disable attributes\n",
        "[[eraseable]] float x;\n",
        false,
    ),
    (
        "#pragma flag enable assumeint\n",
        "enum { A, B };\nstring s = B;\n",
        false,
    ),
    (
        "#pragma flag enable assumedouble\n",
        "string s = 1.5;\n",
        false,
    ),
    (
        "#pragma flag enable utf8\n",
        "string s = \"\\xff\";\n",
        false,
    ),
    (
        "#pragma flag enable cpriority\n",
        "float a, b;\nfloat() h = { return !a == b; };\n",
        true,
    ),
    (
        "#pragma flag disable cpriority\n",
        "float a, b;\nfloat() h = { return !a == b; };\n",
        true,
    ),
    (
        "#pragma flag enable cpriority\n#pragma flag disable cpriority\n",
        "float a, b;\nfloat() h = { return !a == b; };\n",
        true,
    ),
    (
        "#pragma flag disable cpriority\n#pragma flag enable cpriority\n",
        "float a, b;\nfloat() h = { return !a == b; };\n",
        true,
    ),
    // FTEF-14.63: the warning settings are the command line's again.
    ("#pragma warning disable F329\n", REDECLARED, true),
    ("#pragma warning enable F329\n", REDECLARED, true),
    ("#pragma warning error F329\n", REDECLARED, false),
    // FTEF-20.1: the keywords carry.
    (
        "#pragma keyword disable for\n",
        "void() g = { for (;;) {} };\n",
        false,
    ),
    (
        "#pragma keyword enable for\n",
        "void() g = { for (;;) {} };\n",
        false,
    ),
    (
        "#pragma keyword disable switch\n",
        "float switch;\nfloat() h = { return switch; };\n",
        true,
    ),
    ("float for;\n", "void() g = { for (;;) {} };\n", false),
    // FTEF-20.3: the target is the command line's.
    (
        "#pragma target hexen2\n",
        "void() g = { loop { break; } };\n",
        false,
    ),
    // FTEF-12.6, FTEF-21.14: the pragma defaults.
    ("#pragma noref 1\n", "void() h;\n", false),
    ("#pragma noref 0\n", "void() h;\n", false),
    (
        "#pragma defaultstatic 1\n",
        "void() g = { float x = 1; float x = 2; };\n",
        false,
    ),
    (
        "#pragma defaultstatic 0\n",
        "void() g = { float x = 1; float x = 2; };\n",
        false,
    ),
    ("#pragma nosave 1\n", "float x;\n", false),
];

/// Where the pragma and the state's pragma stand, with `{probe}` read before `{setter}`.
const PLACES: &[&str] = &[
    "#pragma autoproto\n{probe}{setter}",
    "{probe}#pragma autoproto\n{setter}",
    "{probe}{setter}#pragma autoproto\n",
    "#pragma autoproto\n{setter}{probe}",
    "{setter}#pragma autoproto\n{probe}",
    "{setter}{probe}",
    "{probe}{setter}",
];

/// Programs whose outcome a warning decides, compiled with every warning an error, each name
/// referred to so that it has no "no references" warning.
const WARNINGS: &[&str] = &[
    "float x;\n#pragma autoproto\nfloat x = 5;\nfloat() h = { return x; };\n",
    "float x = 5;\n#pragma autoproto\nfloat() h = { return x; };\n",
    "#pragma autoproto\nfloat x = 5;\nfloat() h = { return x; };\n",
    "void() g;\n#pragma autoproto\nvoid() g = {};\nvoid() k = { g(); };\n",
    "float() h = { return 1; };\n#pragma autoproto\nvoid() k = { h(); };\n",
    "#pragma autoproto\nvoid() h = #6;\nvoid() k = { h(); };\n",
    "void() h = #6;\n#pragma autoproto\nvoid() h = #6;\nvoid() k = { h(); };\n",
    "void() h = #6;\n#pragma autoproto\nvoid() h = #7;\nvoid() k = { h(); };\n",
    "void() h = {};\n#pragma autoproto\nvoid() h = #7;\nvoid() k = { h(); };\n",
    "#pragma autoproto\nvoid() h = {};\nvoid() h = #7;\nvoid() k = { h(); };\n",
    "const float c = 5;\n#pragma autoproto\nfloat() h = { return c; };\n",
    "float c;\n#pragma autoproto\nconst float c = 5;\nfloat() h = { return c; };\n",
    "float c = 5;\n#pragma autoproto\nvar float c = 5;\nfloat() h = { return c; };\n",
    "#pragma autoproto\nfloat c = 5;\n#pragma flag enable assumevar\nfloat() h = { return c; };\n",
    "#pragma flag enable assumevar\nfloat c = 5;\n#pragma autoproto\nfloat() h = { return c; };\n",
    "__accumulate void() h = {};\n#pragma autoproto\n__accumulate void() h = {};\nvoid() k = { h(); };\n",
    "#pragma autoproto\ntypedef float t;\nt x;\nfloat() h = { return x; };\n",
    "typedef float t;\n#pragma autoproto\ntypedef float t;\nt x;\nfloat() h = { return x; };\n",
    "typedef float t;\n#pragma autoproto\ntypedef string t;\nt x;\nfloat() h = { return 1; };\n",
    "enum { A, B };\n#pragma autoproto\nenum { A, B };\nfloat() h = { return B; };\n",
    "struct S { float a; };\n#pragma autoproto\nstruct S { float a; };\nstruct S x;\nfloat() h = { return x.a; };\n",
    ".float fl;\n#pragma autoproto\n.float fl;\nfloat() h = { return self.fl; };\nentity self;\n",
    "float() h = { return 1; };\n#pragma autoproto\nfloat() h = { return 2; };\n",
    "string s = \"a\";\n#pragma autoproto\nstring s = \"b\";\nstring() h = { return s; };\n",
];

/// What an included file holds: the pragma among other lines.
const INCLUDED: &[&str] = &[
    "#pragma autoproto\n",
    "float x;\n#pragma autoproto\n",
    "#pragma autoproto 0\n",
    "void() h = {};\n#pragma autoproto\n",
    "void() h = {};\n",
    "#pragma autoproto\nvoid() h = {};\n",
];

/// What the including file reads before and after the `#include`.
const AROUND: &[(&str, &str)] = &[
    ("", ""),
    ("", "void() k = { h(); };\n"),
    ("void() k = { h(); };\n", ""),
    ("float x;\n", "void() k = { h(); };\n"),
    ("void() g = {};\n", "float y;\n"),
];

/// Every case of the family, `f` bound before each program.
pub(super) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for before in BEFORE {
        for pragma in PRAGMAS {
            for after in AFTER {
                for options in OPTIONS {
                    cases.push(Case {
                        options: options.iter().map(|&o| o.to_owned()).collect(),
                        ..Case::new(Dialect::Fteqcc, format!("{BIND}{before}{pragma}{after}"))
                    });
                }
            }
        }
    }
    for source in WARNINGS {
        for autoproto in [false, true] {
            let mut options: Vec<String> = WERROR_OPTIONS.map(str::to_owned).to_vec();
            if autoproto {
                options.push("-Fautoproto".to_owned());
            }
            cases.push(Case {
                options,
                ..Case::new(Dialect::Fteqcc, format!("{BIND}{source}"))
            });
        }
    }
    for included in INCLUDED {
        for (before, after) in AROUND {
            for options in OPTIONS {
                cases.push(Case {
                    options: options.iter().map(|&o| o.to_owned()).collect(),
                    project: Some(Project {
                        compiled: "t.qc".to_owned(),
                        files: vec![("b.qh".to_owned(), (*included).to_owned())],
                        include_dirs: Vec::new(),
                        progs: None,
                    }),
                    ..Case::new(
                        Dialect::Fteqcc,
                        format!("{BIND}{before}#include \"b.qh\"\n{after}"),
                    )
                });
            }
        }
    }
    for (setter, probe, werror) in STATE {
        for place in PLACES {
            for autoproto in [false, true] {
                let text = place.replace("{setter}", setter).replace("{probe}", probe);
                let mut options: Vec<String> = Vec::new();
                if *werror {
                    options.extend(WERROR_OPTIONS.map(str::to_owned));
                }
                if autoproto {
                    options.push("-Fautoproto".to_owned());
                }
                cases.push(Case {
                    options,
                    ..Case::new(Dialect::Fteqcc, format!("{BIND}{text}"))
                });
            }
        }
    }
    cases
}
