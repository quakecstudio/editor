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

/// The definitions every case of [`FORMAT_CASES`] reads.
const FORMAT_DEFS: &str = "string(string fmt, ...) sprintf = #627; void(string evname, string argtypes, ...) sendevent = #359; void(float num, float type, .__variant fld) clientstat = #232; void(float num, float type, .__variant fld) addstat = #232; float(string s) cvar = #45; float f; int i; __uint u; __int64 l; __uint64 ul; __double d; vector v; string s; entity e; .float ff; .vector fv; .string fs; .int fi; .entity fe; string r;";

/// `sprintf` format strings, `sendevent` type lists, `clientstat` and `addstat` type numbers and
/// `cvar` names against arguments of each type, on the FTE target: the pinned FTEQCC's warnings
/// for each (2026-10-28).
const FORMAT_CASES: &[(&str, &[&str])] = &[
    ("r = sprintf(\"%d\", f);", &[]),
    (
        "r = sprintf(\"%d\", i);",
        &["sprintf: %d requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%d\", s);",
        &["sprintf: %d requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%d\", v);",
        &["sprintf: %d requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%d\", e);",
        &["sprintf: %d requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%d\", l);",
        &["sprintf: %d requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%d\", d);",
        &["sprintf: %d requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%d\", u);",
        &["sprintf: %d requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%d\");",
        &["sprintf: %d requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%d\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%d\""],
    ),
    (
        "r = sprintf(\"%i\", f);",
        &[
            "sprintf: %i requires int at arg 2 (got float)",
            "sprintf: %i requires int at arg 2 (got float)",
        ],
    ),
    ("r = sprintf(\"%i\", i);", &[]),
    (
        "r = sprintf(\"%i\", s);",
        &[
            "sprintf: %i requires int at arg 2 (got string)",
            "sprintf: %i requires int at arg 2 (got string)",
        ],
    ),
    (
        "r = sprintf(\"%i\", v);",
        &[
            "sprintf: %i requires int at arg 2 (got vector)",
            "sprintf: %i requires int at arg 2 (got vector)",
        ],
    ),
    ("r = sprintf(\"%i\", e);", &[]),
    (
        "r = sprintf(\"%i\", l);",
        &[
            "sprintf: %i requires int at arg 2 (got __int64)",
            "sprintf: %i requires int at arg 2 (got __int64)",
        ],
    ),
    (
        "r = sprintf(\"%i\", d);",
        &[
            "sprintf: %i requires int at arg 2 (got __double)",
            "sprintf: %i requires int at arg 2 (got __double)",
        ],
    ),
    ("r = sprintf(\"%i\", u);", &[]),
    (
        "r = sprintf(\"%i\");",
        &[
            "sprintf: %i requires int at arg 2 (got void)",
            "sprintf: %i requires int at arg 2 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%i\", f, f);",
        &[
            "sprintf: %i requires int at arg 2 (got float)",
            "sprintf: %i requires int at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%i\"",
        ],
    ),
    ("r = sprintf(\"%f\", f);", &[]),
    (
        "r = sprintf(\"%f\", i);",
        &["sprintf: %f requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%f\", s);",
        &["sprintf: %f requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%f\", v);",
        &["sprintf: %f requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%f\", e);",
        &["sprintf: %f requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%f\", l);",
        &["sprintf: %f requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%f\", d);",
        &["sprintf: %f requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%f\", u);",
        &["sprintf: %f requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%f\");",
        &["sprintf: %f requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%f\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%f\""],
    ),
    ("r = sprintf(\"%g\", f);", &[]),
    (
        "r = sprintf(\"%g\", i);",
        &["sprintf: %g requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%g\", s);",
        &["sprintf: %g requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%g\", v);",
        &["sprintf: %g requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%g\", e);",
        &["sprintf: %g requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%g\", l);",
        &["sprintf: %g requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%g\", d);",
        &["sprintf: %g requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%g\", u);",
        &["sprintf: %g requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%g\");",
        &["sprintf: %g requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%g\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%g\""],
    ),
    (
        "r = sprintf(\"%s\", f);",
        &["sprintf: %s requires string at arg 2 (got float)"],
    ),
    (
        "r = sprintf(\"%s\", i);",
        &["sprintf: %s requires string at arg 2 (got __int32)"],
    ),
    ("r = sprintf(\"%s\", s);", &[]),
    (
        "r = sprintf(\"%s\", v);",
        &["sprintf: %s requires string at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%s\", e);",
        &["sprintf: %s requires string at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%s\", l);",
        &["sprintf: %s requires string at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%s\", d);",
        &["sprintf: %s requires string at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%s\", u);",
        &["sprintf: %s requires string at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%s\");",
        &["sprintf: %s requires string at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%s\", f, f);",
        &[
            "sprintf: %s requires string at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%s\"",
        ],
    ),
    (
        "r = sprintf(\"%v\", f);",
        &["sprintf: %v requires vector at arg 2 (got float)"],
    ),
    (
        "r = sprintf(\"%v\", i);",
        &["sprintf: %v requires vector at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%v\", s);",
        &["sprintf: %v requires vector at arg 2 (got string)"],
    ),
    ("r = sprintf(\"%v\", v);", &[]),
    (
        "r = sprintf(\"%v\", e);",
        &["sprintf: %v requires vector at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%v\", l);",
        &["sprintf: %v requires vector at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%v\", d);",
        &["sprintf: %v requires vector at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%v\", u);",
        &["sprintf: %v requires vector at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%v\");",
        &["sprintf: %v requires vector at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%v\", f, f);",
        &[
            "sprintf: %v requires vector at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%v\"",
        ],
    ),
    ("r = sprintf(\"%x\", f);", &[]),
    (
        "r = sprintf(\"%x\", i);",
        &["sprintf: %x requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%x\", s);",
        &["sprintf: %x requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%x\", v);",
        &["sprintf: %x requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%x\", e);",
        &["sprintf: %x requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%x\", l);",
        &["sprintf: %x requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%x\", d);",
        &["sprintf: %x requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%x\", u);",
        &["sprintf: %x requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%x\");",
        &["sprintf: %x requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%x\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%x\""],
    ),
    ("r = sprintf(\"%c\", f);", &[]),
    (
        "r = sprintf(\"%c\", i);",
        &["sprintf: %c requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%c\", s);",
        &["sprintf: %c requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%c\", v);",
        &["sprintf: %c requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%c\", e);",
        &["sprintf: %c requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%c\", l);",
        &["sprintf: %c requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%c\", d);",
        &["sprintf: %c requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%c\", u);",
        &["sprintf: %c requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%c\");",
        &["sprintf: %c requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%c\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%c\""],
    ),
    ("r = sprintf(\"%e\", f);", &[]),
    (
        "r = sprintf(\"%e\", i);",
        &["sprintf: %e requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%e\", s);",
        &["sprintf: %e requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%e\", v);",
        &["sprintf: %e requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%e\", e);",
        &["sprintf: %e requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%e\", l);",
        &["sprintf: %e requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%e\", d);",
        &["sprintf: %e requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%e\", u);",
        &["sprintf: %e requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%e\");",
        &["sprintf: %e requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%e\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%e\""],
    ),
    ("r = sprintf(\"%u\", f);", &[]),
    (
        "r = sprintf(\"%u\", i);",
        &["sprintf: %u requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%u\", s);",
        &["sprintf: %u requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%u\", v);",
        &["sprintf: %u requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%u\", e);",
        &["sprintf: %u requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%u\", l);",
        &["sprintf: %u requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%u\", d);",
        &["sprintf: %u requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%u\", u);",
        &["sprintf: %u requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%u\");",
        &["sprintf: %u requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%u\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%u\""],
    ),
    (
        "r = sprintf(\"%ld\", f);",
        &[
            "sprintf: %ld requires int at arg 2 (got float)",
            "sprintf: %ld requires int at arg 2 (got float)",
        ],
    ),
    ("r = sprintf(\"%ld\", i);", &[]),
    (
        "r = sprintf(\"%ld\", s);",
        &[
            "sprintf: %ld requires int at arg 2 (got string)",
            "sprintf: %ld requires int at arg 2 (got string)",
        ],
    ),
    (
        "r = sprintf(\"%ld\", v);",
        &[
            "sprintf: %ld requires int at arg 2 (got vector)",
            "sprintf: %ld requires int at arg 2 (got vector)",
        ],
    ),
    (
        "r = sprintf(\"%ld\", e);",
        &[
            "sprintf: %ld requires int at arg 2 (got entity)",
            "sprintf: %ld requires int at arg 2 (got entity)",
        ],
    ),
    (
        "r = sprintf(\"%ld\", l);",
        &[
            "sprintf: %ld requires int at arg 2 (got __int64)",
            "sprintf: %ld requires int at arg 2 (got __int64)",
        ],
    ),
    (
        "r = sprintf(\"%ld\", d);",
        &[
            "sprintf: %ld requires int at arg 2 (got __double)",
            "sprintf: %ld requires int at arg 2 (got __double)",
        ],
    ),
    ("r = sprintf(\"%ld\", u);", &[]),
    (
        "r = sprintf(\"%ld\");",
        &[
            "sprintf: %ld requires int at arg 2 (got void)",
            "sprintf: %ld requires int at arg 2 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%ld\", f, f);",
        &[
            "sprintf: %ld requires int at arg 2 (got float)",
            "sprintf: %ld requires int at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%ld\"",
        ],
    ),
    (
        "r = sprintf(\"%lld\", f);",
        &[
            "sprintf: %lld requires int at arg 2 (got float)",
            "sprintf: %lld requires int at arg 2 (got float)",
        ],
    ),
    ("r = sprintf(\"%lld\", i);", &[]),
    (
        "r = sprintf(\"%lld\", s);",
        &[
            "sprintf: %lld requires int at arg 2 (got string)",
            "sprintf: %lld requires int at arg 2 (got string)",
        ],
    ),
    (
        "r = sprintf(\"%lld\", v);",
        &[
            "sprintf: %lld requires int at arg 2 (got vector)",
            "sprintf: %lld requires int at arg 2 (got vector)",
        ],
    ),
    (
        "r = sprintf(\"%lld\", e);",
        &[
            "sprintf: %lld requires int at arg 2 (got entity)",
            "sprintf: %lld requires int at arg 2 (got entity)",
        ],
    ),
    (
        "r = sprintf(\"%lld\", l);",
        &[
            "sprintf: %lld requires int at arg 2 (got __int64)",
            "sprintf: %lld requires int at arg 2 (got __int64)",
        ],
    ),
    (
        "r = sprintf(\"%lld\", d);",
        &[
            "sprintf: %lld requires int at arg 2 (got __double)",
            "sprintf: %lld requires int at arg 2 (got __double)",
        ],
    ),
    ("r = sprintf(\"%lld\", u);", &[]),
    (
        "r = sprintf(\"%lld\");",
        &[
            "sprintf: %lld requires int at arg 2 (got void)",
            "sprintf: %lld requires int at arg 2 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%lld\", f, f);",
        &[
            "sprintf: %lld requires int at arg 2 (got float)",
            "sprintf: %lld requires int at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%lld\"",
        ],
    ),
    ("r = sprintf(\"%hd\", f);", &[]),
    (
        "r = sprintf(\"%hd\", i);",
        &["sprintf: %hd requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%hd\", s);",
        &["sprintf: %hd requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%hd\", v);",
        &["sprintf: %hd requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%hd\", e);",
        &["sprintf: %hd requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%hd\", l);",
        &["sprintf: %hd requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%hd\", d);",
        &["sprintf: %hd requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%hd\", u);",
        &["sprintf: %hd requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%hd\");",
        &["sprintf: %hd requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%hd\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%hd\""],
    ),
    (
        "r = sprintf(\"%qd\", f);",
        &["sprintf: %qd requires double at arg 2 (got float)"],
    ),
    (
        "r = sprintf(\"%qd\", i);",
        &["sprintf: %qd requires double at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%qd\", s);",
        &["sprintf: %qd requires double at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%qd\", v);",
        &["sprintf: %qd requires double at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%qd\", e);",
        &["sprintf: %qd requires double at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%qd\", l);",
        &["sprintf: %qd requires double at arg 2 (got __int64)"],
    ),
    ("r = sprintf(\"%qd\", d);", &[]),
    (
        "r = sprintf(\"%qd\", u);",
        &["sprintf: %qd requires double at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%qd\");",
        &["sprintf: %qd requires double at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%qd\", f, f);",
        &[
            "sprintf: %qd requires double at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%qd\"",
        ],
    ),
    ("r = sprintf(\"%5.2f\", f);", &[]),
    (
        "r = sprintf(\"%5.2f\", i);",
        &["sprintf: %5.2f requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%5.2f\", s);",
        &["sprintf: %5.2f requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%5.2f\", v);",
        &["sprintf: %5.2f requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%5.2f\", e);",
        &["sprintf: %5.2f requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%5.2f\", l);",
        &["sprintf: %5.2f requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%5.2f\", d);",
        &["sprintf: %5.2f requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%5.2f\", u);",
        &["sprintf: %5.2f requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%5.2f\");",
        &["sprintf: %5.2f requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%5.2f\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%5.2f\""],
    ),
    ("r = sprintf(\"%-5d\", f);", &[]),
    (
        "r = sprintf(\"%-5d\", i);",
        &["sprintf: %-5d requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%-5d\", s);",
        &["sprintf: %-5d requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%-5d\", v);",
        &["sprintf: %-5d requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%-5d\", e);",
        &["sprintf: %-5d requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%-5d\", l);",
        &["sprintf: %-5d requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%-5d\", d);",
        &["sprintf: %-5d requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%-5d\", u);",
        &["sprintf: %-5d requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%-5d\");",
        &["sprintf: %-5d requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%-5d\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%-5d\""],
    ),
    (
        "r = sprintf(\"%*d\", f);",
        &["sprintf: %*d requires float at arg 3 (got void)"],
    ),
    (
        "r = sprintf(\"%*d\", i);",
        &[
            "sprintf: width modifier requires float at arg 2",
            "sprintf: %*d requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%*d\", s);",
        &[
            "sprintf: width modifier requires float at arg 2",
            "sprintf: %*d requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%*d\", v);",
        &[
            "sprintf: width modifier requires float at arg 2",
            "sprintf: %*d requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%*d\", e);",
        &[
            "sprintf: width modifier requires float at arg 2",
            "sprintf: %*d requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%*d\", l);",
        &[
            "sprintf: width modifier requires float at arg 2",
            "sprintf: %*d requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%*d\", d);",
        &[
            "sprintf: width modifier requires float at arg 2",
            "sprintf: %*d requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%*d\", u);",
        &[
            "sprintf: width modifier requires float at arg 2",
            "sprintf: %*d requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%*d\");",
        &[
            "sprintf: width modifier requires float at arg 2",
            "sprintf: %*d requires float at arg 3 (got void)",
        ],
    ),
    ("r = sprintf(\"%*d\", f, f);", &[]),
    (
        "r = sprintf(\"%.*f\", f);",
        &["sprintf: %.*f requires float at arg 3 (got void)"],
    ),
    (
        "r = sprintf(\"%.*f\", i);",
        &[
            "sprintf: precision modifier requires float at arg 2",
            "sprintf: %.*f requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%.*f\", s);",
        &[
            "sprintf: precision modifier requires float at arg 2",
            "sprintf: %.*f requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%.*f\", v);",
        &[
            "sprintf: precision modifier requires float at arg 2",
            "sprintf: %.*f requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%.*f\", e);",
        &[
            "sprintf: precision modifier requires float at arg 2",
            "sprintf: %.*f requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%.*f\", l);",
        &[
            "sprintf: precision modifier requires float at arg 2",
            "sprintf: %.*f requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%.*f\", d);",
        &[
            "sprintf: precision modifier requires float at arg 2",
            "sprintf: %.*f requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%.*f\", u);",
        &[
            "sprintf: precision modifier requires float at arg 2",
            "sprintf: %.*f requires float at arg 3 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%.*f\");",
        &[
            "sprintf: precision modifier requires float at arg 2",
            "sprintf: %.*f requires float at arg 3 (got void)",
        ],
    ),
    ("r = sprintf(\"%.*f\", f, f);", &[]),
    (
        "r = sprintf(\"%%\", f);",
        &["sprintf: surplus trailing float argument(s) for format \"%%\""],
    ),
    (
        "r = sprintf(\"%%\", i);",
        &["sprintf: surplus trailing __int32 argument(s) for format \"%%\""],
    ),
    (
        "r = sprintf(\"%%\", s);",
        &["sprintf: surplus trailing string argument(s) for format \"%%\""],
    ),
    (
        "r = sprintf(\"%%\", v);",
        &["sprintf: surplus trailing vector argument(s) for format \"%%\""],
    ),
    (
        "r = sprintf(\"%%\", e);",
        &["sprintf: surplus trailing entity argument(s) for format \"%%\""],
    ),
    (
        "r = sprintf(\"%%\", l);",
        &["sprintf: surplus trailing __int64 argument(s) for format \"%%\""],
    ),
    (
        "r = sprintf(\"%%\", d);",
        &["sprintf: surplus trailing __double argument(s) for format \"%%\""],
    ),
    (
        "r = sprintf(\"%%\", u);",
        &["sprintf: surplus trailing __uint argument(s) for format \"%%\""],
    ),
    ("r = sprintf(\"%%\");", &[]),
    (
        "r = sprintf(\"%%\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%%\""],
    ),
    (
        "r = sprintf(\"%p\", f);",
        &["sprintf: %p requires pointer at arg 2 (got float)"],
    ),
    (
        "r = sprintf(\"%p\", i);",
        &["sprintf: %p requires pointer at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%p\", s);",
        &["sprintf: %p requires pointer at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%p\", v);",
        &["sprintf: %p requires pointer at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%p\", e);",
        &["sprintf: %p requires pointer at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%p\", l);",
        &["sprintf: %p requires pointer at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%p\", d);",
        &["sprintf: %p requires pointer at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%p\", u);",
        &["sprintf: %p requires pointer at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%p\");",
        &["sprintf: %p requires pointer at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%p\", f, f);",
        &[
            "sprintf: %p requires pointer at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%p\"",
        ],
    ),
    ("r = sprintf(\"%X\", f);", &[]),
    (
        "r = sprintf(\"%X\", i);",
        &["sprintf: %X requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%X\", s);",
        &["sprintf: %X requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%X\", v);",
        &["sprintf: %X requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%X\", e);",
        &["sprintf: %X requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%X\", l);",
        &["sprintf: %X requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%X\", d);",
        &["sprintf: %X requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%X\", u);",
        &["sprintf: %X requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%X\");",
        &["sprintf: %X requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%X\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%X\""],
    ),
    ("r = sprintf(\"%o\", f);", &[]),
    (
        "r = sprintf(\"%o\", i);",
        &["sprintf: %o requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%o\", s);",
        &["sprintf: %o requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%o\", v);",
        &["sprintf: %o requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%o\", e);",
        &["sprintf: %o requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%o\", l);",
        &["sprintf: %o requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%o\", d);",
        &["sprintf: %o requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%o\", u);",
        &["sprintf: %o requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%o\");",
        &["sprintf: %o requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%o\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%o\""],
    ),
    (
        "r = sprintf(\"%n\", f);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\", i);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\", s);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\", v);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\", e);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\", l);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\", d);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\", u);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\");",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%n\", f, f);",
        &["sprintf: invalid format string: %n"],
    ),
    (
        "r = sprintf(\"%a\", f);",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\", i);",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\", s);",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\", v);",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\", e);",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\", l);",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\", d);",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\", u);",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\");",
        &["sprintf: invalid format string: %a"],
    ),
    (
        "r = sprintf(\"%a\", f, f);",
        &["sprintf: invalid format string: %a"],
    ),
    ("r = sprintf(\"%E\", f);", &[]),
    (
        "r = sprintf(\"%E\", i);",
        &["sprintf: %E requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%E\", s);",
        &["sprintf: %E requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%E\", v);",
        &["sprintf: %E requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%E\", e);",
        &["sprintf: %E requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%E\", l);",
        &["sprintf: %E requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%E\", d);",
        &["sprintf: %E requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%E\", u);",
        &["sprintf: %E requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%E\");",
        &["sprintf: %E requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%E\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%E\""],
    ),
    ("r = sprintf(\"%G\", f);", &[]),
    (
        "r = sprintf(\"%G\", i);",
        &["sprintf: %G requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%G\", s);",
        &["sprintf: %G requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%G\", v);",
        &["sprintf: %G requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%G\", e);",
        &["sprintf: %G requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%G\", l);",
        &["sprintf: %G requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%G\", d);",
        &["sprintf: %G requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%G\", u);",
        &["sprintf: %G requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%G\");",
        &["sprintf: %G requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%G\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%G\""],
    ),
    ("r = sprintf(\"%hhd\", f);", &[]),
    (
        "r = sprintf(\"%hhd\", i);",
        &["sprintf: %hhd requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%hhd\", s);",
        &["sprintf: %hhd requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%hhd\", v);",
        &["sprintf: %hhd requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%hhd\", e);",
        &["sprintf: %hhd requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%hhd\", l);",
        &["sprintf: %hhd requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%hhd\", d);",
        &["sprintf: %hhd requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%hhd\", u);",
        &["sprintf: %hhd requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%hhd\");",
        &["sprintf: %hhd requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%hhd\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%hhd\""],
    ),
    (
        "r = sprintf(\"%lu\", f);",
        &[
            "sprintf: %lu requires int at arg 2 (got float)",
            "sprintf: %lu requires int at arg 2 (got float)",
        ],
    ),
    ("r = sprintf(\"%lu\", i);", &[]),
    (
        "r = sprintf(\"%lu\", s);",
        &[
            "sprintf: %lu requires int at arg 2 (got string)",
            "sprintf: %lu requires int at arg 2 (got string)",
        ],
    ),
    (
        "r = sprintf(\"%lu\", v);",
        &[
            "sprintf: %lu requires int at arg 2 (got vector)",
            "sprintf: %lu requires int at arg 2 (got vector)",
        ],
    ),
    (
        "r = sprintf(\"%lu\", e);",
        &[
            "sprintf: %lu requires int at arg 2 (got entity)",
            "sprintf: %lu requires int at arg 2 (got entity)",
        ],
    ),
    (
        "r = sprintf(\"%lu\", l);",
        &[
            "sprintf: %lu requires int at arg 2 (got __int64)",
            "sprintf: %lu requires int at arg 2 (got __int64)",
        ],
    ),
    (
        "r = sprintf(\"%lu\", d);",
        &[
            "sprintf: %lu requires int at arg 2 (got __double)",
            "sprintf: %lu requires int at arg 2 (got __double)",
        ],
    ),
    ("r = sprintf(\"%lu\", u);", &[]),
    (
        "r = sprintf(\"%lu\");",
        &[
            "sprintf: %lu requires int at arg 2 (got void)",
            "sprintf: %lu requires int at arg 2 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%lu\", f, f);",
        &[
            "sprintf: %lu requires int at arg 2 (got float)",
            "sprintf: %lu requires int at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%lu\"",
        ],
    ),
    (
        "r = sprintf(\"%jd\", f);",
        &["sprintf: length modifier j is a placeholder"],
    ),
    (
        "r = sprintf(\"%jd\", i);",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: %jd requires float at arg 2 (got __int32)",
        ],
    ),
    (
        "r = sprintf(\"%jd\", s);",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: %jd requires float at arg 2 (got string)",
        ],
    ),
    (
        "r = sprintf(\"%jd\", v);",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: %jd requires float at arg 2 (got vector)",
        ],
    ),
    (
        "r = sprintf(\"%jd\", e);",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: %jd requires float at arg 2 (got entity)",
        ],
    ),
    (
        "r = sprintf(\"%jd\", l);",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: %jd requires float at arg 2 (got __int64)",
        ],
    ),
    (
        "r = sprintf(\"%jd\", d);",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: %jd requires float at arg 2 (got __double)",
        ],
    ),
    (
        "r = sprintf(\"%jd\", u);",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: %jd requires float at arg 2 (got __uint)",
        ],
    ),
    (
        "r = sprintf(\"%jd\");",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: %jd requires float at arg 2 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%jd\", f, f);",
        &[
            "sprintf: length modifier j is a placeholder",
            "sprintf: surplus trailing float argument(s) for format \"%jd\"",
        ],
    ),
    (
        "r = sprintf(\"%zd\", f);",
        &["sprintf: length modifier z is a placeholder"],
    ),
    (
        "r = sprintf(\"%zd\", i);",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: %zd requires float at arg 2 (got __int32)",
        ],
    ),
    (
        "r = sprintf(\"%zd\", s);",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: %zd requires float at arg 2 (got string)",
        ],
    ),
    (
        "r = sprintf(\"%zd\", v);",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: %zd requires float at arg 2 (got vector)",
        ],
    ),
    (
        "r = sprintf(\"%zd\", e);",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: %zd requires float at arg 2 (got entity)",
        ],
    ),
    (
        "r = sprintf(\"%zd\", l);",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: %zd requires float at arg 2 (got __int64)",
        ],
    ),
    (
        "r = sprintf(\"%zd\", d);",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: %zd requires float at arg 2 (got __double)",
        ],
    ),
    (
        "r = sprintf(\"%zd\", u);",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: %zd requires float at arg 2 (got __uint)",
        ],
    ),
    (
        "r = sprintf(\"%zd\");",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: %zd requires float at arg 2 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%zd\", f, f);",
        &[
            "sprintf: length modifier z is a placeholder",
            "sprintf: surplus trailing float argument(s) for format \"%zd\"",
        ],
    ),
    (
        "r = sprintf(\"%Lf\", f);",
        &[
            "sprintf: %Lf requires int at arg 2 (got float)",
            "sprintf: %Lf requires int at arg 2 (got float)",
        ],
    ),
    ("r = sprintf(\"%Lf\", i);", &[]),
    (
        "r = sprintf(\"%Lf\", s);",
        &[
            "sprintf: %Lf requires int at arg 2 (got string)",
            "sprintf: %Lf requires int at arg 2 (got string)",
        ],
    ),
    (
        "r = sprintf(\"%Lf\", v);",
        &[
            "sprintf: %Lf requires int at arg 2 (got vector)",
            "sprintf: %Lf requires int at arg 2 (got vector)",
        ],
    ),
    (
        "r = sprintf(\"%Lf\", e);",
        &[
            "sprintf: %Lf requires int at arg 2 (got entity)",
            "sprintf: %Lf requires int at arg 2 (got entity)",
        ],
    ),
    (
        "r = sprintf(\"%Lf\", l);",
        &[
            "sprintf: %Lf requires int at arg 2 (got __int64)",
            "sprintf: %Lf requires int at arg 2 (got __int64)",
        ],
    ),
    (
        "r = sprintf(\"%Lf\", d);",
        &[
            "sprintf: %Lf requires int at arg 2 (got __double)",
            "sprintf: %Lf requires int at arg 2 (got __double)",
        ],
    ),
    ("r = sprintf(\"%Lf\", u);", &[]),
    (
        "r = sprintf(\"%Lf\");",
        &[
            "sprintf: %Lf requires int at arg 2 (got void)",
            "sprintf: %Lf requires int at arg 2 (got void)",
        ],
    ),
    (
        "r = sprintf(\"%Lf\", f, f);",
        &[
            "sprintf: %Lf requires int at arg 2 (got float)",
            "sprintf: %Lf requires int at arg 2 (got float)",
            "sprintf: surplus trailing float argument(s) for format \"%Lf\"",
        ],
    ),
    ("r = sprintf(\"%#x\", f);", &[]),
    (
        "r = sprintf(\"%#x\", i);",
        &["sprintf: %#x requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%#x\", s);",
        &["sprintf: %#x requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%#x\", v);",
        &["sprintf: %#x requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%#x\", e);",
        &["sprintf: %#x requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%#x\", l);",
        &["sprintf: %#x requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%#x\", d);",
        &["sprintf: %#x requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%#x\", u);",
        &["sprintf: %#x requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%#x\");",
        &["sprintf: %#x requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%#x\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%#x\""],
    ),
    ("r = sprintf(\"%+d\", f);", &[]),
    (
        "r = sprintf(\"%+d\", i);",
        &["sprintf: %+d requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%+d\", s);",
        &["sprintf: %+d requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%+d\", v);",
        &["sprintf: %+d requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%+d\", e);",
        &["sprintf: %+d requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%+d\", l);",
        &["sprintf: %+d requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%+d\", d);",
        &["sprintf: %+d requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%+d\", u);",
        &["sprintf: %+d requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%+d\");",
        &["sprintf: %+d requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%+d\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%+d\""],
    ),
    ("r = sprintf(\"% d\", f);", &[]),
    (
        "r = sprintf(\"% d\", i);",
        &["sprintf: % d requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"% d\", s);",
        &["sprintf: % d requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"% d\", v);",
        &["sprintf: % d requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"% d\", e);",
        &["sprintf: % d requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"% d\", l);",
        &["sprintf: % d requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"% d\", d);",
        &["sprintf: % d requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"% d\", u);",
        &["sprintf: % d requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"% d\");",
        &["sprintf: % d requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"% d\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"% d\""],
    ),
    ("r = sprintf(\"%05d\", f);", &[]),
    (
        "r = sprintf(\"%05d\", i);",
        &["sprintf: %05d requires float at arg 2 (got __int32)"],
    ),
    (
        "r = sprintf(\"%05d\", s);",
        &["sprintf: %05d requires float at arg 2 (got string)"],
    ),
    (
        "r = sprintf(\"%05d\", v);",
        &["sprintf: %05d requires float at arg 2 (got vector)"],
    ),
    (
        "r = sprintf(\"%05d\", e);",
        &["sprintf: %05d requires float at arg 2 (got entity)"],
    ),
    (
        "r = sprintf(\"%05d\", l);",
        &["sprintf: %05d requires float at arg 2 (got __int64)"],
    ),
    (
        "r = sprintf(\"%05d\", d);",
        &["sprintf: %05d requires float at arg 2 (got __double)"],
    ),
    (
        "r = sprintf(\"%05d\", u);",
        &["sprintf: %05d requires float at arg 2 (got __uint)"],
    ),
    (
        "r = sprintf(\"%05d\");",
        &["sprintf: %05d requires float at arg 2 (got void)"],
    ),
    (
        "r = sprintf(\"%05d\", f, f);",
        &["sprintf: surplus trailing float argument(s) for format \"%05d\""],
    ),
    ("sendevent(\"ev\", \"s\", s);", &[]),
    (
        "sendevent(\"ev\", \"s\", f);",
        &["sendevent: 's' specified, but was passed float"],
    ),
    (
        "sendevent(\"ev\", \"s\", d);",
        &["sendevent: 's' specified, but was passed __double"],
    ),
    (
        "sendevent(\"ev\", \"s\", i);",
        &["sendevent: 's' specified, but was passed __int32"],
    ),
    (
        "sendevent(\"ev\", \"s\", l);",
        &["sendevent: 's' specified, but was passed __int64"],
    ),
    (
        "sendevent(\"ev\", \"s\", u);",
        &["sendevent: 's' specified, but was passed __uint"],
    ),
    (
        "sendevent(\"ev\", \"s\", ul);",
        &["sendevent: 's' specified, but was passed __uint64"],
    ),
    (
        "sendevent(\"ev\", \"s\", v);",
        &["sendevent: 's' specified, but was passed vector"],
    ),
    (
        "sendevent(\"ev\", \"s\", e);",
        &["sendevent: 's' specified, but was passed entity"],
    ),
    (
        "sendevent(\"ev\", \"s\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"s\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 's' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"f\", s);",
        &["sendevent: 'f' specified, but was passed string"],
    ),
    ("sendevent(\"ev\", \"f\", f);", &[]),
    (
        "sendevent(\"ev\", \"f\", d);",
        &["sendevent: 'f' specified, but was passed __double"],
    ),
    (
        "sendevent(\"ev\", \"f\", i);",
        &["sendevent: 'f' specified, but was passed __int32"],
    ),
    (
        "sendevent(\"ev\", \"f\", l);",
        &["sendevent: 'f' specified, but was passed __int64"],
    ),
    (
        "sendevent(\"ev\", \"f\", u);",
        &["sendevent: 'f' specified, but was passed __uint"],
    ),
    (
        "sendevent(\"ev\", \"f\", ul);",
        &["sendevent: 'f' specified, but was passed __uint64"],
    ),
    (
        "sendevent(\"ev\", \"f\", v);",
        &["sendevent: 'f' specified, but was passed vector"],
    ),
    (
        "sendevent(\"ev\", \"f\", e);",
        &["sendevent: 'f' specified, but was passed entity"],
    ),
    (
        "sendevent(\"ev\", \"f\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"f\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
        ],
    ),
    (
        "sendevent(\"ev\", \"F\", s);",
        &["sendevent: 'F' specified, but was passed string"],
    ),
    (
        "sendevent(\"ev\", \"F\", f);",
        &["sendevent: 'F' specified, but was passed float"],
    ),
    ("sendevent(\"ev\", \"F\", d);", &[]),
    (
        "sendevent(\"ev\", \"F\", i);",
        &["sendevent: 'F' specified, but was passed __int32"],
    ),
    (
        "sendevent(\"ev\", \"F\", l);",
        &["sendevent: 'F' specified, but was passed __int64"],
    ),
    (
        "sendevent(\"ev\", \"F\", u);",
        &["sendevent: 'F' specified, but was passed __uint"],
    ),
    (
        "sendevent(\"ev\", \"F\", ul);",
        &["sendevent: 'F' specified, but was passed __uint64"],
    ),
    (
        "sendevent(\"ev\", \"F\", v);",
        &["sendevent: 'F' specified, but was passed vector"],
    ),
    (
        "sendevent(\"ev\", \"F\", e);",
        &["sendevent: 'F' specified, but was passed entity"],
    ),
    (
        "sendevent(\"ev\", \"F\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"F\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 'F' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"i\", s);",
        &["sendevent: 'i' specified, but was passed string"],
    ),
    (
        "sendevent(\"ev\", \"i\", f);",
        &["sendevent: 'i' specified, but was passed float"],
    ),
    (
        "sendevent(\"ev\", \"i\", d);",
        &["sendevent: 'i' specified, but was passed __double"],
    ),
    ("sendevent(\"ev\", \"i\", i);", &[]),
    (
        "sendevent(\"ev\", \"i\", l);",
        &["sendevent: 'i' specified, but was passed __int64"],
    ),
    (
        "sendevent(\"ev\", \"i\", u);",
        &["sendevent: 'i' specified, but was passed __uint"],
    ),
    (
        "sendevent(\"ev\", \"i\", ul);",
        &["sendevent: 'i' specified, but was passed __uint64"],
    ),
    (
        "sendevent(\"ev\", \"i\", v);",
        &["sendevent: 'i' specified, but was passed vector"],
    ),
    (
        "sendevent(\"ev\", \"i\", e);",
        &["sendevent: 'i' specified, but was passed entity"],
    ),
    (
        "sendevent(\"ev\", \"i\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"i\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 'i' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"I\", s);",
        &["sendevent: 'I' specified, but was passed string"],
    ),
    (
        "sendevent(\"ev\", \"I\", f);",
        &["sendevent: 'I' specified, but was passed float"],
    ),
    (
        "sendevent(\"ev\", \"I\", d);",
        &["sendevent: 'I' specified, but was passed __double"],
    ),
    (
        "sendevent(\"ev\", \"I\", i);",
        &["sendevent: 'I' specified, but was passed __int32"],
    ),
    ("sendevent(\"ev\", \"I\", l);", &[]),
    (
        "sendevent(\"ev\", \"I\", u);",
        &["sendevent: 'I' specified, but was passed __uint"],
    ),
    (
        "sendevent(\"ev\", \"I\", ul);",
        &["sendevent: 'I' specified, but was passed __uint64"],
    ),
    (
        "sendevent(\"ev\", \"I\", v);",
        &["sendevent: 'I' specified, but was passed vector"],
    ),
    (
        "sendevent(\"ev\", \"I\", e);",
        &["sendevent: 'I' specified, but was passed entity"],
    ),
    (
        "sendevent(\"ev\", \"I\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"I\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 'I' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"u\", s);",
        &["sendevent: 'u' specified, but was passed string"],
    ),
    (
        "sendevent(\"ev\", \"u\", f);",
        &["sendevent: 'u' specified, but was passed float"],
    ),
    (
        "sendevent(\"ev\", \"u\", d);",
        &["sendevent: 'u' specified, but was passed __double"],
    ),
    (
        "sendevent(\"ev\", \"u\", i);",
        &["sendevent: 'u' specified, but was passed __int32"],
    ),
    (
        "sendevent(\"ev\", \"u\", l);",
        &["sendevent: 'u' specified, but was passed __int64"],
    ),
    ("sendevent(\"ev\", \"u\", u);", &[]),
    (
        "sendevent(\"ev\", \"u\", ul);",
        &["sendevent: 'u' specified, but was passed __uint64"],
    ),
    (
        "sendevent(\"ev\", \"u\", v);",
        &["sendevent: 'u' specified, but was passed vector"],
    ),
    (
        "sendevent(\"ev\", \"u\", e);",
        &["sendevent: 'u' specified, but was passed entity"],
    ),
    (
        "sendevent(\"ev\", \"u\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"u\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 'u' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"U\", s);",
        &["sendevent: 'U' specified, but was passed string"],
    ),
    (
        "sendevent(\"ev\", \"U\", f);",
        &["sendevent: 'U' specified, but was passed float"],
    ),
    (
        "sendevent(\"ev\", \"U\", d);",
        &["sendevent: 'U' specified, but was passed __double"],
    ),
    (
        "sendevent(\"ev\", \"U\", i);",
        &["sendevent: 'U' specified, but was passed __int32"],
    ),
    (
        "sendevent(\"ev\", \"U\", l);",
        &["sendevent: 'U' specified, but was passed __int64"],
    ),
    (
        "sendevent(\"ev\", \"U\", u);",
        &["sendevent: 'U' specified, but was passed __uint"],
    ),
    ("sendevent(\"ev\", \"U\", ul);", &[]),
    (
        "sendevent(\"ev\", \"U\", v);",
        &["sendevent: 'U' specified, but was passed vector"],
    ),
    (
        "sendevent(\"ev\", \"U\", e);",
        &["sendevent: 'U' specified, but was passed entity"],
    ),
    (
        "sendevent(\"ev\", \"U\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"U\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 'U' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"v\", s);",
        &["sendevent: 'v' specified, but was passed string"],
    ),
    (
        "sendevent(\"ev\", \"v\", f);",
        &["sendevent: 'v' specified, but was passed float"],
    ),
    (
        "sendevent(\"ev\", \"v\", d);",
        &["sendevent: 'v' specified, but was passed __double"],
    ),
    (
        "sendevent(\"ev\", \"v\", i);",
        &["sendevent: 'v' specified, but was passed __int32"],
    ),
    (
        "sendevent(\"ev\", \"v\", l);",
        &["sendevent: 'v' specified, but was passed __int64"],
    ),
    (
        "sendevent(\"ev\", \"v\", u);",
        &["sendevent: 'v' specified, but was passed __uint"],
    ),
    (
        "sendevent(\"ev\", \"v\", ul);",
        &["sendevent: 'v' specified, but was passed __uint64"],
    ),
    ("sendevent(\"ev\", \"v\", v);", &[]),
    (
        "sendevent(\"ev\", \"v\", e);",
        &["sendevent: 'v' specified, but was passed entity"],
    ),
    (
        "sendevent(\"ev\", \"v\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"v\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 'v' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"e\", s);",
        &["sendevent: 'e' specified, but was passed string"],
    ),
    (
        "sendevent(\"ev\", \"e\", f);",
        &["sendevent: 'e' specified, but was passed float"],
    ),
    (
        "sendevent(\"ev\", \"e\", d);",
        &["sendevent: 'e' specified, but was passed __double"],
    ),
    (
        "sendevent(\"ev\", \"e\", i);",
        &["sendevent: 'e' specified, but was passed __int32"],
    ),
    (
        "sendevent(\"ev\", \"e\", l);",
        &["sendevent: 'e' specified, but was passed __int64"],
    ),
    (
        "sendevent(\"ev\", \"e\", u);",
        &["sendevent: 'e' specified, but was passed __uint"],
    ),
    (
        "sendevent(\"ev\", \"e\", ul);",
        &["sendevent: 'e' specified, but was passed __uint64"],
    ),
    (
        "sendevent(\"ev\", \"e\", v);",
        &["sendevent: 'e' specified, but was passed vector"],
    ),
    ("sendevent(\"ev\", \"e\", e);", &[]),
    (
        "sendevent(\"ev\", \"e\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"e\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 'e' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"x\", s);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\", f);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\", d);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\", i);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\", l);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\", u);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\", ul);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\", v);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\", e);",
        &["sendevent: 'x' is not a recognised arg type"],
    ),
    (
        "sendevent(\"ev\", \"x\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"x\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 'x' is not a recognised arg type",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\", s);",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"sf\", f);",
        &[
            "sendevent: 's' specified, but was passed float",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\", d);",
        &[
            "sendevent: 's' specified, but was passed __double",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\", i);",
        &[
            "sendevent: 's' specified, but was passed __int32",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\", l);",
        &[
            "sendevent: 's' specified, but was passed __int64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\", u);",
        &[
            "sendevent: 's' specified, but was passed __uint",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\", ul);",
        &[
            "sendevent: 's' specified, but was passed __uint64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\", v);",
        &[
            "sendevent: 's' specified, but was passed vector",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\", e);",
        &[
            "sendevent: 's' specified, but was passed entity",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sf\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"sf\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 's' specified, but was passed float",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\", s);",
        &[
            "sendevent: 'f' specified, but was passed string",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\", f);",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"ffff\", d);",
        &[
            "sendevent: 'f' specified, but was passed __double",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\", i);",
        &[
            "sendevent: 'f' specified, but was passed __int32",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\", l);",
        &[
            "sendevent: 'f' specified, but was passed __int64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\", u);",
        &[
            "sendevent: 'f' specified, but was passed __uint",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\", ul);",
        &[
            "sendevent: 'f' specified, but was passed __uint64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\", v);",
        &[
            "sendevent: 'f' specified, but was passed vector",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\", e);",
        &[
            "sendevent: 'f' specified, but was passed entity",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffff\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"ffff\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", s);",
        &[
            "sendevent: 'f' specified, but was passed string",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", f);",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", d);",
        &[
            "sendevent: 'f' specified, but was passed __double",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", i);",
        &[
            "sendevent: 'f' specified, but was passed __int32",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", l);",
        &[
            "sendevent: 'f' specified, but was passed __int64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", u);",
        &[
            "sendevent: 'f' specified, but was passed __uint",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", ul);",
        &[
            "sendevent: 'f' specified, but was passed __uint64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", v);",
        &[
            "sendevent: 'f' specified, but was passed vector",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", e);",
        &[
            "sendevent: 'f' specified, but was passed entity",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"fffffff\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"fffffff\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: cannot pass more than 8 args to a builtin.",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", s);",
        &[
            "sendevent: 'f' specified, but was passed string",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", f);",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", d);",
        &[
            "sendevent: 'f' specified, but was passed __double",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", i);",
        &[
            "sendevent: 'f' specified, but was passed __int32",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", l);",
        &[
            "sendevent: 'f' specified, but was passed __int64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", u);",
        &[
            "sendevent: 'f' specified, but was passed __uint",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", ul);",
        &[
            "sendevent: 'f' specified, but was passed __uint64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", v);",
        &[
            "sendevent: 'f' specified, but was passed vector",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", e);",
        &[
            "sendevent: 'f' specified, but was passed entity",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"ffffffff\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: cannot pass more than 8 args to a builtin.",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\", s);",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"sss\", f);",
        &[
            "sendevent: 's' specified, but was passed float",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\", d);",
        &[
            "sendevent: 's' specified, but was passed __double",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\", i);",
        &[
            "sendevent: 's' specified, but was passed __int32",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\", l);",
        &[
            "sendevent: 's' specified, but was passed __int64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\", u);",
        &[
            "sendevent: 's' specified, but was passed __uint",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\", ul);",
        &[
            "sendevent: 's' specified, but was passed __uint64",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\", v);",
        &[
            "sendevent: 's' specified, but was passed vector",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\", e);",
        &[
            "sendevent: 's' specified, but was passed entity",
            "sendevent: arg type list longer than args",
        ],
    ),
    (
        "sendevent(\"ev\", \"sss\");",
        &["sendevent: arg type list longer than args"],
    ),
    (
        "sendevent(\"ev\", \"sss\", f, f, f, f, f, f, f, f);",
        &[
            "More than 8 parameters on varargs function",
            "More than 8 parameters on varargs function",
            "sendevent: 's' specified, but was passed float",
            "sendevent: 's' specified, but was passed float",
            "sendevent: 's' specified, but was passed float",
        ],
    ),
    ("clientstat(1, 0, ff);", &[]),
    ("addstat(1, 0, ff);", &[]),
    ("clientstat(1, 0, fv);", &[]),
    ("addstat(1, 0, fv);", &[]),
    ("clientstat(1, 0, fs);", &[]),
    ("addstat(1, 0, fs);", &[]),
    ("clientstat(1, 0, fi);", &[]),
    ("addstat(1, 0, fi);", &[]),
    ("clientstat(1, 0, fe);", &[]),
    ("addstat(1, 0, fe);", &[]),
    ("clientstat(1, 1, ff);", &[]),
    ("addstat(1, 1, ff);", &[]),
    ("clientstat(1, 1, fv);", &[]),
    ("addstat(1, 1, fv);", &[]),
    ("clientstat(1, 1, fs);", &[]),
    ("addstat(1, 1, fs);", &[]),
    ("clientstat(1, 1, fi);", &[]),
    ("addstat(1, 1, fi);", &[]),
    ("clientstat(1, 1, fe);", &[]),
    ("addstat(1, 1, fe);", &[]),
    ("clientstat(1, 2, ff);", &[]),
    ("addstat(1, 2, ff);", &[]),
    ("clientstat(1, 2, fv);", &[]),
    ("addstat(1, 2, fv);", &[]),
    ("clientstat(1, 2, fs);", &[]),
    ("addstat(1, 2, fs);", &[]),
    ("clientstat(1, 2, fi);", &[]),
    ("addstat(1, 2, fi);", &[]),
    ("clientstat(1, 2, fe);", &[]),
    ("addstat(1, 2, fe);", &[]),
    ("clientstat(1, 3, ff);", &[]),
    ("addstat(1, 3, ff);", &[]),
    ("clientstat(1, 3, fv);", &[]),
    ("addstat(1, 3, fv);", &[]),
    ("clientstat(1, 3, fs);", &[]),
    ("addstat(1, 3, fs);", &[]),
    ("clientstat(1, 3, fi);", &[]),
    ("addstat(1, 3, fi);", &[]),
    ("clientstat(1, 3, fe);", &[]),
    ("addstat(1, 3, fe);", &[]),
    ("clientstat(1, 4, ff);", &[]),
    ("addstat(1, 4, ff);", &[]),
    ("clientstat(1, 4, fv);", &[]),
    ("addstat(1, 4, fv);", &[]),
    ("clientstat(1, 4, fs);", &[]),
    ("addstat(1, 4, fs);", &[]),
    ("clientstat(1, 4, fi);", &[]),
    ("addstat(1, 4, fi);", &[]),
    ("clientstat(1, 4, fe);", &[]),
    ("addstat(1, 4, fe);", &[]),
    ("clientstat(1, 5, ff);", &[]),
    ("addstat(1, 5, ff);", &[]),
    ("clientstat(1, 5, fv);", &[]),
    ("addstat(1, 5, fv);", &[]),
    ("clientstat(1, 5, fs);", &[]),
    ("addstat(1, 5, fs);", &[]),
    ("clientstat(1, 5, fi);", &[]),
    ("addstat(1, 5, fi);", &[]),
    ("clientstat(1, 5, fe);", &[]),
    ("addstat(1, 5, fe);", &[]),
    ("clientstat(1, 8, ff);", &[]),
    ("addstat(1, 8, ff);", &[]),
    ("clientstat(1, 8, fv);", &[]),
    ("addstat(1, 8, fv);", &[]),
    ("clientstat(1, 8, fs);", &[]),
    ("addstat(1, 8, fs);", &[]),
    ("clientstat(1, 8, fi);", &[]),
    ("addstat(1, 8, fi);", &[]),
    ("clientstat(1, 8, fe);", &[]),
    ("addstat(1, 8, fe);", &[]),
    ("clientstat(1, 9, ff);", &[]),
    ("addstat(1, 9, ff);", &[]),
    ("clientstat(1, 9, fv);", &[]),
    ("addstat(1, 9, fv);", &[]),
    ("clientstat(1, 9, fs);", &[]),
    ("addstat(1, 9, fs);", &[]),
    ("clientstat(1, 9, fi);", &[]),
    ("addstat(1, 9, fi);", &[]),
    ("clientstat(1, 9, fe);", &[]),
    ("addstat(1, 9, fe);", &[]),
    ("clientstat(1, 12, ff);", &[]),
    ("addstat(1, 12, ff);", &[]),
    ("clientstat(1, 12, fv);", &[]),
    ("addstat(1, 12, fv);", &[]),
    ("clientstat(1, 12, fs);", &[]),
    ("addstat(1, 12, fs);", &[]),
    ("clientstat(1, 12, fi);", &[]),
    ("addstat(1, 12, fi);", &[]),
    ("clientstat(1, 12, fe);", &[]),
    ("addstat(1, 12, fe);", &[]),
    ("clientstat(1, 13, ff);", &[]),
    ("addstat(1, 13, ff);", &[]),
    ("clientstat(1, 13, fv);", &[]),
    ("addstat(1, 13, fv);", &[]),
    ("clientstat(1, 13, fs);", &[]),
    ("addstat(1, 13, fs);", &[]),
    ("clientstat(1, 13, fi);", &[]),
    ("addstat(1, 13, fi);", &[]),
    ("clientstat(1, 13, fe);", &[]),
    ("addstat(1, 13, fe);", &[]),
    ("clientstat(1, -1, ff);", &[]),
    ("addstat(1, -1, ff);", &[]),
    ("clientstat(1, -1, fv);", &[]),
    ("addstat(1, -1, fv);", &[]),
    ("clientstat(1, -1, fs);", &[]),
    ("addstat(1, -1, fs);", &[]),
    ("clientstat(1, -1, fi);", &[]),
    ("addstat(1, -1, fi);", &[]),
    ("clientstat(1, -1, fe);", &[]),
    ("addstat(1, -1, fe);", &[]),
    ("clientstat(1, 2.5, ff);", &[]),
    ("addstat(1, 2.5, ff);", &[]),
    ("clientstat(1, 2.5, fv);", &[]),
    ("addstat(1, 2.5, fv);", &[]),
    ("clientstat(1, 2.5, fs);", &[]),
    ("addstat(1, 2.5, fs);", &[]),
    ("clientstat(1, 2.5, fi);", &[]),
    ("addstat(1, 2.5, fi);", &[]),
    ("clientstat(1, 2.5, fe);", &[]),
    ("addstat(1, 2.5, fe);", &[]),
    ("clientstat(1, 2i, ff);", &[]),
    ("addstat(1, 2i, ff);", &[]),
    ("clientstat(1, 2i, fv);", &[]),
    ("addstat(1, 2i, fv);", &[]),
    ("clientstat(1, 2i, fs);", &[]),
    ("addstat(1, 2i, fs);", &[]),
    ("clientstat(1, 2i, fi);", &[]),
    ("addstat(1, 2i, fi);", &[]),
    ("clientstat(1, 2i, fe);", &[]),
    ("addstat(1, 2i, fe);", &[]),
    ("clientstat(1, 8i, ff);", &[]),
    ("addstat(1, 8i, ff);", &[]),
    ("clientstat(1, 8i, fv);", &[]),
    ("addstat(1, 8i, fv);", &[]),
    ("clientstat(1, 8i, fs);", &[]),
    ("addstat(1, 8i, fs);", &[]),
    ("clientstat(1, 8i, fi);", &[]),
    ("addstat(1, 8i, fi);", &[]),
    ("clientstat(1, 8i, fe);", &[]),
    ("addstat(1, 8i, fe);", &[]),
    (
        "f = cvar(\"vid_conwidth\");",
        &[
            "cvar: cvar(\"vid_conwidth\") is deprecated and is likely to give some hacky value to work around old API usage which does not necessarily reflect the actual cvar value. Use getviewprop(VF_SCREENVSIZE) for the screen's virtual use, or use ftos+cvar_string to read the actual value of the cvar, or cast to variant to mute this warning.",
        ],
    ),
    (
        "f = cvar(\"vid_conheight\");",
        &[
            "cvar: cvar(\"vid_conheight\") is deprecated and is likely to give some hacky value to work around old API usage which does not necessarily reflect the actual cvar value. Use getviewprop(VF_SCREENVSIZE) for the screen's virtual use, or use ftos+cvar_string to read the actual value of the cvar, or cast to variant to mute this warning.",
        ],
    ),
    ("f = cvar(\"x\");", &[]),
    ("f = cvar(s);", &[]),
];

// spec FTEF-33.1: each format string's directives against the arguments, as FTEQCC checks them
// spec FTEF-33.2: each event type list and stat type number against the arguments
// spec FTEF-19.54: `__int`, `__uint` and the core types' `__` names are types of their own names
#[test]
fn every_builtin_argument_check_is_fteqccs() {
    for &(statement, expected) in FORMAT_CASES {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        db.set_file(
            FILE,
            PathBuf::from("t.qc"),
            format!("#pragma target fte\n{FORMAT_DEFS}\nvoid() g = {{ {statement} }};\n")
                .into_bytes(),
        );
        let found: Vec<String> = db
            .analyze_module(&ModuleInput {
                compiled: vec![FILE],
                command_line: CommandLine::new::<&str>(&[]),
                ..ModuleInput::default()
            })
            .diagnostics()
            .map(|d| d.diagnostic.message.clone())
            .collect();
        assert_eq!(found, expected, "{statement}");
    }
}
