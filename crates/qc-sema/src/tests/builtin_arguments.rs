// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks FTEQCC makes of some builtins' arguments (`docs/spec/dialect-fteqcc.md`
//! FTEF-33). Each case's warnings are the pinned FTEQCC's (2026-10-27), without the notes it
//! prints after them.

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

fn diags(statement: &str) -> Vec<(Code, String)> {
    let source = format!(
        "const string FMT = \"%s\"; const float VF_ORIGIN = 11; .float fl;\n\
         string(string fmt, ...) sprintf = #627; float(string n) cvar = #45;\n\
         void(float p, ...) setviewprop = #300;\n\
         void(float n, float t, .__variant f) addstat = #232;\n\
         void(string ev, string types, ...) sendevent = #359;\n\
         void() g = {{ local float a; local vector v; local string s; local entity e; \
         local int i; {statement} }};\n"
    );
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.into_bytes());
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

fn format(message: &str) -> (Code, String) {
    (Code::W0254, format!("sprintf: {message}"))
}

fn argument(message: &str) -> (Code, String) {
    (Code::W0255, message.to_owned())
}

// spec FTEF-33.1: each directive's argument is checked against its conversion, floats by
// default and integers for `%i`, `%p` and the `l` modifier
// spec QCD-7.96: W0254 for a format string's warnings
#[test]
fn format_conversions() {
    for quiet in [
        "s = sprintf(\"%d\", a);",
        "s = sprintf(\"%i\", i);",
        "s = sprintf(\"%f %s\", a, s);",
        "s = sprintf(\"%v\", v);",
        "s = sprintf(\"%.*f\", a, a);",
        "s = sprintf(\"%5.2f\", a);",
        "s = sprintf(\"%2$s %1$f\", a, s);",
        "s = sprintf(\"%i\", e);",
    ] {
        assert_eq!(diags(quiet), [], "{quiet}");
    }
    let cases: [(&str, &[&str]); 7] = [
        (
            "s = sprintf(\"%s\", a);",
            &["%s requires string at arg 2 (got float)"],
        ),
        (
            "s = sprintf(\"%g\", s);",
            &["%g requires float at arg 2 (got string)"],
        ),
        (
            "s = sprintf(\"%v\", a);",
            &["%v requires vector at arg 2 (got float)"],
        ),
        (
            "s = sprintf(\"%x\", i);",
            &["%x requires float at arg 2 (got __int32)"],
        ),
        (
            "s = sprintf(\"%lx\", a);",
            &[
                "%lx requires int at arg 2 (got float)",
                "%lx requires int at arg 2 (got float)",
            ],
        ),
        (
            "s = sprintf(FMT, a);",
            &["%s requires string at arg 2 (got float)"],
        ),
        (
            "s = sprintf(\"%d %d\", a);",
            &["%d requires float at arg 3 (got void)"],
        ),
    ];
    for (statement, messages) in cases {
        let expected: Vec<_> = messages.iter().map(|m| format(m)).collect();
        assert_eq!(diags(statement), expected, "{statement}");
    }
}

// spec FTEF-33.1: a surplus argument, a bad directive, a `*` that is no float and a
// placeholder length modifier warn
#[test]
fn format_strings() {
    let cases: [(&str, &[&str]); 6] = [
        (
            "s = sprintf(\"%d\", a, a);",
            &["surplus trailing float argument(s) for format \"%d\""],
        ),
        (
            "s = sprintf(\"%%\", a);",
            &["surplus trailing float argument(s) for format \"%%\""],
        ),
        ("s = sprintf(\"%q\", a);", &["invalid format string: %q"]),
        ("s = sprintf(\"%\", a);", &["invalid format string: %"]),
        (
            "s = sprintf(\"%*f\", s, a);",
            &["width modifier requires float at arg 2"],
        ),
        (
            "s = sprintf(\"%zd\", i);",
            &[
                "length modifier z is a placeholder",
                "%zd requires float at arg 2 (got __int32)",
            ],
        ),
    ];
    for (statement, messages) in cases {
        let expected: Vec<_> = messages.iter().map(|m| format(m)).collect();
        assert_eq!(diags(statement), expected, "{statement}");
    }
}

// spec FTEF-33.2: `cvar`'s deprecated names, `setviewprop`'s property types and unknown
// properties, and `sendevent`'s type list
// spec QCD-7.97: W0255 for a builtin's argument check
#[test]
fn builtin_arguments() {
    for quiet in [
        "setviewprop(VF_ORIGIN, v);",
        "addstat(10, 2, fl);",
        "sendevent(\"ev\", \"fsv\", a, s, v);",
    ] {
        assert_eq!(diags(quiet), [], "{quiet}");
    }
    let deprecated = "cvar: cvar(\"vid_conwidth\") is deprecated and is likely to give some \
                      hacky value to work around old API usage which does not necessarily \
                      reflect the actual cvar value. Use getviewprop(VF_SCREENVSIZE) for the \
                      screen's virtual use, or use ftos+cvar_string to read the actual value of \
                      the cvar, or cast to variant to mute this warning.";
    let cases = [
        ("a = cvar(\"vid_conwidth\");", deprecated),
        (
            "setviewprop(VF_ORIGIN, a);",
            "setviewprop(VF_ORIGIN, ...): expected vector, got float",
        ),
        ("setviewprop(999, v);", "setviewprop: unknown argument 999"),
        (
            "sendevent(\"ev\", \"ff\", a);",
            "sendevent: arg type list longer than args",
        ),
        (
            "sendevent(\"ev\", \"fx\", a, a);",
            "sendevent: 'x' is not a recognised arg type",
        ),
        (
            "sendevent(\"ev\", \"s\", a);",
            "sendevent: 's' specified, but was passed float",
        ),
    ];
    for (statement, message) in cases {
        assert_eq!(diags(statement), [argument(message)], "{statement}");
    }
}
