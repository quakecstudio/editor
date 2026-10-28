// SPDX-License-Identifier: GPL-3.0-or-later

//! The flags and the target the semantic model's casts and conversions read
//! (`docs/spec/dialect-fteqcc.md` FTEF-22), and the pointer casts of FTEF-19.31. Each program is
//! also a case of the cast family of `cargo xtask oracles builtins`, which compares the outcome
//! with the pinned FTEQCC.

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

/// The one file these tests read.
const FILE: FileId = FileId(0);

/// The diagnostics of the FTEQCC file `source`, preprocessed and read with the command line
/// `args`.
fn analysed(source: &str, args: &[&str]) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new(args),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

/// The diagnostics of [`analysed`] with no option.
fn diags(source: &str) -> Vec<(Code, String)> {
    analysed(source, &[])
}

/// The codes of [`analysed`] with the options `args`.
fn codes_with(source: &str, args: &[&str]) -> Vec<Code> {
    analysed(source, args).into_iter().map(|(c, _)| c).collect()
}

/// The codes of [`analysed`] with no option.
fn codes(source: &str) -> Vec<Code> {
    codes_with(source, &[])
}

fn one(code: Code, message: &str) -> Vec<(Code, String)> {
    vec![(code, message.to_owned())]
}

// spec QCD-7.70: W0229 "Implicit lax cast from TYPE to TYPE" for a cast `lax` lets through
// spec FTEF-22.1: under `lax` a cast FTEQCC refuses is the warning "Implicit lax cast", in an
// initialiser, a default value, a local's initialiser and a strict enum's value
#[test]
fn lax_makes_refused_casts_warnings() {
    let lax = "#pragma flag enable lax\n";
    assert_eq!(
        diags(&format!("{lax}float x = \"a\";\n")),
        one(Code::W0229, "Implicit lax cast from string to float")
    );
    assert_eq!(
        diags(&format!("{lax}void() g = 5;\n")),
        one(Code::W0229, "Implicit lax cast from float to void()")
    );
    assert_eq!(
        diags(&format!("{lax}.float fl = 1;\n")),
        one(Code::W0229, "Implicit lax cast from float to .float")
    );
    assert_eq!(codes(&format!("{lax}string s = '1 2 3';\n")), [Code::W0229]);
    assert_eq!(
        codes(&format!("{lax}void() g = {{ float x = \"a\"; }};\n")),
        [Code::W0229]
    );
    assert_eq!(
        codes(&format!("{lax}float(float a = \"a\") g = #2;\n")),
        [Code::W0229]
    );
    assert_eq!(codes("float(float a = \"a\") g = #2;\n"), [Code::E0213]);
    assert_eq!(
        diags(&format!(
            "{lax}enum class E : float {{ A }};\nfloat x = E::A;\n"
        )),
        one(Code::W0229, "Implicit lax cast from enum E:float to float")
    );
    assert_eq!(
        codes("enum class E : float { A };\nfloat x = E::A;\n"),
        [Code::E0213]
    );
    // The flag holds from where the pragma sets it to where one clears it.
    assert_eq!(
        codes(&format!(
            "{lax}float x = \"a\";\n#pragma flag disable lax\nfloat y = \"a\";\n"
        )),
        [Code::W0229, Code::E0213]
    );
    assert_eq!(codes_with("float x = \"a\";\n", &["-Flax"]), [Code::W0229]);
    // `-Werror` makes the warning an error.
    assert_eq!(
        codes_with(&format!("{lax}float x = \"a\";\n"), &["-Werror"]),
        [Code::E0125]
    );
    // A pragma in an `extern "C"` block that names no saved flag holds after it.
    assert_eq!(
        codes("extern \"C\" {\n#pragma flag enable lax\n}\nfloat x = \"a\";\n"),
        [Code::W0229]
    );
}

// spec FTEF-22.6: a value is cast with the flags at the token after it
#[test]
fn flags_at_the_token_after_the_value() {
    assert_eq!(
        codes("float x =\n#pragma flag enable lax\n\"a\";\n"),
        [Code::W0229]
    );
    assert_eq!(
        codes("float x = \"a\"\n#pragma flag enable lax\n;\n"),
        [Code::W0229]
    );
    assert_eq!(
        codes("#pragma flag enable lax\n#pragma flag disable lax\nfloat y = \"a\";\n"),
        [Code::E0213]
    );
}

// spec FTEF-22.2: under `qccx` a float casts to an entity or a string and back, with
// "Implicit cast"; a double to an entity and a string to a vector stay refused
#[test]
fn qccx_casts_floats_entities_and_strings() {
    let qccx = "#pragma flag enable qccx\n";
    assert_eq!(
        diags(&format!("{qccx}float x = \"a\";\n")),
        one(Code::W0220, "Implicit cast from string to float")
    );
    assert_eq!(
        diags(&format!("{qccx}entity e = 1;\n")),
        one(Code::W0220, "Implicit cast from float to entity")
    );
    assert_eq!(codes(&format!("{qccx}entity e = 1.5;\n")), [Code::W0220]);
    assert_eq!(
        diags(&format!("{qccx}string s = 1;\n")),
        one(Code::W0220, "Implicit cast from float to string")
    );
    assert_eq!(
        diags(&format!("{qccx}entity e = 1i;\n")),
        one(Code::W0220, "Implicit cast from __int32 to entity")
    );
    assert_eq!(codes(&format!("{qccx}entity e = 1d;\n")), [Code::E0213]);
    assert_eq!(codes(&format!("{qccx}vector v = \"a\";\n")), [Code::E0213]);
    assert_eq!(
        codes(&format!("{qccx}void() g = {{ entity e; float x = e; }};\n")),
        [Code::W0220]
    );
    assert_eq!(
        codes("void() g = { entity e; float x = e; };\n"),
        [Code::E0213]
    );
    // `-Fqccx` and `-std=qccx` ignore F314, the pragma does not.
    assert_eq!(codes_with("float x = \"a\";\n", &["-Fqccx"]), []);
    assert_eq!(codes_with("float x = \"a\";\n", &["-std=qccx"]), []);
}

// spec QCD-3.36: E0236 "Automatic type conversions disabled." under `typeexplicit`
// spec FTEF-22.3: `typeexplicit` refuses any conversion of an enum's value or a state frame,
// and reads no initialiser's cast
#[test]
fn typeexplicit_refuses_conversions() {
    let explicit = "#pragma flag enable typeexplicit\n";
    let message = |got: &str| {
        one(
            Code::E0236,
            &format!("Automatic type conversions disabled. Needed float, got {got}."),
        )
    };
    assert_eq!(
        diags(&format!("{explicit}enum {{ A = 1i }};\n")),
        message("integer")
    );
    assert_eq!(
        diags(&format!("{explicit}enum {{ A = 0i }};\n")),
        message("integer")
    );
    assert_eq!(
        diags(&format!("{explicit}enum {{ A = '1 2 3' }};\n")),
        message("vector")
    );
    assert_eq!(codes(&format!("{explicit}enum {{ A = 1 }};\n")), []);
    assert_eq!(
        diags(&format!(
            "{explicit}#pragma flag enable assumeint\nenum {{ A = 1.5 }};\n"
        )),
        one(
            Code::E0236,
            "Automatic type conversions disabled. Needed integer, got float."
        )
    );
    assert_eq!(codes(&format!("{explicit}float x = 1i;\n")), []);
    assert_eq!(
        diags(&format!("{explicit}void() g = [1i, g] {{}};\n")),
        message("integer")
    );
    assert_eq!(codes("void() g = [1i, g] {};\n"), []);
    assert_eq!(
        codes(&format!(
            "{explicit}const float c = 1;\nenum {{ A = c }};\n"
        )),
        []
    );
    assert_eq!(
        codes_with("enum { A = 1i };\n", &["-Ftypeexplicit"]),
        [Code::E0236]
    );
}

// spec QCD-7.71: W0230 "Implicit type mismatch." under `lax`
// spec QCD-3.38: E0238 "state type mismatch" for a cycle's frame `lax` let through
// spec FTEF-22.3: a value nothing converts is "Implicit type mismatch", a warning under `lax`
// that keeps the value, the enum's name then counted as with no value, and a cycle's frame
// then "state type mismatch"; the kinds are named by `basictypenames`
#[test]
fn lax_conversions_and_mismatches() {
    let lax = "#pragma flag enable lax\n";
    let mismatch = "Implicit type mismatch. Needed float, got string.";
    assert_eq!(diags("enum { A = \"a\" };\n"), one(Code::E0227, mismatch));
    assert_eq!(
        diags(&format!("{lax}enum {{ A = \"a\" }};\n")),
        one(Code::W0230, mismatch)
    );
    assert_eq!(
        codes(&format!(
            "{lax}enum {{ A = \"a\", B }};\nfloat x = B;\nfloat x = 2;\n"
        )),
        [Code::W0230, Code::E0363]
    );
    assert_eq!(
        codes(&format!(
            "{lax}enum {{ A = \"a\", B }};\nfloat x = B;\nfloat x = 1;\n"
        )),
        [Code::W0230]
    );
    assert_eq!(
        diags("#pragma flag enable assumeint\nenum { A = \"a\" };\n"),
        one(
            Code::E0227,
            "Implicit type mismatch. Needed integer, got string."
        )
    );
    assert_eq!(
        diags("void() g = [\"a\", g] {};\n"),
        one(Code::E0227, mismatch)
    );
    assert_eq!(
        codes(&format!("{lax}void() g = [\"a\", g] {{}};\n")),
        [Code::W0230]
    );
    assert_eq!(
        diags(&format!("{lax}void() g = [++ \"a\" .. 2] {{}};\n")),
        vec![
            (Code::W0230, mismatch.to_owned()),
            (Code::E0238, "state type mismatch".to_owned())
        ]
    );
    // A name converts by its definition's type, before its constness is read.
    assert_eq!(
        codes("const string s = \"a\";\nenum { A = s };\n"),
        [Code::E0227]
    );
    assert_eq!(
        codes(&format!(
            "{lax}const string s = \"a\";\nenum {{ A = s, B }};\nfloat x = B;\nfloat x = 1;\n"
        )),
        [Code::W0230]
    );
    assert_eq!(codes("float c;\nenum { A = c };\n"), [Code::E0228]);
}

// spec QCD-3.37: E0237 "pointer<->string casts were disabled." at the value
// spec FTEF-22.4: the DarkPlaces target, `undefwordsize` and `-std=gmqcc` make a string cast
// to a pointer "pointer<->string casts were disabled.", `void*` and `char*` too, and a pointer
// to a string not; the target and the options set it in order
#[test]
fn undefined_word_size() {
    let disabled = one(Code::E0237, "pointer<->string casts were disabled.");
    assert_eq!(diags("#pragma target dp\nfloat *p = \"a\";\n"), disabled);
    assert_eq!(diags("#pragma target dp\nvoid *p = \"a\";\n"), disabled);
    assert_eq!(diags("#pragma target dp\n__uint8 *p = \"a\";\n"), disabled);
    assert_eq!(
        diags("#pragma flag enable undefwordsize\nfloat *p = \"a\";\n"),
        disabled
    );
    assert_eq!(
        codes("#pragma target darkplaces\nvoid() g = { float *p; string s = p; };\n"),
        [Code::W0220]
    );
    for source in [
        "#pragma target dp\n#pragma target fte\nfloat *p = \"a\";\n",
        "#pragma target dp\n#pragma flag disable undefwordsize\nfloat *p = \"a\";\n",
        "#pragma target bogus\nfloat *p = \"a\";\n",
    ] {
        assert_eq!(
            codes(source)
                .into_iter()
                .filter(|c| *c != Code::W0117)
                .collect::<Vec<_>>(),
            [Code::W0220],
            "{source}"
        );
    }
    for (args, code) in [
        (&["-Tdp"][..], Code::E0237),
        (&["-Tdp_1"][..], Code::E0237),
        (&["-Fundefwordsize"][..], Code::E0237),
        (&["-std=gmqcc"][..], Code::E0237),
        (&["-Fundefwordsize", "-Tid"][..], Code::W0220),
        (&["-Tdp", "-Tfte"][..], Code::W0220),
        (&["-Tdp", "-Tparse"][..], Code::E0237),
    ] {
        assert_eq!(codes_with("float *p = \"a\";\n", args), [code], "{args:?}");
    }
}

// spec FTEF-19.31: a string casts to and from a pointer to an 8-bit integer, and a pointer, a
// function or a string to and from `void*`, with no warning; a pointer to another warns unless
// they point to one type
#[test]
fn void_and_byte_pointers() {
    for source in [
        "void *p = \"a\";\n",
        "__int8 *p = \"a\";\n",
        "__uint8 *p = \"a\";\n",
        "void() g = { void *p; string s = p; };\n",
        "void() g = { __uint8 *p; string s = p; };\n",
        "void() g = { float *p; void *q = p; };\n",
        "void() g = { void *p; vector *q = p; };\n",
        "void() g = { void() h; void *q = h; };\n",
        "void() g = { void *p; void() q = p; };\n",
        "void() g = { float *p; float *q = p; };\n",
    ] {
        assert_eq!(codes(source), [], "{source}");
    }
    assert_eq!(
        diags("__int16 *p = \"a\";\n"),
        one(Code::W0220, "Implicit cast from string to __int16*")
    );
    assert_eq!(
        diags("void() g = { float *p; vector *q = p; };\n"),
        one(Code::W0220, "Implicit cast from float* to vector*")
    );
    assert_eq!(
        diags("void() g = { float *p; string s = p; };\n"),
        one(Code::W0220, "Implicit cast from float* to string")
    );
}

// spec QCD-3.39: E0239 "extern is not supported with this target format" at the definition
// spec FTEF-22.5: an `extern` function is an error unless the target at the end is one of FTE's
#[test]
fn extern_needs_an_fte_target() {
    let refused = one(
        Code::E0239,
        "extern is not supported with this target format",
    );
    assert_eq!(diags("extern void(float x) h;\n"), refused);
    assert_eq!(
        diags("#pragma target fte\nextern void(float x) h;\n#pragma target id\n"),
        refused
    );
    assert_eq!(
        diags("#pragma target dp\nextern void(float x) h;\n"),
        refused
    );
    for source in [
        "#pragma target fte\nextern void(float x) h;\n",
        "extern void(float x) h;\n#pragma target fte\n",
        "#pragma target ftedebug\nextern void(float x) h;\n",
        "#pragma target fteh2\nextern void(float x) h;\n",
    ] {
        assert_eq!(codes(source), [], "{source}");
    }
    assert_eq!(codes_with("extern void(float x) h;\n", &["-Tfte"]), []);
    assert_eq!(
        codes_with("#pragma target id\nextern void(float x) h;\n", &["-Tfte"]),
        [Code::E0239]
    );
}

// spec FTEF-22.4: `QCC_SetDefaultProperties` clears `flag_undefwordsize` before the second
// autoprototyping pass, so a target a later pragma sets holds only after it there
#[test]
fn the_second_pass_starts_on_the_standard_target() {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    let source = "float *p = \"a\";\n#pragma target dp\nfloat *q = \"a\";\n";
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        autoproto: Some(true),
        ..ModuleInput::default()
    });
    let codes: Vec<Code> = analysis.diagnostics().map(|d| d.diagnostic.code).collect();
    assert_eq!(codes, [Code::W0220, Code::E0237]);
}

// spec FTEF-19.31: types are compared with their typedefs resolved, a function by its parts and
// a pointer by what it points to, all the way down
#[test]
fn casts_compare_types_through_typedefs_and_pointers() {
    for source in [
        "typedef float t;\nvoid() g = { void(t x) a; void(float x) b = a; };\n",
        "void() g = { void(int x) a; void(integer x) b = a; };\n",
        "typedef __uint8 byte;\nbyte *p = \"a\";\n",
        "typedef void v;\nv *p = \"a\";\n",
    ] {
        assert_eq!(codes(source), [], "{source}");
    }
    assert_eq!(
        diags("void() g = { void(float x) a; void(vector x) b = a; };\n"),
        one(
            Code::W0220,
            "Implicit cast from void(float x) to void(vector x)"
        )
    );
    assert_eq!(
        diags("void() g = { float **p; vector **q = p; };\n"),
        one(Code::W0220, "Implicit cast from float** to vector**")
    );
    assert_eq!(
        diags("void() g = { float **p; float *q = p; };\n"),
        one(Code::W0220, "Implicit cast from float** to float*")
    );
}

// spec FTEF-22.3: a cycle's two frames are converted before "state type mismatch", given once,
// and a fatal conversion ends the header; a named enum value converts by its typedef's type
#[test]
fn state_headers_and_typedef_enum_values() {
    let lax = "#pragma flag enable lax\n";
    assert_eq!(
        codes(&format!("{lax}void() g = [++ \"a\" .. \"b\"] {{}};\n")),
        [Code::W0230, Code::W0230, Code::E0238]
    );
    assert_eq!(codes("void() g = [++ \"a\" .. \"b\"] {};\n"), [Code::E0227]);
    assert_eq!(
        codes("#pragma flag enable typeexplicit\nvoid() g = [1i, 5] {};\n"),
        [Code::E0236]
    );
    assert_eq!(
        codes("typedef string str;\nconst str s = \"a\";\nenum { A = s };\n"),
        [Code::E0227]
    );
}

// spec FTEF-24.7: a function type prints its parameters' marks, names and defaults
// spec FTEF-24.14: a default prints as QCC_VarAtOffset prints a constant of its type
#[test]
fn function_types_print_their_parameters() {
    for (ty, shown) in [
        ("void(float x, string y)", "void(float x, string y)"),
        ("void(float x, ...)", "void(float x, ...)"),
        (
            "void(float x, optional float y = 2)",
            "void(float x, optional float y = 2f)",
        ),
        (
            "void(float x = 0.5, vector v = '1 2 3', string s = \"a\")",
            "void(float x = 0.5f, vector v = '1 2 3', string s = \"a\")",
        ),
        ("void(__inout float x)", "void(inout float x)"),
        ("void(int x = 3)", "void(__int32 x = 0x3i)"),
        ("void(__uint32 x = 5)", "void(__uint32 x = 0x5u)"),
        ("void(__uint32 x = 0)", "void(__uint32 x = 0u)"),
        (
            "void(__int64 x = -1)",
            "void(__int64 x = 0xffffffffffffffffill)",
        ),
        ("void(__uint64 x = 7)", "void(__uint64 x = 0x7ull)"),
        ("void(__double x = 2.5)", "void(__double x = 2.5d)"),
        ("void(vector x = [1, 2, 3])", "void(vector x = '1 2 3')"),
        ("void(int x = 0x10)", "void(__int32 x = 0x10i)"),
    ] {
        assert_eq!(
            diags(&format!(
                "{ty} h = {{}}; float b; void() g = {{ b = h; }};\n"
            )),
            [(Code::E0213, format!("Cannot cast from {shown} to float"))],
            "{ty}"
        );
    }
}

// spec FTEF-19.50: a field of a struct, union or class type casts only to a field of its own type
// or of a variant
// spec FTEF-24.12: a named union prints as a struct, one with no tag `<union>`, and a field of a
// struct its own name
#[test]
fn fields_of_aggregates_and_their_names() {
    let messages = |statement: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\nstruct S {{ float x; }}; union U {{ float a; }}; \
             typedef union {{ float a; }} V; class Player {{ float hp; }}; \
             class Other : Player {{ float y; }};\n.S fs, fs2; .U fu; .Player fc; .Other fo; \
             .__variant fv; .float fa; S s; U u; V v;\nvoid() g = {{ {statement} }};\n"
        ))
        .into_iter()
        .filter(|(code, _)| *code == Code::E0213)
        .map(|(_, message)| message)
        .collect()
    };
    for quiet in ["fs = fs2;", "fs = fv;", "fv = fs;"] {
        assert_eq!(messages(quiet), Vec::<String>::new(), "{quiet}");
    }
    for (statement, message) in [
        ("fs = fu;", "Cannot cast from .U to .S"),
        ("fs = fa;", "Cannot cast from .float to .S"),
        ("fa = fs;", "Cannot cast from .S to .float"),
        ("fc = fo;", "Cannot cast from class Other to class Player"),
        ("fc = fa;", "Cannot cast from .float to class Player"),
        ("s = u;", "Cannot cast from struct U to struct S"),
        ("s = v;", "Cannot cast from union <union> to struct S"),
    ] {
        assert_eq!(messages(statement), [message], "{statement}");
    }
}

// spec FTEF-29.11: a global initialised with a constant whose word is 0 takes it as a null
#[test]
fn a_named_null_initialises_any_global() {
    let messages = |source: &str| -> Vec<String> {
        diags(&format!("#pragma target fte\n{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    for source in [
        "enum { H }; string s = H;",
        "const float z = 0; string s2 = z;",
        "const int zi = 0; string s3 = zi;",
    ] {
        assert_eq!(messages(source), Vec::<String>::new(), "{source}");
    }
    assert_eq!(
        messages("enum { H, K }; string s = K;"),
        ["Cannot cast from float to string"]
    );
    assert_eq!(
        messages("struct S { float a; }; const float z = 0; S st = z;"),
        ["Cannot cast from float to struct S"]
    );
    assert_eq!(
        messages("enum class E5 : float { M }; string s = E5::M;"),
        ["Cannot cast from enum E5:float to string"]
    );
}

// spec FTEF-15.10: a parameter's written struct, union or enum is its type
#[test]
fn a_parameter_writes_its_type() {
    let messages = |source: &str| -> Vec<String> {
        diags(&format!("#pragma target fte\n{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    assert_eq!(
        messages("void(union { float a; vector v; } u) f = {}; float x; void() g = { x = f; };"),
        ["Cannot cast from void(union <union> u) to float"]
    );
    assert_eq!(
        messages("void(enum { EA } e) f = { local string s = e; };"),
        ["Cannot cast from float to string"]
    );
    assert_eq!(
        messages("void(struct S1 { float a; } s) f = { s.a = \"x\"; };"),
        ["Cannot cast from string to float"]
    );
}

// spec FTEF-23.7: a comparison's value is a `__bfloat`, `_Bool` an `int`
// spec FTEF-19.55: a float or an integer cast to a vector is multiplied by `'1 1 1'`
// spec FTEF-26.11: a logical not of a constant folds before any emulation
#[test]
fn truth_values_and_vector_casts() {
    let messages = |pragma: &str, statement: &str| -> Vec<String> {
        diags(&format!(
            "{pragma}\n_Bool b; string s; vector v; float f; int i; float arr[1]; float x; \
             entity world;\nvoid() g = {{ {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    let fte = "#pragma target fte";
    assert_eq!(
        messages(fte, "s = (f == 1);"),
        ["Cannot cast from __bfloat to string"]
    );
    assert_eq!(
        messages(fte, "s = b;"),
        ["Implicit cast from __int32 to string"]
    );
    assert_eq!(
        messages("", "v = i;"),
        [
            "Implicit cast from __int32 to vector",
            "itof emulation: denormals are unsafe i"
        ]
    );
    assert_eq!(
        messages("", "local vector lv = i;"),
        [
            "Implicit cast from __int32 to vector",
            "itof emulation: denormals are unsafe i"
        ]
    );
    assert_eq!(messages("", "if (!1i) f = 1;"), Vec::<String>::new());
    for (statement, index) in [
        ("x = arr[(!0i) + 3i];", 4),
        ("x = arr[(!'0 1 0') + 3];", 3),
        ("x = arr[(!0.0) + 3];", 4),
    ] {
        let found = messages(fte, statement);
        let bounds = format!("(constant) array index out of bounds (0 <= {index} < 1)");
        assert!(found.contains(&bounds), "{statement}: {found:?}");
    }
}

// spec FTEF-11.4: a `__variant` target differs from a pointer or function target
#[test]
fn variant_pointers_against_pointer_targets() {
    let messages = |statement: &str| -> Vec<String> {
        diags(&format!(
            "float **p; __variant *q; __variant **t; void() *r; string *s;\n\
             void() g = {{ {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    assert_eq!(
        messages("q = p;"),
        ["Implicit cast from float** to __variant*"]
    );
    assert_eq!(
        messages("q = r;"),
        ["Implicit cast from void()* to __variant*"]
    );
    assert_eq!(
        messages("p = q;"),
        ["Implicit cast from __variant* to float**"]
    );
    assert!(messages("t = p;").is_empty());
    assert!(messages("q = s;").is_empty());
}

// spec FTEF-8.6: a named enum's type casts as its base and prints by its name
#[test]
fn named_enum_types_cast_as_their_base() {
    let messages = |source: &str| -> Vec<String> {
        diags(&format!("#pragma target fte\n{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    assert_eq!(
        messages("enum EN { A }; EN e; string s = e;"),
        ["Cannot cast from EN to string"]
    );
    assert_eq!(
        messages("enum EN : int { A }; EN e; void() f = { local string s = e; };"),
        ["Implicit cast from EN to string"]
    );
    assert_eq!(
        messages("enum EN { A }; EN e; void() f = { local entity s = e; };"),
        ["Cannot cast from EN to entity"]
    );
    let standard: Vec<String> = diags("enum EN { A }; EN e; void() f = { local int i = e; };\n")
        .into_iter()
        .map(|(_, message)| message)
        .collect();
    assert_eq!(standard, ["ftoi emulation: denormals are unsafe"]);
    assert_eq!(
        messages("enum EN : int { A }; enum EN { B }; EN e; void() f = { local string s = e; };"),
        [
            "legacy enum base type. Use \"enum [class] [name_e]:type\" instead",
            "Implicit cast from EN to string"
        ]
    );
    assert!(messages("enum EN { A }; string s = A; EN e = 1i;").is_empty());
}

// spec FTEF-23.8: booleans are one type, cast to by their logical truth
#[test]
fn casts_to_booleans_take_the_logical_truth() {
    let messages = |source: &str| -> Vec<String> {
        diags(&format!("{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    let ftoi = "ftoi emulation: denormals are unsafe";
    assert!(messages("__bint b; __bfloat b;").is_empty());
    assert_eq!(
        messages("_Bool b; int b;"),
        ["Type mismatch on redeclaration of b. __int32, should be __bint"]
    );
    assert_eq!(
        messages("int b; _Bool b;"),
        ["Type mismatch on redeclaration of b. __bint, should be __int32"]
    );
    assert!(messages("void() g = { local __bint c = 1.5; };").is_empty());
    assert_eq!(
        messages("float f; void() g = { local _Bool c = f; };"),
        [ftoi]
    );
    assert_eq!(
        messages("string s; void() g = { local __bint c; c = s; };"),
        [ftoi, ftoi]
    );
    assert_eq!(
        messages("float f; void() g = { local _Bool c; c = (_Bool)f; };"),
        [ftoi, ftoi, ftoi]
    );
    assert!(messages("float f; void() g = { local __bfloat c; c = f; };").is_empty());
}

// spec FTEF-22.8: pointers cast by their pointees and functions by `typecmp_lax`
#[test]
fn pointers_and_functions_cast_by_typecmp_lax() {
    let messages = |statement: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\nstruct S {{ float a; }};\nvoid() g = {{ local float *pf; local S *ps; \
             local float **ppf; local void(float) f2; local void(float, ...) f4; local void(int) f3; \
             {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    assert_eq!(
        messages("pf = ps;"),
        ["Implicit cast from struct S* to float*"]
    );
    assert_eq!(
        messages("ps = ppf;"),
        ["Implicit cast from float** to struct S*"]
    );
    assert!(messages("f2 = f4; f4 = f2;").is_empty());
    assert_eq!(
        messages("f2 = f3;"),
        ["Implicit cast from void(__int32) to void(float)"]
    );
}

// spec FTEF-23.10: a value made a boolean is always its truth, a boolean's too
// spec FTEF-26.12: a function's definition is emitted and emulated as a variable is
#[test]
fn a_boolean_is_always_a_truth() {
    let found = |statement: &str| -> usize {
        diags(&format!(
            "_Bool b; int i; void() fn = {{}};\nvoid() g = {{ {statement} }};\n"
        ))
        .iter()
        .filter(|(_, m)| m.as_str() == "ftoi emulation: denormals are unsafe")
        .count()
    };
    assert_eq!(found("b = b;"), 2);
    assert_eq!(found("b = i;"), 2);
    assert_eq!(found("b = (_Bool)i;"), 3);
    assert_eq!(found("b = (_Bool)fn;"), 3);
}
