// SPDX-License-Identifier: GPL-3.0-or-later

//! The flags that change in the middle of a file as the semantic model reads them
//! (`docs/spec/dialect-fteqcc.md` FTEF-21): the definitions an `extern "C"` block makes, and
//! `assumevar`, `subscope`, `qcfuncs`, `assumeint`, the attributes and the defaults of `#pragma
//! noref`, `nosave` and `defaultstatic`. Each program is also a case of the flag family of
//! `cargo xtask oracles builtins`, which compares the outcome with the pinned FTEQCC.

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

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

/// The one file these tests read.
const FILE: FileId = FileId(0);

/// The codes of the errors of the FTEQCC files `sources`, one module in that order.
fn module_errors(sources: &[&str]) -> Vec<(FileId, Code)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    let mut compiled = Vec::new();
    for (at, source) in sources.iter().enumerate() {
        let id = FileId(at);
        db.set_file(
            id,
            PathBuf::from(format!("f{at}.qc")),
            source.as_bytes().to_vec(),
        );
        compiled.push(id);
    }
    let analysis = db.analyze_module(&ModuleInput {
        compiled,
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| (d.file, d.diagnostic.code))
        .collect()
}

/// The codes of [`analysed`] with no option.
fn codes(source: &str) -> Vec<Code> {
    analysed(source, &[]).into_iter().map(|(c, _)| c).collect()
}

/// The codes of the errors of [`analysed`] with no option.
fn errors(source: &str) -> Vec<Code> {
    codes(source).into_iter().filter(|c| c.is_error()).collect()
}

/// The codes of [`analysed`] with the options `args`.
fn codes_with(source: &str, args: &[&str]) -> Vec<Code> {
    analysed(source, args).into_iter().map(|(c, _)| c).collect()
}

// spec FTEF-21.17: `"C"` turns `assumevar` on for its block, in any case, its string's parts
// joined, and for one definition after any `;`; any other string sets nothing
// spec FTEF-21.11: under `assumevar` an initialised global is no constant, so naming it in
// another's initialiser is "is not constant", and `const` still makes one
#[test]
fn extern_c_and_assumevar() {
    let not_constant = [Code::E0214];
    assert_eq!(
        errors("extern \"C\" { float x = 5; float y = x; }\n"),
        not_constant
    );
    assert_eq!(
        errors("extern \"C\" float x = 5;\nfloat y = x;\n"),
        not_constant
    );
    assert_eq!(
        errors("extern \"c\" { float x = 5; float y = x; }\n"),
        not_constant
    );
    assert_eq!(
        errors("extern \"\" \"C\" { float x = 5; float y = x; }\n"),
        not_constant
    );
    assert_eq!(
        errors("extern \"C\" ;\nfloat x = 5;\nfloat y = x;\n"),
        not_constant
    );
    assert_eq!(errors("extern \"C++\" { float x = 5; float y = x; }\n"), []);
    assert_eq!(errors("extern \"QC\" { float x = 5; float y = x; }\n"), []);
    assert_eq!(
        errors("extern \"C\" { float x = 5; }\nfloat y = x;\n"),
        not_constant
    );
    assert_eq!(errors("float x = 5;\nfloat y = x;\n"), []);
    assert_eq!(
        errors("#pragma flag enable assumevar\nfloat x = 5;\nfloat y = x;\n"),
        not_constant
    );
    assert_eq!(
        errors("#pragma flag enable assumevar\nconst float x = 5;\nfloat y = x;\n"),
        []
    );
    assert_eq!(
        codes_with("float x = 5;\nfloat y = x;\n", &["-Fassumevar"]),
        not_constant
    );
    // FTEF-19.3: the first declaration fixes the constness.
    assert_eq!(
        codes("extern \"C\" { float x = 5; }\nfloat x = 6;\n"),
        [Code::W0212, Code::E0363]
    );
}

// spec FTEF-21.3: nested blocks restore what each saved, so `"QC"` in `"C"` leaves `assumevar`
// on and its end turns it on again
#[test]
fn nested_blocks() {
    assert_eq!(
        errors("extern \"C\" { extern \"QC\" { float x = 5; float y = x; } }\n"),
        [Code::E0214]
    );
    assert_eq!(
        errors("extern \"C\" { extern \"QC\" { float x = 5; } float y = x; }\n"),
        [Code::E0214]
    );
}

// spec FTEF-21.12: under `subscope`, from `"C"`, a pragma or the command line, a block's locals
// end with it, read at its `}`, and a local of a name made again is a new one
#[test]
fn subscoped_locals() {
    let unknown = [Code::E0301];
    assert_eq!(
        errors("extern \"C\" { void g() = { { float x; } x = 1; }; }\n"),
        unknown
    );
    assert_eq!(errors("void() g = { { float x; } x = 1; };\n"), []);
    assert_eq!(
        errors("#pragma flag enable subscope\nvoid() g = { { float x; } x = 1; };\n"),
        unknown
    );
    assert_eq!(
        codes_with("void() g = { { float x; } x = 1; };\n", &["-Fsubscope"]),
        unknown
    );
    assert_eq!(
        errors("void() g = { { float x;\n#pragma flag enable subscope\n} x = 1; };\n"),
        unknown
    );
    assert_eq!(
        errors(
            "#pragma flag enable subscope\nvoid() g = { { float x;\n#pragma flag disable \
             subscope\n} x = 1; };\n"
        ),
        []
    );
    assert_eq!(
        errors("void() g = { float x; string x; };\n"),
        [Code::E0205]
    );
    assert_eq!(
        codes("#pragma flag enable subscope\nvoid() g = { float x; string x; x = \"a\"; };\n"),
        []
    );
    assert_eq!(
        codes("#pragma flag enable subscope\nvoid() g = { float x; { float x; } x = 1; };\n"),
        []
    );
    assert_eq!(
        codes("void() g = { float x; { float x; } x = 1; };\n"),
        [Code::W0207]
    );
    // FTEF-19.19: a `for` statement's locals end with it either way.
    assert_eq!(
        errors("#pragma flag enable subscope\nvoid() g = { for (float i = 0;;) {} i = 1; };\n"),
        unknown
    );
}

// spec FTEF-21.7: with `qcfuncs` off a declaration's `*` is each declarator's, a parameter list
// before a name is the declarator's, and `(*name)(params)` is a variable of function type
#[test]
fn c_declarators() {
    assert_eq!(errors("extern \"C\" { float *a, b = 1.5; }\n"), []);
    assert_eq!(errors("float *a, b = 1.5;\n"), [Code::E0213]);
    assert_eq!(
        errors("#pragma flag disable qcfuncs\nfloat *a, b = 1.5;\n"),
        []
    );
    assert_eq!(codes_with("float *a, b = 1.5;\n", &["-Fno-qcfuncs"]), []);
    assert_eq!(errors("extern \"C\" { .void() think; }\n"), [Code::E0211]);
    assert_eq!(errors(".void() think;\n"), []);
    assert_eq!(
        errors("extern \"C\" { void(float a) g; }\nvoid(float a) g = #2;\n"),
        []
    );
    assert_eq!(
        codes("extern \"C\" { float (*fp)(float); }\nfloat(float) fp;\n"),
        [Code::W0212]
    );
    assert_eq!(errors("extern \"C\" { float (*fp)(float) = 0; }\n"), []);
    assert_eq!(errors("float a, (*fp)(float);\n"), []);
    assert_eq!(
        errors("extern \"C\" { typedef float *fp; fp a; }\nfloat *a;\n"),
        []
    );
    assert_eq!(
        errors("float a, (float x) g;\nfloat(float x) g = #2;\n"),
        []
    );
}

// spec FTEF-21.8: with `qcfuncs` off `()` is variadic, a first parameter with no type an int
// and an array parameter a pointer, as `[]` is either way
#[test]
fn c_parameters() {
    assert_eq!(
        errors("extern \"C\" { void h(); }\nvoid h(float x) = #1;\n"),
        [Code::E0205, Code::E0211]
    );
    assert_eq!(
        errors("extern \"C\" { void h(); }\nvoid(...) h = #1;\n"),
        []
    );
    // FTEF-19.7: a function declared again as `T(...)` is no difference.
    assert_eq!(
        errors("void h(float x) = #1;\nextern \"C\" { void h(); }\n"),
        []
    );
    assert_eq!(errors("void h(float x) = #1;\nvoid h();\n"), [Code::E0205]);
    assert_eq!(
        errors("extern \"C\" { void h(x) = #1; }\nvoid h(int a) = #1;\n"),
        []
    );
    assert_eq!(
        errors("extern \"C\" { void h(x, y) = #1; }\n"),
        [Code::E0203]
    );
    assert_eq!(
        errors("extern \"C\" { void h(float a[2]); }\nvoid h(float *a) = #1;\n"),
        []
    );
    // FTEF-17.21: with `qcfuncs` on the `float` of `a[2]` is no pointer, as the pinned FTEQCC
    // reads it, "Type mismatch on redeclaration of h. void(float* a), should be void(float a)".
    assert_eq!(
        errors("void h(float a[2]);\nvoid h(float *a) = #1;\n"),
        [Code::E0205, Code::E0211]
    );
    assert_eq!(errors("void h(float a[]);\nvoid h(float *a) = #1;\n"), []);
}

// spec FTEF-21.10: an `extern` on a type that is no function's is dropped, with its warning when
// `qcfuncs` is on, and with it off a function declared in a body is a global
#[test]
fn extern_and_c_local_functions() {
    assert_eq!(codes("extern float x;\n"), [Code::W0214]);
    assert_eq!(codes("extern \"C\" { extern float x; }\n"), []);
    assert_eq!(errors("extern \"C\" { extern void h(); }\n"), [Code::E0211]);
    assert_eq!(errors("extern void h(float x);\n"), [Code::E0211]);
    assert_eq!(
        codes("extern void h(float x);\nvoid h(float x) = {};\n"),
        [Code::W0214]
    );
    assert_eq!(errors("void() g = { void h(); };\n"), []);
    assert_eq!(
        errors("extern \"C\" { void g() = { void h(); }; }\n"),
        [Code::E0211]
    );
    assert_eq!(
        errors("extern \"C\" { void g() = { float h(); }; float h() = { return 1; }; }\n"),
        []
    );
}

// spec FTEF-21.9: attributes, and the definitions `alias` makes
// spec QCD-3.34: E0234 "TARGET not yet defined, cannot create NAME as an alias"
// spec QCD-3.35: E0235 "alias NAME may not be initialised"
#[test]
fn attributes() {
    assert_eq!(errors("float y;\nvar [[alias(y)]] float x;\n"), []);
    assert_eq!(errors("float y;\nvar [[alias(\"y\")]] float x;\n"), []);
    assert_eq!(
        analysed("var [[alias(y)]] float x;\n", &[]),
        [(
            Code::E0234,
            "y not yet defined, cannot create x as an alias".to_owned()
        )]
    );
    assert_eq!(
        analysed("float y;\nvar [[alias(y)]] float x = 1;\n", &[]),
        [(Code::E0235, "alias x may not be initialised".to_owned())]
    );
    assert_eq!(errors("float y;\nvar [[alias(y)]] string x;\n"), []);
    assert_eq!(
        errors("const [[accumulate]] void() h = { };\nconst [[accumulate]] void() h = { };\n"),
        []
    );
    assert!(errors("void() h = { };\nvoid() h = { };\n").contains(&Code::E0216));
    assert_eq!(
        codes("const [[__deprecated(\"old\")]] float x = 1;\nfloat() h = { return x; };\n"),
        [Code::W0314]
    );
    assert_eq!(
        codes("const [[eraseable]] float x;\n"),
        codes("noref float x;\n")
    );
}

// spec FTEF-21.13: under `assumeint` an enum's names are integers
#[test]
fn integer_enums() {
    assert_eq!(
        errors("#pragma flag enable assumeint\nenum { A, B };\nstring s = B;\n"),
        []
    );
    assert_eq!(errors("enum { A, B };\nstring s = B;\n"), [Code::E0213]);
    assert_eq!(
        errors("extern \"C\" { enum { A, B }; }\nstring s = B;\n"),
        []
    );
}

// spec FTEF-21.14: a typedef's `static`, `noref` and `nosave` are no error under their pragma's
// default, and under `defaultstatic` a typedef has two storage classes; `typedef` among the
// qualifiers, and a type left out after it
#[test]
fn typedefs_and_pragma_defaults() {
    assert_eq!(
        codes("#pragma defaultstatic 1\ntypedef float t;\n"),
        [Code::W0214]
    );
    assert_eq!(
        codes("static typedef float t;\n"),
        [Code::W0214, Code::E0224]
    );
    assert_eq!(
        codes("#pragma defaultstatic 1\ntypedef static float t;\n"),
        [Code::W0214]
    );
    assert_eq!(codes("#pragma noref 1\nnoref typedef float t;\n"), []);
    assert_eq!(codes("noref typedef float t;\n"), [Code::E0224]);
    assert_eq!(codes("#pragma nosave 1\nnosave typedef float t;\n"), []);
    assert_eq!(codes("nosave typedef float t;\n"), [Code::E0224]);
    assert_eq!(codes("typedef static x;\n"), [Code::W0214, Code::E0224]);
    assert_eq!(errors("typedef x;\nx y;\n"), []);
    // FTEF-19.77: the type left out is an `int`.
    assert_eq!(
        analysed("typedef qq; qq a; string s = a;\n", &[])
            .into_iter()
            .map(|(_, message)| message)
            .collect::<Vec<_>>(),
        [
            "Implicit cast from __int32 to string",
            "initializer for 's' is not constant"
        ]
    );
}

// spec FTEF-21.6: the warning is F304, which the warning settings turn off or make an error
// spec FTEF-21.9: an unknown attribute is F321
#[test]
fn the_parsers_flag_warnings_follow_the_settings() {
    let unary = "float a, b;\nfloat() h = { return !a == b; };\n";
    assert_eq!(codes(unary), [Code::W0227]);
    assert_eq!(codes_with(unary, &["-Wno-F304"]), []);
    assert_eq!(codes_with(unary, &["-Werror"]), [Code::E0125]);
    let attribute = "#pragma flag enable attributes\n[[foo]] float x;\n";
    assert_eq!(codes(attribute), [Code::W0228]);
    assert_eq!(codes_with(attribute, &["-Wno-F321"]), []);
}

// spec FTEF-21.17: the tokens a block lexes otherwise have the parser's lexical diagnostics, not
// the preprocessor's
#[test]
fn a_blocks_lexical_diagnostics() {
    assert_eq!(
        errors("extern \"C\" { string s = \"\\9\"; }\n"),
        [Code::E0103]
    );
    assert_eq!(errors("extern \"QC\" { string s = \"\\9\"; }\n"), []);
    assert_eq!(errors("string s = \"\\9\";\n"), []);
    let off = "#pragma flag disable qcfuncs\n";
    assert_eq!(
        errors(&format!("{off}extern \"QC\" {{ string s = \"\\9\"; }}\n")),
        []
    );
    assert_eq!(
        errors(&format!("{off}string s = \"\\9\";\n")),
        [Code::E0103]
    );
    let block = format!("{off}extern \"QC\" {{ string s = \"\\9\"; }}\n");
    let after = format!("{block}string t = \"\\9\";\n");
    assert_eq!(module_errors(&[&after]), [(FileId(0), Code::E0103)]);
    let at = block.find("\"\\9").unwrap();
    let other = format!("{}string u = \"\\9\";\n", " ".repeat(at - 11));
    assert_eq!(module_errors(&[&block, &other]), [(FileId(1), Code::E0103)]);
}

// spec FTEF-21.17: `QCC_PR_LexString` joins the parts of a string written in one text, so a
// macro's string and the string beside it are two tokens: the first is the block's string, the
// second starts its definition
#[test]
fn a_macros_string_is_not_joined_to_its_neighbour() {
    let probe = "{ string s = \"\\9\"; }";
    let block = |define: &str, string: &str| errors(&format!("{define}extern {string} {probe}\n"));
    assert_eq!(block("#define C \"C\"\n", "C"), [Code::E0103]);
    assert_eq!(block("", "\"C\" \"C\""), []);
    assert_eq!(block("#define C \"C\" \"x\"\n", "C"), []);
    assert_eq!(
        block("#define C \"C\"\n", "C C"),
        [Code::E0203, Code::E0103]
    );
    assert_eq!(
        block("#define C \"C\"\n", "\"C\" C"),
        [Code::E0203, Code::E0103]
    );
    assert_eq!(
        block("#define C \"C\"\n", "C \"x\""),
        [Code::E0203, Code::E0103]
    );
    assert_eq!(block("#define C(a) a \"x\"\n", "C(\"C\")"), []);
    assert_eq!(
        block("#define D \"C\"\n#define C D \"x\"\n", "C"),
        [Code::E0203, Code::E0103]
    );
}

// spec FTEF-21.10: `extern` is tested against the base type with its typedefs read
#[test]
fn extern_on_a_typedef() {
    assert_eq!(
        codes("typedef float myfloat;\nextern myfloat x;\n"),
        [Code::W0214]
    );
    assert_eq!(
        codes("typedef void() fn;\nextern fn h;\n"),
        codes("extern void() h;\n")
    );
    // `!pr_scope`: in a body the `extern` stays, and a typedef takes no qualifier.
    for body in [
        "typedef extern float y;",
        "extern typedef float y;",
        "typedef extern void() y;",
    ] {
        assert_eq!(
            errors(&format!("void() g = {{ {body} }};\n")),
            [Code::E0224],
            "{body}"
        );
    }
    // A typedef in an `extern` block in a body is that body's, and is checked as any.
    assert_eq!(
        errors("void() g = { extern \"C\" { typedef float F; } F x; };\n"),
        []
    );
    assert_eq!(
        errors("void() g = { extern \"C\" { typedef static float F; } };\n"),
        [Code::E0224]
    );
    assert_eq!(errors("typedef extern float y;\n"), []);
    assert_eq!(errors("void() g = { extern float y; };\n"), []);
}

// spec FTEF-21.1: the definition after a block reads the flags it restored
#[test]
fn the_definition_after_a_block() {
    assert_eq!(
        errors("extern \"C\" int a; enum { A, B };\nstring s = B;\n"),
        [Code::E0213]
    );
}

// spec FTEF-21.21: with `qcfuncs` off a body's C function may be defined there, a C function
// pointer is a variable, a `const` one a constant, and a function's parameters are the list
// before its name
#[test]
fn c_declarators_with_qcfuncs_off() {
    let off = |source: &str| codes(&format!("#pragma flag disable qcfuncs\n{source}\n"));
    for source in [
        "void() g = { void x() {} };",
        "void() g = { void x() { } x(); };",
        "void() g = { float (*x)(float); x(1); };",
        "void() g = { entity (*x)(); };",
        "float (*x)(float)(float);",
        "void(float x) h = { x = 1; };",
        "struct S { float a; }; S x; void(S s) h = { s.a = 1; }; void() g = { h(x); };",
        "typedef void() fn; void(fn callback) h = { callback(); };",
    ] {
        assert_eq!(off(source), [], "{source}");
    }
    assert_eq!(off("void() g = { void h(); };"), [Code::E0211]);
    // FTEF-21.8: only an empty list is variadic with `qcfuncs` off.
    assert_eq!(
        off("void(float a) h = {}; void() g = { h(1, 2); };"),
        [Code::W0252]
    );
    assert_eq!(off("void() h0 = {}; void() g = { h0(1, 2); };"), []);
    // FTEF-21.8: a parameter with no type is an `int` with `qcfuncs` off.
    assert_eq!(off("void(a) hi = { local string s = a; };"), [Code::W0220]);
    assert_eq!(
        off("void(a) hi = {}; void() g = { hi(\"x\"); };"),
        [Code::W0220]
    );
    assert_eq!(off("const float (*x)(float);"), [Code::W0214, Code::E0211]);
}

// spec FTEF-19.51: after an attribute list alone the type may be left out, and is `int`
#[test]
fn an_omitted_type_after_attributes_is_int() {
    let cast = (
        Code::W0220,
        "Implicit cast from __int32 to string".to_owned(),
    );
    let on = "#pragma flag enable attributes\n";
    assert_eq!(
        analysed(&format!("{on}[[eraseable]] x = 5; string s = x;\n"), &[]),
        std::slice::from_ref(&cast)
    );
    assert_eq!(
        analysed(
            &format!("{on}void() g = {{ static [[eraseable]] y = 2; local string t = y; }};\n"),
            &[]
        ),
        [cast]
    );
}

// spec FTEF-21.9: `[[eraseable]]` is `noref`, and `[[__deprecated("WHY")]]` deprecates with its
// reason
#[test]
fn attribute_qualifiers() {
    let on = "#pragma flag enable attributes\n";
    assert_eq!(
        analysed(
            &format!("{on}[[eraseable]] float x = 1; float x = 2;\n"),
            &[]
        ),
        []
    );
    assert_eq!(
        errors(&format!("{on}float y = 1; float y = 2;\n")),
        [Code::E0363]
    );
    let deprecated = |attribute: &str| -> Vec<String> {
        analysed(
            &format!("{on}[[{attribute}]] float x; float q; void() g = {{ q = x; }};\n"),
            &[],
        )
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    assert_eq!(
        deprecated("__deprecated(\"use z\")"),
        ["Variable \"x\" is deprecated: use z"]
    );
    assert_eq!(deprecated("__deprecated"), ["Variable \"x\" is deprecated"]);
}

// spec FTEF-21.10: an `extern` typedef of a type that is surely no function drops the `extern`
// with its warning; one of a function's typedef keeps it, two storage classes
#[test]
fn an_extern_typedef() {
    let messages = |source: &str| -> Vec<String> {
        analysed(&format!("#pragma target fte\n{source}\n"), &[])
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    assert_eq!(
        messages("extern typedef float fl;"),
        ["extern keyword may only apply to qc functions."]
    );
    let conflicting = |name: &str| {
        vec![
            "Conflicting storage classes.".to_owned(),
            format!("bad combination of modifiers with typedef (defining {name})"),
        ]
    };
    assert_eq!(
        messages("typedef void() fn; extern typedef fn fn2;"),
        conflicting("fn2")
    );
    assert_eq!(messages("extern typedef void() fn3;"), conflicting("fn3"));
}
