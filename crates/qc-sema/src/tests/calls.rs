// SPDX-License-Identifier: GPL-3.0-or-later

//! Calls as FTEQCC checks them (`docs/spec/dialect-fteqcc.md` FTEF-32). Each case's
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

fn diags(source: &str) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

const DEFS: &str = "entity self; .void() think; float() random = #7; void(float x) f1 = {}; \
                    void(float x, float y = 2) d = {}; void(float x, optional float y) o = {}; \
                    void(...) va = {}; void() vf = {}; float(float x) ff = { return x; };\n";

fn body(statement: &str) -> Vec<(Code, String)> {
    diags(&format!(
        "{DEFS}void() g = {{ local float a; local string s; local int i; {statement} }};\n"
    ))
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

// spec FTEF-32.1: a call of a value that is no function is "not a function", a fatal error
// spec FTEF-19.40: a call of a name whose type is a value's is "not a function"
#[test]
fn not_a_function() {
    assert_eq!(body("a = a();"), [d(Code::E0256, "not a function")]);
}

// spec FTEF-32.3: an argument is cast to its parameter's type, and converted
#[test]
fn arguments_are_cast() {
    for quiet in [
        "d(1);",
        "o(1);",
        "a = ff(ff(1));",
        "self.think();",
        "a = random(1, 2);",
    ] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
    assert_eq!(
        body("f1(s);"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
    assert_eq!(
        body("f1(i);"),
        [d(Code::E0125, "itof emulation: denormals are unsafe i")]
    );
    assert_eq!(
        body("a = vf();"),
        [d(Code::E0213, "Cannot cast from void to float")]
    );
}

// spec FTEF-32.3: an argument past the last parameter is ignored with a warning naming it; a
// varargs call warns past 8; a missing argument with no default is "UNDEFINED"
// spec QCD-7.93: W0251 for more than 8 arguments to a varargs function
// spec QCD-7.94: W0252 for too many arguments
// spec QCD-7.95: W0253 for too few arguments
#[test]
fn argument_counts() {
    assert_eq!(
        body("f1(1, 2);"),
        [d(
            Code::W0252,
            "too many parameters on call to f1, argument IMMEDIATE will be ignored"
        )]
    );
    assert_eq!(
        body("f1(1, s);"),
        [d(
            Code::W0252,
            "too many parameters on call to f1, argument s will be ignored"
        )]
    );
    assert_eq!(
        body("va(1,2,3,4,5,6,7,8,9);"),
        [d(Code::W0251, "More than 8 parameters on varargs function")]
    );
    for short in [("f1();", "f1"), ("d();", "d")] {
        assert_eq!(
            body(short.0),
            [d(
                Code::W0253,
                &format!(
                    "too few parameters on call to {}, x will be UNDEFINED",
                    short.1
                )
            )],
            "{}",
            short.0
        );
    }
}

// spec FTEF-32.2: the intrinsics a function with no parameters stands for: `_` takes a string
// immediate, `random`'s arguments are made floats
// spec QCD-4.46: E0340 for an intrinsic's argument refused
// spec QCD-3.27: E0227 for an argument with no conversion to the intrinsic's type
#[test]
fn intrinsics() {
    assert_eq!(body("s = _(\"x\");"), []);
    assert_eq!(
        body("s = _(s);"),
        [d(
            Code::E0340,
            "_() intrinsic accepts only a string immediate"
        )]
    );
    assert_eq!(
        body("a = random(s);"),
        [d(
            Code::E0227,
            "Implicit type mismatch. Needed float, got string."
        )]
    );
}

// spec FTEF-32.4: a local's initialiser is read and cast to the local's type
#[test]
fn local_initialisers() {
    assert_eq!(
        body("local float y = \"s\";"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
    assert_eq!(
        body("local float y = i;"),
        [d(Code::E0125, "itof emulation: denormals are unsafe i")]
    );
    assert_eq!(
        body("local vector v = [1, s, 2];"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
}

// spec FTEF-32.5: in a member function a field named bare is `this.field`, but `::name` is the
// field itself, which a field parameter takes
#[test]
fn scoped_fields_in_member_functions() {
    let source = |statement: &str| {
        format!(
            "entity self, world; .float entnum; .string targetname;\n\
             entity(entity s, .__variant f, __variant m) findfloat = #98;\n\
             entity(entity s, .string f, string m) find = #18;\n\
             class C {{ nonvirtual void() m; }};\n\
             void() C::m = {{ local entity x; {statement} }};\n"
        )
    };
    assert_eq!(diags(&source("x = find(world, ::targetname, \"a\");")), []);
    assert_eq!(diags(&source("x = findfloat(world, ::entnum, 1);")), []);
    assert_eq!(
        diags(&source("x = find(world, targetname, \"a\");")),
        [d(Code::E0213, "Cannot cast from string to .string")]
    );
}

// spec FTEF-32.6: an `out` argument is stored back after the call; a read-only one is the error
// "Unable to write to read-only out argument"
// spec QCD-4.45: E0339 for a read-only out argument
#[test]
fn out_arguments() {
    let source = |statement: &str| {
        format!(
            "const float k = 3; void(__out float x) of = {{ x = 1; }};\n\
             void() g = {{ local float a; {statement} }};\n"
        )
    };
    assert_eq!(diags(&source("of(a);")), []);
    for read_only in ["of(1);", "of(k);"] {
        assert_eq!(
            diags(&source(read_only)),
            [d(Code::E0339, "Unable to write to read-only out argument")],
            "{read_only}"
        );
    }
}

// spec FTEF-32.3: a call passes at most 136 arguments; past 8, a varargs call warns at each
// spec QCD-4.44: E0338 for more than 136 arguments
#[test]
fn the_most_arguments() {
    let args = vec!["1"; 137].join(",");
    let found = diags(&format!(
        "void(...) va = {{}}; void() g = {{ va({args}); }};\n"
    ));
    let warnings = found.iter().filter(|(c, _)| *c == Code::W0251).count();
    assert_eq!(warnings, 128);
    assert_eq!(
        found.last(),
        Some(&d(Code::E0338, "More than 136 parameters"))
    );
}

// spec FTEF-32.7: `va_arg(index, TYPE)` reads a type, makes its index an integer, and works only
// in a function with `...`
// spec QCD-4.52: E0346 for `va_arg` outside a variadic function
#[test]
fn va_arg_intrinsic() {
    assert_eq!(
        diags("void(...) v = { local float x = va_arg(0, float); };\n"),
        []
    );
    assert_eq!(
        diags("void() v = { local float x = va_arg(0, float); };\n"),
        [d(
            Code::E0346,
            "va_arg() intrinsic only works inside varadic functions"
        )]
    );
    assert_eq!(
        diags("void(...) v = { local string x = va_arg(0, float); };\n"),
        [d(Code::E0213, "Cannot cast from float to string")]
    );
    assert_eq!(
        diags("void(...) v = { local float x = va_arg(\"s\", float); };\n"),
        [d(
            Code::E0227,
            "Implicit type mismatch. Needed integer, got string."
        )]
    );
}

// spec FTEF-32.2: `randomv` takes vectors as they are; `entnum` needs `nextent`
// spec QCD-4.53: E0347 for `entnum` with no `nextent`
// spec FTEF-24.9: an intrinsic's name or `nil` after `.` is no field
#[test]
fn randomv_entnum_and_intrinsic_fields() {
    assert_eq!(
        diags("float a; void() g = { a = randomv(1, 2); };\n"),
        [d(Code::E0340, "type mismatch on parm 1")]
    );
    assert_eq!(diags("vector v; void() g = { v = randomv(v, v); };\n"), []);
    assert_eq!(
        diags("entity e; float a; void() g = { a = entnum(e); };\n"),
        [d(Code::E0347, "the nextent builtin is not defined")]
    );
    assert_eq!(
        diags(
            "#pragma target fte\nentity e; float a; entity(entity x) nextent = #47;\nvoid() g = { a = entnum(e); };\n"
        ),
        []
    );
    for (name, shown) in [
        ("entnum", "entnum"),
        ("nil", "IMMEDIATE"),
        ("random", "random"),
    ] {
        assert_eq!(
            diags(&format!(
                "entity e; float a; void() g = {{ a = e.{name}; }};\n"
            )),
            [d(Code::E0318, &format!("{shown} is not a field"))],
            "{name}"
        );
    }
}

// spec FTEF-19.45: an unknown `autocvar_` name in a body warns that it is promoted to a global
#[test]
fn an_unknown_autocvar_in_a_body() {
    assert_eq!(
        diags("float y; void() g = { y = autocvar_x; };\n"),
        [
            d(
                Code::W0213,
                "Autocvar \"autocvar_x\" defined with local scope. promoting to global."
            ),
            d(Code::E0301, "Unknown value \"autocvar_x\".")
        ]
    );
}

// spec FTEF-32.8: a call's value is called no further; the `(` after it ends the expression,
// and the construct around it expects its own token
#[test]
fn a_call_of_a_call_ends_the_expression() {
    assert_eq!(body("ff(1)(2);"), [d(Code::E0204, "expected ;, found (")]);
    assert_eq!(
        body("a = (ff(1)(2));"),
        [d(Code::E0204, "expected ), found (")]
    );
    assert_eq!(
        body("f1(ff(1)(2));"),
        [d(Code::E0204, "expected ), found (")]
    );
}
