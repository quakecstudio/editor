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

// spec FTEF-32.7: `va_arg`'s second argument is any type, after a `,`
// spec QCD-3.3: E0203 `"X" is not a type`
// spec QCD-3.4: E0204 "expected X, found Y"
// spec QCD-3.62: E0201 `")" - not a name` for no index
#[test]
fn va_arg_reads_any_type() {
    let body = |statement: &str| {
        diags(&format!(
            "#pragma target fte\nvoid *p; string x; float f; vector w; int i;\n\
             void(...) g = {{ {statement} }};\n"
        ))
    };
    for statement in [
        "x = va_arg(1, string);",
        "w = va_arg(1, vector);",
        "i = va_arg(1, int);",
        "p = va_arg(1, float*);",
    ] {
        assert_eq!(body(statement), [], "{statement}");
    }
    assert_eq!(
        body("f = va_arg(1, string);"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
    assert_eq!(
        body("f = va_arg(i, .float);"),
        [d(Code::E0213, "Cannot cast from .float to float")]
    );
    assert_eq!(
        body("x = va_arg(1);"),
        [d(Code::E0204, "expected ,, found )")]
    );
    assert_eq!(
        body("x = va_arg(1, 2);"),
        [d(Code::E0203, "\"2\" is not a type")]
    );
    assert_eq!(
        body("x = va_arg();"),
        [d(Code::E0201, "\")\" - not a name")]
    );
}

// spec FTEF-32.2: `alloca`, `used_sound` and `used_model` read an argument whatever follows, and
// `alloca` pushes its words by `OP_PUSH`
// spec QCD-4.26: E0320 for the `OP_PUSH` the target refuses
#[test]
fn alloca_pushes_and_intrinsics_need_an_argument() {
    let body = |target: &str, statement: &str| {
        diags(&format!(
            "{target}\nvoid *p; string x; int i;\nvoid() g = {{ {statement} }};\n"
        ))
    };
    let push = d(
        Code::E0320,
        "Opcode \"<PUSH>|PUSH\" not valid for target. Consider the use of: #pragma target fte_6614",
    );
    assert_eq!(
        body("#pragma target fte", "p = alloca(5);"),
        std::slice::from_ref(&push)
    );
    assert_eq!(body("", "p = alloca(i);").last(), Some(&push));
    assert_eq!(body("#pragma target fte_6614", "p = alloca(i);"), []);
    for statement in ["p = alloca();", "x = used_sound();", "x = used_model();"] {
        assert_eq!(
            body("#pragma target fte", statement),
            [d(Code::E0201, "\")\" - not a name")],
            "{statement}"
        );
    }
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

// spec FTEF-17.17: autocvar() is the global of its default's type and value
// spec QCD-4.68: E0367 "autocvar default value is not constant"
// spec QCD-4.69: E0368 "autocvar NAME was already initialised with another value"
#[test]
fn autocvar_calls_give_their_cvar_a_value() {
    let messages = |body: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\nfloat x;\nvoid() f = {{ {body} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    assert_eq!(
        messages("local string s = autocvar(sv_x, 1);"),
        ["Cannot cast from float to string"]
    );
    assert_eq!(
        messages("local float a = autocvar(sv_x, x);"),
        ["autocvar default value is not constant"]
    );
    assert_eq!(
        messages("local float a = autocvar(sv_x, 1); local float b = autocvar(sv_x, 2);"),
        ["autocvar sv_x was already initialised with another value"]
    );
    assert!(
        messages("local float a = autocvar(sv_x, 1); local float b = autocvar(sv_x, 1);")
            .is_empty()
    );
    assert_eq!(
        messages(
            "local string a = autocvar(sv_s, \"a\"); local string b = autocvar(sv_s, \"b\", \"d\");"
        ),
        ["autocvar sv_s was already initialised with another value"]
    );
    // The call defines the global, which the rest of the module refers to and redeclares.
    let module = |source: &str| -> Vec<String> {
        diags(&format!("#pragma target fte\n{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    let call = "void() f = { local float a = autocvar(sv_x, 1); };";
    assert_eq!(
        module(&format!(
            "{call} void() g = {{ local string b = autocvar_sv_x; }};"
        )),
        ["Cannot cast from float to string"]
    );
    assert_eq!(
        module(&format!("{call} string autocvar_sv_x;")),
        ["Type mismatch on redeclaration of autocvar_sv_x. string, should be float"]
    );
    assert_eq!(
        module(&format!("string autocvar_sv_x; {call}")),
        ["Type mismatch on redeclaration of autocvar_sv_x. float, should be string"]
    );
    assert!(module(&format!("{call} float autocvar_sv_x;")).is_empty());
    let changed = |value: &str| module(&format!("{call} float autocvar_sv_x = {value};"));
    assert_eq!(changed("1").len(), 2);
    assert_eq!(
        changed("2").last().map(String::as_str),
        Some("incompatible redeclaration")
    );
}

// spec FTEF-17.19: a path new to its list warns of a backslash, once per list
// spec QCD-7.110: W0317 "backslashes in path names are non-portable - PATH"
#[test]
fn backslashes_in_paths_warn_once_per_list() {
    let messages = |body: &str| -> Vec<String> {
        diags(&format!(
            "entity world;\nvoid(string s) precache_sound = #19;\n\
             void(string s) precache_model = #20;\nvoid(entity e, string m) setmodel = #3;\n\
             void(entity e, float c, string s, float v, float a) sound = #8;\n\
             void(entity e, string s) localsound = #9;\n\
             string P = \"m\\\\p.mdl\";\nvoid() f = {{ {body} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    let warn = |path: &str| format!("backslashes in path names are non-portable - {path}");
    assert_eq!(
        messages(r#"precache_sound("a\\b.wav"); precache_sound("a\\b.wav");"#),
        [warn(r"a\b.wav")]
    );
    assert_eq!(
        messages(r#"precache_sound("a\\b.wav"); precache_model("a\\b.wav");"#),
        [warn(r"a\b.wav"), warn(r"a\b.wav")]
    );
    assert_eq!(
        messages("setmodel(world, P); precache_model(P);"),
        [warn(r"m\p.mdl")]
    );
    assert_eq!(
        messages(r#"sound(world, 0, "c\\d.wav", 1, 1); localsound(world, "c\\d.wav");"#),
        [warn(r"c\d.wav")]
    );
    assert_eq!(messages(r#"used_model("e\\f.mdl");"#)[0], warn(r"e\f.mdl"));
    assert!(messages(r#"precache_sound("a/b.wav"); setmodel(world, "");"#).is_empty());
    assert!(
        messages(r#"setmodel("x\\y.mdl", world);"#)
            .iter()
            .all(|m| !m.starts_with("backslashes"))
    );
}

// spec FTEF-17.20: `spawn(CLASS, field: value)` is a value of the class, its fields the class's
// members or field definitions
// spec QCD-4.74: E0373 "Spawn operator with undefined class: NAME"
// spec QCD-4.75: E0374 "Named field is not a field."
// spec QCD-7.111: W0318 "That = should be a :"
#[test]
fn spawn_takes_a_class_and_its_fields() {
    let messages = |body: &str| -> Vec<(Code, String)> {
        diags(&format!(
            "entity() spawn = #14;\nclass B : entity {{ float b; }};\nclass C : B {{ float x; }};\n\
             class D : entity {{ float w; }};\nstruct S {{ float a; }};\n.float y; entity e; \
             float n;\nvoid() f = {{ {body} }};\n"
        ))
        .into_iter()
        .filter(|(_, message)| !message.contains("spawnfunc"))
        .collect()
    };
    let d = |code: Code, message: &str| (code, message.to_owned());
    let not_field = d(Code::E0374, "Named field is not a field.");
    for fine in [
        "local C c = spawn(C);",
        "local C c = spawn(e, C, x: 1, y: \"s\", b: 3);",
        "e = spawn();",
    ] {
        assert!(messages(fine).is_empty(), "{fine}: {:?}", messages(fine));
    }
    assert_eq!(
        messages("local float v = spawn(C);"),
        [d(Code::E0213, "Cannot cast from class C to float")]
    );
    assert_eq!(
        messages("e = spawn(C, y = 2);"),
        [d(Code::W0318, "That = should be a :")]
    );
    assert_eq!(
        messages("e = spawn(C, n: 2);"),
        std::slice::from_ref(&not_field)
    );
    assert_eq!(
        messages("e = spawn(C, 5);"),
        std::slice::from_ref(&not_field)
    );
    assert_eq!(
        messages("e = spawn(C, w: 1);"),
        [
            d(Code::E0319, "Unknown field \"w\" in class \"C\""),
            not_field
        ]
    );
    assert_eq!(
        messages("e = spawn(n);"),
        [d(Code::E0204, "expected ,, found )")]
    );
    assert_eq!(
        messages("e = spawn(e, n);"),
        [d(Code::E0373, "Spawn operator with undefined class: n")]
    );
    assert_eq!(
        messages("e = spawn(e, x: 1);"),
        [d(Code::E0373, "Spawn operator with undefined class: x")]
    );
    assert_eq!(
        messages("local S s = spawn(S);"),
        [d(Code::E0201, "\")\" - not a name")]
    );
}

// spec FTEF-32.9: an argument left empty passes its parameter's default, freeing its reference
// once too often
// spec QCD-4.76: E0375 "Default value not specified for implicit argument N"
// spec QCD-7.112: W0319 "INTERNAL: over-freed refcount to NAME"
#[test]
fn empty_arguments_take_their_defaults() {
    let messages = |body: &str| -> Vec<(Code, String)> {
        diags(&format!(
            "void(float a, float b = 2, float c = 3) g = {{}};\nvoid(float a, float b) h = {{}};\n\
             void(...) v = #1;\nfloat K = 4;\nvoid(float a, float b = K) k = {{}};\n\
             void(float a, optional float b) o = {{}};\nvoid() f = {{ {body} }};\n"
        ))
    };
    let d = |code: Code, message: &str| (code, message.to_owned());
    let freed = |name: &str| {
        d(
            Code::W0319,
            &format!("INTERNAL: over-freed refcount to {name}"),
        )
    };
    assert_eq!(messages("g(1, , 3);"), [freed("2f")]);
    assert_eq!(messages("g(1, 2, );"), [freed("3f")]);
    assert_eq!(messages("g(1, , );"), [freed("2f"), freed("3f")]);
    assert_eq!(messages("k(1, );"), [freed("K")]);
    let undefined = |n: usize| {
        d(
            Code::E0375,
            &format!("Default value not specified for implicit argument {n}"),
        )
    };
    assert_eq!(messages("g(, 2);"), [undefined(1)]);
    assert_eq!(messages("h(1, );"), [undefined(2)]);
    assert_eq!(messages("o(1, );"), [undefined(2)]);
    let missing = |found: &str| d(Code::E0201, &format!("\"{found}\" - not a name"));
    assert_eq!(messages("g(1, 2, 3, );"), [missing(")")]);
    assert_eq!(messages("v(1, );"), [missing(")")]);
    assert_eq!(messages("v(1, , 2);"), [missing(",")]);
}

// spec FTEF-33.2: `addstat`'s constant type number must be the field's basic type
// (`QCC_VerifyArgs_MatchingFieldType`), read from the argument as passed, so a field converted to
// a `.void` parameter is refused first and a variadic one is checked
#[test]
fn addstat_type_numbers_match_their_fields() {
    let found = |decl: &str, call: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\n{decl}\n.float fh; .string fs; float x;\nvoid() f = {{ {call} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    let variadic = "void(float num, float type, ...) addstat = #232;";
    assert_eq!(
        found(variadic, "addstat(32, 1, fh);"),
        ["addstat: indicated type ev_string does not match passed field type .float"]
    );
    assert_eq!(
        found(variadic, "addstat(32, 2i, fs);"),
        ["addstat: indicated type ev_float does not match passed field type .string"]
    );
    assert_eq!(
        found(variadic, "addstat(32, 99, fs);"),
        ["addstat: indicated type 99 is not a basic type"]
    );
    assert!(found(variadic, "addstat(32, 2, fh);").is_empty());
    assert!(found(variadic, "addstat(32, x, fs);").is_empty());
    let typed = "void(float num, float type, .void fld) addstat = #232;";
    assert_eq!(
        found(typed, "addstat(32, 1, fh);"),
        ["Cannot cast from .float to .void"]
    );
}

// spec FTEF-33.1: a format is a constant string's whole text, its adjacent literals joined, as
// FreeCS builds its shader formats across line continuations
#[test]
fn a_format_constant_is_every_literal_it_joins() {
    let found = |defs: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\nstring(string fmt, ...) sprintf = #627;\n{defs}\n\
             void() f = {{ local string q = sprintf(F, \"a\", 1); }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    assert!(found("const string F = \"{\\n\" \\\n \"%s %f\\n\" \\\n \"}\";").is_empty());
    assert_eq!(
        found("const string F = \"a\" \"%s\";"),
        ["sprintf: surplus trailing float argument(s) for format \"a%s\""]
    );
    // Two spellings of one value redeclare the constant with no error.
    assert_eq!(
        found("const string F = \"a\" \"%s\"; const string F = \"a%s\";"),
        ["sprintf: surplus trailing float argument(s) for format \"a%s\""]
    );
}

// spec FTEF-32.2: `random`'s arguments are supplied as floats, a type that does not convert
// refused
// spec FTEF-27.7: a conversion the target lacks is emulated with its warning, unless the program
// defines a helper of the emulation's name
// spec FTEF-33.1: a format that is no constant is not read
// spec FTEF-31.3: a pointer's string, vector, entity or function is loaded by its own opcode,
// which the id target refuses
// spec FTEF-31.9: a constant vector's component is a constant, by index or by `_z`
#[test]
fn behaviours_real_programs_reach() {
    let found = |target: &str, source: &str| -> Vec<String> {
        diags(&format!("#pragma target {target}\n{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .filter(|m| !m.ends_with("has no body"))
            .collect()
    };
    let defs = "float x; __int i; __uint u; string s; vector v; float() random = #7;\n";
    let body = |statement: &str| format!("{defs}void() f = {{ {statement} }};");
    let mismatch = |got: &str| format!("Implicit type mismatch. Needed float, got {got}.");
    assert_eq!(found("fte", &body("x = random(s);")), [mismatch("string")]);
    assert_eq!(
        found("fte", &body("x = random(1, v);")),
        [mismatch("vector")]
    );
    assert!(found("fte", &body("x = random(i, 2);")).is_empty());
    assert_eq!(
        found("id", &body("x = i;")),
        ["itof emulation: denormals are unsafe i"]
    );
    assert_eq!(
        found("id", &body("x = u;")),
        [
            "itof emulation: denormals are unsafe u",
            "utof emulation: will break if u is big"
        ]
    );
    let helper = "float(__int a) itof = #0;\n";
    assert!(found("id", &format!("{helper}{}", body("x = i;"))).is_empty());
    assert!(found("id", &format!("{helper}{}", body("x = u;"))).is_empty());
    assert!(
        found(
            "fte",
            &format!(
                "string(string f, ...) sprintf = #627;\n{}",
                body("s = sprintf(s, x);")
            )
        )
        .is_empty()
    );
    let refused = |op: &str| {
        format!("Opcode \"=|{op}\" not valid for target. Consider the use of: #pragma target fte")
    };
    let memgetval = "memgetval function not defined: cannot emulate OP_LOADP_*".to_owned();
    assert_eq!(
        found("id", "string *ps; string t;\nvoid() f = { t = *ps; };"),
        [memgetval.clone(), refused("LOADP_S")]
    );
    assert_eq!(
        found("id", "vector *pv; vector w;\nvoid() f = { w = *pv; };"),
        [refused("LOADP_V")]
    );
    assert_eq!(
        found("id", "entity *pe; entity e;\nvoid() f = { e = *pe; };"),
        [memgetval.clone(), refused("LOADP_ENT")]
    );
    let past = "(constant) array index out of bounds (0 <= 3 < 2)";
    let vector = "const vector cv = '1 2 3'; float farr[2]; float y;\n";
    assert_eq!(
        found(
            "fte",
            &format!("{vector}void() f = {{ y = farr[cv[2]]; }};")
        ),
        [past]
    );
    assert_eq!(
        found("fte", &format!("{vector}void() f = {{ y = farr[cv_z]; }};")),
        [past]
    );
}
