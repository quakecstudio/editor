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

// spec FTEF-24.10: a `.` after a value of a type with no members ends the expression: the
// statement goes on with the value, then the `.` is "expected ;, found ."
#[test]
fn a_dot_after_a_value_with_no_members() {
    let cases = [
        (
            "float a, b; void() g = { b = a.nope; };",
            vec![d(Code::E0204, "expected ;, found .")],
        ),
        (
            "float a, b; void() g = { b = a.x + 1; };",
            vec![d(Code::E0204, "expected ;, found .")],
        ),
        (
            "float a, b; void(float x) f = {}; void() g = { f(a.x); };",
            vec![d(Code::E0204, "expected ), found .")],
        ),
        (
            "float a, b; void() g = { b = (a.x); };",
            vec![d(Code::E0204, "expected ), found .")],
        ),
        (
            "float a, b; float c[2]; void() g = { b = c[a.x]; };",
            vec![d(Code::E0204, "expected ], found .")],
        ),
        // The construct's end is fatal at once, before what follows it is read.
        (
            "float a, b; string s; void(float x, float y) f = {}; void() g = { f((a.x), \"s\"); };",
            vec![d(Code::E0204, "expected ), found .")],
        ),
        (
            "float a, b; string s; void() g = { b = (a.x) + s; };",
            vec![d(Code::E0204, "expected ), found .")],
        ),
        (
            "float a, b; string s; float c[2]; void() g = { b = c[a.x] + s; };",
            vec![d(Code::E0204, "expected ], found .")],
        ),
        (
            ".float fl; float b; void() g = { b = fl.x; };",
            vec![d(Code::E0213, "Cannot cast from .float to float")],
        ),
        (
            "int i; float b; void() g = { b = i.x; };",
            vec![
                d(Code::E0125, "itof emulation: denormals are unsafe i"),
                d(Code::E0204, "expected ;, found ."),
            ],
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(diags(&format!("{source}\n")), expected, "{source}");
    }
}

// spec FTEF-31.4: "Address-of temp on line FILE:LINE" names the file as the compiler was given it
// and follows `#pragma file` and `#pragma line`
// spec FTEF-34.6: the line of unreachable code follows `#pragma line`
#[test]
fn messages_name_the_file_and_line_fteqcc_has() {
    let analyze = |source: &str| -> Vec<String> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        db.set_file(FILE, PathBuf::from("/x/y/t.qc"), source.as_bytes().to_vec());
        let mut names = std::collections::BTreeMap::new();
        names.insert(FILE, "t.qc".to_owned());
        db.analyze_module(&ModuleInput {
            compiled: vec![FILE],
            names,
            dir: PathBuf::from("/x/y"),
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| d.diagnostic.message.clone())
        .collect()
    };
    let address = "float a; void() g = { local float *p; p = &(a+a); };\n";
    assert_eq!(
        analyze(&format!("#pragma target fte\n{address}")),
        [
            "Address-of temp on line t.qc:2",
            "Cannot cast from float to float*"
        ]
    );
    assert_eq!(
        analyze(&format!(
            "#pragma target fte\n#pragma file foo.qc\n#pragma line 40\n{address}"
        )),
        [
            "Address-of temp on line foo.qc:40",
            "Cannot cast from float to float*"
        ]
    );
    assert_eq!(
        analyze("#pragma line 40\nvoid() g = { return; g(); };\n"),
        ["g: contains unreachable code (line 40)"]
    );
}

// spec FTEF-34.10, QCD-4.56: a second `default` is "Duplicated default case", after the errors
// of the cases before it
#[test]
fn a_second_default_is_refused() {
    let found = |source: &str| -> Vec<(Code, String)> {
        diags(source)
            .into_iter()
            .filter(|(code, _)| *code != Code::W0204)
            .collect()
    };
    assert_eq!(
        found("void() g = { switch (1) { default: case 1: default: ; } };\n"),
        [d(Code::E0352, "Duplicated default case")]
    );
    assert_eq!(
        found("void() g = { switch (1) { default: break; } switch (2) { default: ; } };\n"),
        []
    );
    assert_eq!(
        found("void() g = { switch (1) { case \"a\": default: default: ; } };\n"),
        [d(
            Code::E0227,
            "Implicit type mismatch. Needed float, got string."
        )]
    );
}

// spec FTEF-24.11: a return names a type by its own name: a pointer `ptr`, a field `.T`, a
// function `__function`
// spec FTEF-34.1: "'NAME' returned TYPE, expected TYPE" and "'NAME' returned nothing, expected
// TYPE"
#[test]
fn a_return_names_types_by_their_own_names() {
    let defs = "#pragma target fte\nfloat *p; .float fa; void() h = {};\n";
    let returned = |what: &str| d(Code::W0257, &format!("'g' returned {what}, expected void"));
    assert_eq!(
        diags(&format!(
            "{defs}void() g = {{ return p; return fa; return h; return (float*)p; }};\n"
        )),
        [
            returned("ptr"),
            returned(".float"),
            returned("__function"),
            returned("ptr"),
        ]
    );
    let nothing = |ret: &str| {
        let message = format!("'g' returned nothing, expected {ret}");
        let found = diags(&format!(
            "{defs}{ret_type} g = {{ return; }};\n",
            ret_type = match ret {
                "ptr" => "float*()",
                _ => "void()()",
            }
        ));
        assert_eq!(found, [d(Code::W0256, &message)], "{ret}");
    };
    nothing("ptr");
    nothing("__function");
}

// spec FTEF-34.9: at global scope an argument of more than three words is copied three words a
// parameter as it is read, the others stored after; `OP_CALL1H` passes the first two parameters
// with no store and calls by `OP_CALLnH`
#[test]
fn global_calls_store_their_arguments() {
    let unable = |op: &str| {
        d(
            Code::E0345,
            &format!("Unable to generate statements at global scope ({op})."),
        )
    };
    let defs = "struct st { vector a, b; }; struct s5 { vector a; float b, c; }; st S; s5 S5; \
                __int64 L;\nfloat(float f, s5 s) q5 = #6; float(st s) q = #4; \
                float(__int64 l, vector v, float x) q7 = #8; float() z = #3;\n";
    let at = |target: &str, init: &str| {
        let found = diags(&format!("{target}\n{defs}float r = {init};\n"));
        found
            .into_iter()
            .filter(|(code, _)| *code == Code::E0345)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        at("", "q5(1, S5)"),
        ["STORE_V", "STORE_F", "STORE_F", "STORE_F", "CALL3"].map(unable)
    );
    assert_eq!(at("", "q(S)"), ["STORE_V", "STORE_V", "CALL2"].map(unable));
    assert_eq!(
        at("", "q7(L, '1 1 1', 2)"),
        ["STORE_V", "STORE_V", "STORE_F", "CALL3"].map(unable)
    );
    assert_eq!(at("", "z()"), [unable("CALL0")]);
    let fte = "#pragma target fte";
    assert_eq!(
        at(fte, "q5(1, S5)"),
        ["STORE_V", "STORE_I64", "CALL3H"].map(unable)
    );
    assert_eq!(
        at(fte, "q7(L, '1 1 1', 2)"),
        ["STORE_F", "CALL3H"].map(unable)
    );
    assert_eq!(at(fte, "z()"), [unable("CALL0")]);
}

/// Each diagnostic of `source` with its line, counted from 1.
fn lined(source: &str) -> Vec<(u32, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    })
    .diagnostics()
    .map(|d| {
        let before = source.get(..d.diagnostic.span.start).unwrap_or_default();
        let line = u32::try_from(before.matches('\n').count())
            .unwrap()
            .saturating_add(1);
        (line, d.diagnostic.message.clone())
    })
    .collect()
}

/// The cases of [`gotos_find_their_labels_at_the_function_s_end`], each with the pinned
/// FTEQCC's messages and their lines.
const GOTOS_CASES: &[(&str, &[(u32, &str)])] = &[
    ("void() f = { if (0) { goto nowhere; } };\n", &[]),
    ("void() f = { if (0) goto nowhere; else return; };\n", &[]),
    ("void() f = { if (1) {} else goto nowhere; };\n", &[]),
    (
        "void() f = { goto nowhere; };\n",
        &[(1, "Goto statement with no matching label \"nowhere\"")],
    ),
    ("void() f = { if (0) { goto a; } a: return; };\n", &[]),
    ("void() f = { if (0) { a: return; } goto a; };\n", &[]),
    (
        "float() f = {\n\tgoto x;\n\tgoto y;\n\tif (0) continue;\n\twhile (1) break;\n\tcontinue;\n\n};\nvoid() g = { goto z; };\n",
        &[
            (8, "Goto statement with no matching label \"x\""),
            (9, "Goto statement with no matching label \"z\""),
        ],
    ),
    (
        "float a;\nvoid() f = { goto top;\na = 1;\ntop:\na = 2; };\n",
        &[(3, "f: contains unreachable code (line 3)")],
    ),
    (
        "float a;\nvoid() g = { top: a = 1; };\nvoid() f = { goto top; };\n",
        &[(3, "Goto statement with no matching label \"top\"")],
    ),
    (
        "float a;\nvoid() f = { goto out; void() h = { out: a = 1; }; };\n",
        &[(2, "f: contains unreachable code (line 2)")],
    ),
    (
        "float a;\nvoid() f = { goto out; void() h = { a = 1; }; out: a = 2; };\n",
        &[(2, "Goto statement with no matching label \"out\"")],
    ),
    (
        "float a;\nvoid() f = { top: a = 1; void() h = { a = 2; }; goto top; };\n",
        &[(2, "Goto statement with no matching label \"top\"")],
    ),
    (
        "const float K = 0;\nvoid() f = { if (K) goto nowhere; };\n",
        &[],
    ),
    (
        "void() f = { while (0) goto nowhere; };\n",
        &[(1, "Goto statement with no matching label \"nowhere\"")],
    ),
    (
        "void() f = { goto nowhere; continue; };\n",
        &[(1, "Goto statement with no matching label \"nowhere\"")],
    ),
    (
        "float a;\nvoid() f = { goto out; void() h = { goto in; a = 1; }; in: a = 2; };\n",
        &[(2, "Goto statement with no matching label \"out\"")],
    ),
    (
        "float a;\nvoid() f = { void() h = { goto in; a = 1; }; in: a = 2; };\n",
        &[(2, "Goto statement with no matching label \"in\"")],
    ),
    (
        "float a;\nvoid() f = { void() h = { in: a = 1; }; goto in; };\n",
        &[(2, "Goto statement with no matching label \"in\"")],
    ),
    (
        "float a;\nvoid() f = { goto x; void() h = { x: a = 1; void() k = { a = 3; }; }; };\n",
        &[(2, "f: contains unreachable code (line 2)")],
    ),
    (
        "float a;\nvoid() f = { goto nowhere; void() h = { a = 1; }; };\n",
        &[(2, "Goto statement with no matching label \"nowhere\"")],
    ),
    (
        "void() f = {\n\treturn;\n\tlocal float a = 1;\n};\n",
        &[(3, "f: contains unreachable code (line 3)")],
    ),
    (
        "void() f = {\n\treturn;\n\n\tprint(\"x\");\n\tprint(\"y\");\n};\n",
        &[(4, "Unknown value \"print\".")],
    ),
    (
        "float() f = {\n\tif (1) return 1;\n\tlocal float x;\n\tx = 2;\n};\n",
        &[(5, "f: not all control paths return a value")],
    ),
];

// spec FTEF-34.12: the first `goto` generated with no label read since the last function's end
// is fatal at the function's `}`; a stripped arm's `goto` is not checked, and a function defined
// inside another checks the gotos and labels read since the last end, then forgets them
// spec QCD-4.81: E0308 "Goto statement with no matching label \"NAME\"" at the closing brace
// spec FTEF-34.13: unreachable code is reported at its own line
#[test]
fn gotos_find_their_labels_at_the_function_s_end() {
    for (source, expected) in GOTOS_CASES {
        let expected: Vec<(u32, String)> = expected
            .iter()
            .map(|&(line, m)| (line, m.to_owned()))
            .collect();
        assert_eq!(lined(source), expected, "{source}");
    }
}

/// The cases of [`a_constant_condition_picks_its_arm_at_global_scope`], each with the pinned
/// FTEQCC's messages and their lines.
const GLOBAL_TERNARIES_CASES: &[(&str, &[(u32, &str)])] = &[
    (
        "float y; float q = __NULL__ ?: y;\n",
        &[(1, "initializer for 'q' is not constant")],
    ),
    ("float y; string q = \"a\" ?: \"b\";\n", &[]),
    ("float y; string q = \"\" ?: \"b\";\n", &[]),
    ("float y; float ff = 2; float q = ff ?: y;\n", &[]),
    ("float x; float y; float q = sizeof(x) ?: y;\n", &[]),
    (
        "float y; float q = 1 ? y : 2;\n",
        &[(1, "initializer for 'q' is not constant")],
    ),
    ("float y; float q = 0 ? y : 2;\n", &[]),
    ("float y; float q = 1 ?: y;\n", &[]),
    (
        "float y; float q = 0 ?: y;\n",
        &[(1, "initializer for 'q' is not constant")],
    ),
    ("float y; float q = -0.0 ? 3 : y;\n", &[]),
    (
        "float y; float q = -0.0 ?: y;\n",
        &[(1, "initializer for 'q' is not constant")],
    ),
    (
        "float y; vector q = 0 ? y : 2;\n",
        &[(1, "Implicit cast from float to vector")],
    ),
    ("float y; float q = 1i ? 2 : \"x\";\n", &[]),
    (
        "float y; float q = 0 ? y + 1 : 2;\n",
        &[(1, "Unable to generate statements at global scope (ADD_F).")],
    ),
    (
        "float y; float q = y ? 1 : 2;\n",
        &[
            (1, "Unable to generate statements at global scope (IFNOT)."),
            (
                1,
                "Unable to generate statements at global scope (STORE_F).",
            ),
            (1, "Unable to generate statements at global scope (GOTO)."),
            (
                1,
                "Unable to generate statements at global scope (STORE_F).",
            ),
            (1, "initializer for 'q' is not constant"),
        ],
    ),
];

// spec FTEF-30.15: at global scope a constant condition picks its arm with no statement, `?:` by
// its truth and `? :` by its first word
#[test]
fn a_constant_condition_picks_its_arm_at_global_scope() {
    for (source, expected) in GLOBAL_TERNARIES_CASES {
        let expected: Vec<(u32, String)> = expected
            .iter()
            .map(|&(line, m)| (line, m.to_owned()))
            .collect();
        assert_eq!(lined(source), expected, "{source}");
    }
}

// spec FTEF-34.14: the profile's own text for a program FTEQCC refuses too: `typedef ;` at its own
// line, and `(T *)` of a name that is no type as a type
#[test]
fn programs_both_refuse_keep_this_profile_s_text() {
    assert_eq!(
        lined("typedef ;\n"),
        [(1, "typedef defines no types".to_owned())]
    );
    assert_eq!(
        lined("void() f = { local float y = (T *)0; };\n"),
        [(1, "\"T\" is not a type".to_owned())]
    );
}

// spec FTEF-22.9: a truth value is its own type, to which a float case does not convert, the
// mismatch reported at the token after the switch's body, where the cases are converted; a
// switch of a sum takes it
#[test]
fn a_switch_of_a_truth_value_takes_no_float_case() {
    let logical = "float a, b;\nvoid() f = {\nswitch (a || b)\n{\ncase 0:\nbreak;\n}\nf();\n};\n";
    assert_eq!(
        lined(logical),
        [(
            8,
            "Implicit type mismatch. Needed bool, got float.".to_owned()
        )]
    );
    let compared =
        "float a, b;\nvoid() f = {\nswitch (a < b) { case 1: break; default: break; }\n};\n";
    assert_eq!(
        lined(compared),
        [(
            4,
            "Implicit type mismatch. Needed bool, got float.".to_owned()
        )]
    );
    let sum = "float a, b;\nvoid() f = {\nswitch (a + b)\n{\ncase 0:\nbreak;\n}\n};\n";
    assert_eq!(lined(sum), []);
}

// spec FTEF-19.77: after a qualifier a type `QCC_TypeForName` finds, or a C type word, is the
// declaration's type, so a definition of it with no name is "not a name", and one with `;`
// declares nothing; a name that is no type has its type left out, an `int`
#[test]
fn a_type_after_a_qualifier_is_the_type() {
    let not_a_name = || [(1, "\"=\" - not a name".to_owned())];
    for source in [
        "const float = 1;\n",
        "const int = 0i;\n",
        "const unsigned = 1;\n",
        "__unused float = 1;\n",
        "noref string = \"a\";\n",
        "typedef float t; const t = 1;\n",
    ] {
        assert_eq!(lined(source), not_a_name(), "{source}");
    }
    for source in [
        "static entity;\n",
        "var vector;\n",
        "typedef float t; static t;\n",
        "static x;\n",
        "const k = 5;\n",
    ] {
        assert_eq!(lined(source), [], "{source}");
    }
}

// spec FTEF-8.7: a keyword that starts a type after `enum` is the enum's legacy base type, read
// whole, then the `{` is expected
#[test]
fn a_keyword_type_after_enum_is_its_base() {
    let legacy = "legacy enum base type. Use \"enum [class] [name_e]:type\" instead";
    let expected = |list: &[&str]| -> Vec<(u32, String)> {
        list.iter().map(|m| (1, (*m).to_owned())).collect()
    };
    assert_eq!(
        lined("enum enum { A };\n"),
        expected(&["expected {, found ;"])
    );
    assert_eq!(
        lined("enumflags enumflags { A, B };\n"),
        expected(&["expected {, found ;"])
    );
    assert_eq!(lined("enum enum { B } { A };\n"), expected(&[legacy]));
    assert_eq!(
        lined("enum struct S { float a; } { A };\n"),
        expected(&[legacy, "values for enums of this type must be initialised"])
    );
    assert_eq!(
        lined("enum accessor A : float { A };\n"),
        expected(&["expected }, found A"])
    );
}
