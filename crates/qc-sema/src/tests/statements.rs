// SPDX-License-Identifier: GPL-3.0-or-later

//! Statements as FTEQCC checks them (`docs/spec/dialect-fteqcc.md` FTEF-34). Each case's
//! diagnostics are the pinned FTEQCC's on the standard target (2026-10-27); a warning its
//! settings make an error is `E0125` (F313).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

fn analyze(source: &str, autoproto: bool) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        autoproto: autoproto.then_some(true),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

fn diags(source: &str) -> Vec<(Code, String)> {
    analyze(source, false)
}

const DEFS: &str = "entity self; .float nextthink; float time; float a; string s; vector v; \
                    int i; __variant vr;\n";

/// The diagnostics of a function `g` returning `ret` with the body `body`.
fn function(ret: &str, body: &str) -> Vec<(Code, String)> {
    diags(&format!("{DEFS}{ret}() g = {{ {body} }};\n"))
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

// spec FTEF-34.1: `return;` in a function that returns a value warns once, and not after
// `return =`; a value is cast to the return type; a value returned from a `void` function warns;
// `__NULL__` returns from any function
// spec QCD-7.98: W0256 for a bare `return;` in a function that returns a value
// spec QCD-7.99: W0257 for a value returned from a `void` function
#[test]
fn returns() {
    let nothing = d(Code::W0256, "'g' returned nothing, expected float");
    assert_eq!(function("float", "return;"), std::slice::from_ref(&nothing));
    assert_eq!(
        function("float", "return; return;"),
        std::slice::from_ref(&nothing)
    );
    assert_eq!(function("float", "if (a) return 1; return;"), [nothing]);
    for quiet in [
        "return = 1; return;",
        "return = s;",
        "return __NULL__;",
        "return 0;",
        "return a;",
    ] {
        assert_eq!(function("float", quiet), [], "{quiet}");
    }
    assert_eq!(
        function("float", "return s;"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
    assert_eq!(
        function("float", "return i;"),
        [d(Code::E0125, "itof emulation: denormals are unsafe i")]
    );
    assert_eq!(
        function("vector", "return 1;"),
        [d(Code::W0220, "Implicit cast from float to vector")]
    );
    for (body, ty) in [
        ("return 1;", "float"),
        ("return s;", "string"),
        ("return a+1;", "float"),
    ] {
        assert_eq!(
            function("void", body),
            [d(Code::W0257, &format!("'g' returned {ty}, expected void"))],
            "{body}"
        );
    }
}

// spec FTEF-34.2: `if !()` and `if not()` warn that they are FTEQCC's, `WARN_FTE_SPECIFIC`, which
// is off by default
// spec QCD-7.103: W0261 for `if !()` and `if not()`
#[test]
fn negated_ifs() {
    let body = "void() g = { if !(a) a = 1; if not(a) a = 2; };\n";
    assert_eq!(diags(&format!("{DEFS}{body}")), []);
    assert_eq!(
        diags(&format!("{DEFS}#pragma warning enable F311\n{body}")),
        [
            d(Code::W0261, "if !() is specific to fteqcc"),
            d(Code::W0261, "if not() is specific to fteqcc or hexen2")
        ]
    );
}

// spec FTEF-34.2: a condition is tested for its type; an assignment at its top is "suggest
// parenthesis"
// spec FTEF-34.3: `while` and `do` test their conditions as `if` does
// spec FTEF-34.4: a `for` condition is tested with no test of its type
#[test]
fn conditions() {
    for quiet in [
        "if (s) a = 1;",
        "if !(s) a = 1;",
        "if (v) a = 1;",
        "while (s) a = 1;",
        "do a = 1; while (s);",
        "for (; s;) a = 1;",
        "for (; vr;) a = 1;",
        "if (\"x\") a = 1;",
        "while (1) a = 1;",
    ] {
        assert_eq!(function("void", quiet), [], "{quiet}");
    }
    for mismatch in [
        "if (vr) a = 1;",
        "while (vr) a = 1;",
        "do a = 1; while (vr);",
    ] {
        assert_eq!(
            function("void", mismatch),
            [d(Code::W0247, "conditional type mismatch: variant")],
            "{mismatch}"
        );
    }
    assert_eq!(
        function("void", "do a = 1; while (a = 2);"),
        [d(
            Code::W0242,
            "suggest parenthesis for assignment used as truth value"
        )]
    );
}

// spec FTEF-34.5: each case value is made the switch's type, a formula is refused, a type no
// comparison takes is "Bad switch type", and a range of no number warns
// spec QCD-4.47: E0342 for a case value that generates code
// spec QCD-4.48: E0343 for a switch on a type no comparison takes
// spec QCD-7.102: W0260 for a case range of no number
#[test]
fn switches() {
    for quiet in [
        "switch (a) { case a: break; case self.nextthink: break; }",
        "switch (a) { case 1 + 2: break; case 1 .. 2: break; }",
        "switch (v) { case 1: break; }",
        "switch (a) { case 0: break; }",
    ] {
        assert_eq!(function("void", quiet), [], "{quiet}");
    }
    let cases = [
        (
            "switch (a) { case 1: a = 2; break; case s: break; }",
            d(
                Code::E0227,
                "Implicit type mismatch. Needed float, got string.",
            ),
        ),
        (
            "switch (a) { case 1: break; case a+1: break; }",
            d(Code::E0342, "Case statements may not use formulas"),
        ),
        (
            "switch (s) { case 1 .. 2: break; }",
            d(
                Code::E0227,
                "Implicit type mismatch. Needed string, got float.",
            ),
        ),
        (
            "switch (a) { case 1 .. 2: break; case i: break; }",
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
        ),
        (
            "switch (i) { case 1: break; case 2.5: break; }",
            d(
                Code::W0234,
                "Numerical overflow. 2.500000 will be rounded to 2",
            ),
        ),
        (
            "switch (s) { case \"a\" .. \"b\": break; }",
            d(Code::W0260, "switch caserange MUST be a float or integer"),
        ),
        (
            "switch (vr) { case 1: break; }",
            d(Code::E0343, "Bad switch type"),
        ),
        (
            "switch (self) { case self: break; }",
            d(Code::W0239, "Result of comparison is constant"),
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(function("void", body), [expected], "{body}");
    }
}

// spec FTEF-34.7: `thinktime e : t;` makes `t` a float, and refuses an `e` that is no entity
// spec QCD-4.49: E0344 for a `thinktime` of no entity or no float
#[test]
fn thinktime() {
    assert_eq!(function("void", "__thinktime self : 1;"), []);
    let cases = [
        (
            "__thinktime a : 1;",
            d(Code::E0344, "thinktime type mismatch"),
        ),
        (
            "__thinktime self : s;",
            d(
                Code::E0227,
                "Implicit type mismatch. Needed float, got string.",
            ),
        ),
        (
            "__thinktime self : i;",
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
        ),
        (
            "__thinktime self : v;",
            d(
                Code::E0227,
                "Implicit type mismatch. Needed float, got vector.",
            ),
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(function("void", body), [expected], "{body}");
    }
}

// spec FTEF-34.6: a function that returns a value warns when its last statement is no return or
// backward jump, or when a condition jumps to its end
// spec QCD-7.100: W0258 for a function some path through which returns no value
#[test]
fn missing_returns() {
    let missing = d(Code::W0258, "g: not all control paths return a value");
    for body in [
        "a = 1;",
        "",
        "if (a) return 1;",
        "return 1; a = 2;",
        "do { return 1; } while (a);",
        "while (a) { return 1; }",
    ] {
        assert_eq!(
            function("float", body),
            std::slice::from_ref(&missing),
            "{body}"
        );
    }
    assert_eq!(
        function(
            "float",
            "if (a < 0) return 1; if ((a == 3) && (3 >= 0)) return 1;"
        ),
        [d(Code::W0239, "Result of comparison is constant"), missing]
    );
    for body in [
        "if (a) return 1; else return 2;",
        "while (1) { return 1; }",
        "while (1) { a = 1; }",
        "for (;;) { a = 1; }",
        "switch (a) { case 1: return 1; default: return 2; }",
        "if (a) { return 1; } else { if (a) return 2; else return 3; }",
        "if (1) return 1;",
        "switch (a) { case 1: return 1; } return 2;",
        "while (a) return 1; return 2;",
        "while (1) { if (a) break; } return 1;",
        "if (a < 0) return 1; if (a) return 1;",
        "return = 1;",
    ] {
        assert_eq!(function("float", body), [], "{body}");
    }
}

// spec FTEF-34.6: a statement after a `return` or a `goto` that no jump reaches is unreachable
// spec QCD-7.101: W0259 for code no path reaches
#[test]
fn unreachable_code() {
    let unreachable = d(Code::W0259, "g: contains unreachable code (line 2)");
    for body in [
        "for (a = 1; ; a = 2) break;",
        "return; a = 2;",
        "while (a) { break; a = 3; }",
        "goto l; a = 1; l: a = 2;",
        "goto l; a = 1; a = 3; l: a = 2;",
    ] {
        assert_eq!(
            function("void", body),
            std::slice::from_ref(&unreachable),
            "{body}"
        );
    }
    assert_eq!(
        function("float", "return 1; a = 2; return 3;"),
        [unreachable]
    );
    for quiet in [
        "switch (a) { case 1: }",
        "switch (a) { }",
        "switch (a) { case 1: return; }",
        "switch (a) { case 1: a = 2; break; default: return; }",
        "return; return;",
    ] {
        assert_eq!(function("void", quiet), [], "{quiet}");
    }
}

// spec FTEF-34.6: an error in a body ends FTEQCC's parse of its function, which is not checked
// for its returns
#[test]
fn an_error_skips_the_return_checks() {
    assert_eq!(
        function("float", "a = s; a = 1;"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
}

// spec FTEF-12.21: autoprototyping's first pass gives a parameter no default that names a
// definition, which that pass does not initialise; a literal default stays
#[test]
fn autoprototyping_drops_named_defaults() {
    let too_few = d(
        Code::W0253,
        "too few parameters on call to f, b will be UNDEFINED",
    );
    for source in [
        "const float K = 1; void(float x, float b = K) f = {}; void() g = { f(1); };\n",
        "void() g = { f(1); }; const float K = 1; void(float x, float b = K) f = {};\n",
    ] {
        assert_eq!(
            analyze(source, true),
            std::slice::from_ref(&too_few),
            "{source}"
        );
        assert_eq!(
            analyze(source, false).len(),
            usize::from(source.starts_with("void() g")),
            "{source}"
        );
    }
    assert_eq!(
        analyze(
            "void(float x, float b = 1) f = {}; void() g = { f(1); };\n",
            true
        ),
        []
    );
}

// spec FTEF-34.1: `return;` and `return =` in a function returning a type larger than a vector
// are fatal errors
// spec FTEF-34.8: such a function needs `OP_STORE_P`, and warns that it is inefficient
// spec QCD-4.50: E0341 for `return;` or `return =` of a type larger than a vector
// spec QCD-7.104: W0262 for a function returning a type larger than a vector
#[test]
fn large_returns() {
    let source = |target: &str, body: &str| {
        format!("{target}struct s {{float a, b, c, d;}};\ns() g = {{ {body} }};\n")
    };
    let fte = "#pragma target fte\n";
    assert_eq!(
        diags(&source("", "return;")),
        [d(
            Code::E0320,
            "Opcode \"=|STORE_P\" not valid for target. Consider the use of: #pragma target fte"
        )]
    );
    assert_eq!(
        diags(&source(fte, "return;")),
        [d(Code::E0341, "'g' returned nothing, expected s")]
    );
    assert_eq!(
        diags(&source(fte, "return = 1;")),
        [d(Code::E0341, "'g' not supported when returning s")]
    );
    assert_eq!(
        diags(&source(fte, "local s x; return x;")),
        [d(
            Code::W0262,
            "Function g returns (large) struct s. This is inefficient."
        )]
    );
    // A struct of no more words than a vector returns as a value does.
    assert_eq!(
        diags("struct s {float a, b;};\ns() g = { return; };\n"),
        [d(Code::W0256, "'g' returned nothing, expected s")]
    );
}

// spec FTEF-34.5: a switch on a strict enum warns of each item no case covers, the last
// first; a `default` covers them all
// spec QCD-7.105: W0263 for an item of a strict enum no case of its switch covers
// spec FTEF-30.2: a strict enum's test is "conditional type mismatch"
#[test]
fn enum_switches() {
    let source = |cases: &str| {
        format!(
            "enum class E : float {{A, B, C}}; E e;\nvoid() g = {{ switch (e) {{ {cases} }} }};\n"
        )
    };
    let mismatch = d(Code::W0247, "conditional type mismatch: enum");
    assert_eq!(
        diags(&source("case E::A: break;")),
        [
            d(Code::W0263, "E::C not part of switch"),
            d(Code::W0263, "E::B not part of switch"),
            mismatch.clone()
        ]
    );
    assert_eq!(
        diags(&source("case E::A: break; default: break;")),
        std::slice::from_ref(&mismatch)
    );
    assert_eq!(
        diags(&source(
            "case E::A: break; case E::B: break; case E::C: break;"
        )),
        std::slice::from_ref(&mismatch)
    );
    assert_eq!(
        diags("enum class E : float {A, B}; E e; float a;\nvoid() g = { if (e) a = 1; };\n"),
        [mismatch]
    );
    assert_eq!(
        diags(&source("case 0: break;")),
        [d(
            Code::E0227,
            "Implicit type mismatch. Needed enum, got float."
        )]
    );
}

// spec FTEF-34.9: an initialiser of a global that needs code is "Unable to generate statements
// at global scope" for each statement, then "is not constant"; a constant one is neither
// spec QCD-4.51: E0345 for a statement generated at global scope
// spec FTEF-30.7: a vector of one non-null component is that component times its axis; a
// component left out is 0
#[test]
fn global_initialisers() {
    let unable = |op: &str| {
        d(
            Code::E0345,
            &format!("Unable to generate statements at global scope ({op})."),
        )
    };
    let not_constant = |name: &str| {
        d(
            Code::E0214,
            &format!("initializer for '{name}' is not constant"),
        )
    };
    let cases: [(&str, Vec<(Code, String)>); 7] = [
        (
            "float a; float b = a + 1;",
            vec![unable("ADD_F"), not_constant("b")],
        ),
        ("float b = 1 + 2;", vec![]),
        ("const vector v = [19, 20];", vec![]),
        (
            "float a; vector w = [a];",
            vec![unable("MUL_VF"), not_constant("w")],
        ),
        (
            "float a; vector w = [a, 1];",
            vec![
                unable("STORE_F"),
                unable("STORE_F"),
                unable("STORE_F"),
                not_constant("w"),
            ],
        ),
        (
            "string(float x, string y, vector z) f = #1; string b = f(1, \"a\", [1,2,3]);",
            vec![
                unable("STORE_F"),
                unable("STORE_F"),
                unable("STORE_V"),
                unable("CALL3"),
                not_constant("b"),
            ],
        ),
        (
            "float a; float b = a ? 1 : 2;",
            vec![
                unable("IFNOT"),
                unable("STORE_F"),
                unable("GOTO"),
                unable("STORE_F"),
                not_constant("b"),
            ],
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(diags(&format!("{source}\n")), expected, "{source}");
    }
    assert_eq!(
        diags("entity self; .float fl; float b = self.fl;\n"),
        [unable("LOADF_F"), not_constant("b")]
    );
    assert_eq!(
        diags("int i; float b = i + 1;\n"),
        [
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
            unable("DIV_F"),
            unable("ADD_F"),
            not_constant("b")
        ]
    );
}

// spec FTEF-16.14: an unknown name that is a keyword in force warns that it is used as a name
// before it is "Unknown value"
#[test]
fn an_unknown_keyword_name() {
    assert_eq!(
        diags("float c;\nvoid() g = { c = local; };\n"),
        [
            d(
                Code::W0221,
                "\"local\" keyword used as variable name - coexisting"
            ),
            d(Code::E0301, "Unknown value \"local\".")
        ]
    );
}
