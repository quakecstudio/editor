// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's two autoprototyping passes as the semantic model reads them
//! (`docs/spec/dialect-fteqcc.md` FTEF-12.18 to FTEF-12.20), and the names it finds in any case
//! under `-Fcaseinsens` (FTEF-19.31, FTEF-19.32). Each program is also a case of the two-pass
//! and case-insensitive families of `cargo xtask oracles builtins`, which compare the outcome
//! with the pinned FTEQCC.

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

/// The diagnostics of the FTEQCC module of `files`, compiled in order and read with the command
/// line `args`.
fn module(files: &[&str], args: &[&str]) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    let mut compiled = Vec::new();
    for (index, text) in files.iter().enumerate() {
        let id = FileId(index);
        db.set_file(
            id,
            PathBuf::from(format!("f{index}.qc")),
            text.as_bytes().to_vec(),
        );
        compiled.push(id);
    }
    let analysis = db.analyze_module(&ModuleInput {
        compiled,
        command_line: CommandLine::new(args),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

/// The codes of every diagnostic of the one file `source` read with the options `args`.
fn codes_with(source: &str, args: &[&str]) -> Vec<Code> {
    module(&[source], args)
        .into_iter()
        .map(|(code, _)| code)
        .collect()
}

/// The codes of the errors of the one file `source` read with the options `args`.
fn errors_with(source: &str, args: &[&str]) -> Vec<Code> {
    codes_with(source, args)
        .into_iter()
        .filter(|code| code.is_error())
        .collect()
}

/// The codes of the errors of the one file `source`.
fn errors(source: &str) -> Vec<Code> {
    errors_with(source, &[])
}

/// The codes of the errors of the files `files`.
fn module_errors(files: &[&str]) -> Vec<Code> {
    module(files, &[])
        .into_iter()
        .map(|(code, _)| code)
        .filter(|code| code.is_error())
        .collect()
}

/// A function that uses a name defined after it.
const USES_LATER: &str = "void() k = { h(); };\nvoid() h = {};\n";

/// The options under which a warning is an error.
const WERROR: [&str; 2] = ["-Werror", "-Wno-Q208"];

// spec FTEF-12.18: `numpr_globals` is `RESERVED_OFS` for the whole parse, so a `#pragma
// autoproto` after any definition is taken, with no warning
#[test]
fn the_pragma_is_taken_after_definitions() {
    for before in [
        "void() f = #5;\n",
        "float x;\n",
        "typedef float t;\n",
        "float x = 5;\nvoid() g = #6;\nenum { A, B };\n",
    ] {
        let source = format!("{before}#pragma autoproto\n{USES_LATER}");
        assert_eq!(codes_with(&source, &[]), [], "{source:?}");
    }
    assert_eq!(errors(USES_LATER), [Code::E0301]);
}

// spec FTEF-12.18: the last `#pragma autoproto` of the first pass decides whether a second
// pass follows, a number other than 0, or none, turning it on
#[test]
fn the_last_pragma_decides() {
    for pragmas in [
        "#pragma autoproto\n",
        "#pragma autoproto 1\n",
        "#pragma autoproto 7\n",
        "#pragma autoproto 0\n#pragma autoproto\n",
    ] {
        let source = format!("{pragmas}{USES_LATER}");
        assert_eq!(errors(&source), [], "{source:?}");
    }
    for pragmas in [
        "#pragma autoproto 0\n",
        "#pragma autoproto\n#pragma autoproto 0\n",
    ] {
        let source = format!("{pragmas}{USES_LATER}");
        assert_eq!(errors(&source), [Code::E0301], "{source:?}");
    }
}

// spec FTEF-12.18: `-Fautoproto` starts the flag on, and a pragma of the first pass changes it
// from where it stands
#[test]
fn the_command_line_starts_the_flag() {
    assert_eq!(errors_with(USES_LATER, &["-Fautoproto"]), []);
    // `-Fautoproto` and a last `#pragma autoproto 0`: one pass, whose functions were skipped.
    let off = format!("{USES_LATER}#pragma autoproto 0\n");
    assert_eq!(
        errors_with(&off, &["-Fautoproto"]),
        [Code::E0211, Code::E0211]
    );
    let off_first = format!("#pragma autoproto 0\n{USES_LATER}");
    assert_eq!(errors_with(&off_first, &["-Fautoproto"]), [Code::E0301]);
    // And on again after a `0`.
    let on_again = format!("#pragma autoproto 0\n#pragma autoproto 1\n{USES_LATER}");
    assert_eq!(errors_with(&on_again, &[]), []);
}

// spec FTEF-12.18: the pragma stands in the first pass wherever a file of the module puts it
#[test]
fn the_pragma_holds_across_files() {
    let later = "void() a = { later(); };\nvoid() later = {};\n";
    assert_eq!(module_errors(&["float x;\n", later]), [Code::E0301]);
    assert_eq!(module_errors(&["float x;\n#pragma autoproto\n", later]), []);
    assert_eq!(
        module_errors(&["float x;\n", "#pragma autoproto\n", later]),
        []
    );
    assert_eq!(
        module_errors(&["#pragma autoproto\n", "#pragma autoproto 0\n", later]),
        [Code::E0301]
    );
    // The functions of the first file are read before the pragma of the second.
    assert_eq!(
        module_errors(&[
            "void() a = { later(); };\n",
            "#pragma autoproto\n",
            "void() later = {};\n"
        ]),
        [Code::E0301, Code::E0216, Code::E0206]
    );
}

// spec FTEF-12.18, QCD-7.70: a pragma after a definition is no warning, `W0108` being
// unreachable
#[test]
fn no_pragma_is_a_warning() {
    let source = format!("float x;\n#pragma autoproto\n{USES_LATER}#pragma autoproto\n");
    assert!(!codes_with(&source, &[]).contains(&Code::W0108));
    assert!(!codes_with(&source, &["-Fautoproto"]).contains(&Code::W0108));
}

// spec FTEF-12.19: what stands before the pragma is compiled in full by the first pass, so a
// name it uses must be defined before it, and its function bodies are given again by the second
#[test]
fn what_precedes_the_pragma_is_compiled_in_full() {
    // `h` is used before it is defined, with `autoprototype` off.
    // FTEQCC gives "redeclaration of function body" and "incompatible redeclaration" for a body
    // given again.
    assert_eq!(
        errors(&format!("{USES_LATER}#pragma autoproto\n")),
        [
            Code::E0301,
            Code::E0216,
            Code::E0206,
            Code::E0216,
            Code::E0206
        ]
    );
    assert_eq!(
        errors("void() g = {};\n#pragma autoproto\n"),
        [Code::E0216, Code::E0206]
    );
    assert_eq!(
        errors("void() g = {};\nvoid() k = { g(); };\n#pragma autoproto\n"),
        [Code::E0216, Code::E0206, Code::E0216, Code::E0206]
    );
    // A prototype, a builtin and a value are compared, not redeclared.
    assert_eq!(errors("void() g;\n#pragma autoproto\nvoid() g = {};\n"), []);
    assert_eq!(
        errors("void() g = #5;\n#pragma autoproto\nvoid() g = #5;\n"),
        []
    );
    assert_eq!(
        errors("float x = 5;\n#pragma autoproto\nfloat x = 5;\n"),
        []
    );
    assert_eq!(
        errors("float x = 5;\n#pragma autoproto\nfloat x = 6;\n"),
        [Code::E0206]
    );
    assert_eq!(
        errors("void() h = {};\n#pragma autoproto\nvoid() k = { h(); };\n"),
        [Code::E0216, Code::E0206]
    );
}

// spec FTEF-12.19: a declaration read with `autoprototype` on is made, and its initialiser is
// skipped, its constness the written `=` decides
#[test]
fn what_follows_the_pragma_is_declared_and_skipped() {
    assert_eq!(
        errors("#pragma autoproto\nfloat x = 5;\nfloat y = x;\n"),
        []
    );
    assert_eq!(
        errors("#pragma autoproto\nvoid() g = { later(); };\nvoid() later = {};\n"),
        []
    );
    // The skipped body is read by the second pass, which finds what is wrong with it.
    assert_eq!(
        errors("#pragma autoproto\nvoid() g = { nothing(); };\n"),
        [Code::E0301]
    );
    // With no second pass the bodies it skipped are not defined.
    assert_eq!(
        errors("#pragma autoproto\nvoid() k = {};\n#pragma autoproto 0\n"),
        [Code::E0211]
    );
    assert_eq!(
        errors(
            "#pragma autoproto\nvoid() k = {};\nfloat x = 5;\n#pragma autoproto 0\nvoid() m = {};\n"
        ),
        [Code::E0211]
    );
    // A default value is not initialised in the first pass, where nothing is, and the second
    // pass finds it so before the constant it names is defined.
    assert_eq!(
        errors("#pragma autoproto\nvoid(float a = c) g = {};\nconst float c = 5;\n"),
        [Code::E0218, Code::E0211]
    );
}

// spec FTEF-12.20: the flags the first pass ends with are the second pass's from its first
// token, and the first pass reads with the ones it began with
#[test]
fn the_second_pass_starts_with_the_flags_the_first_ended_with() {
    // `assumevar` at the end: x is redeclared as non-constant, and y is no error.
    let assumevar =
        "#pragma autoproto\nfloat x = 5;\nfloat y = x;\n#pragma flag enable assumevar\n";
    assert_eq!(errors(assumevar), []);
    assert_eq!(codes_with(assumevar, &[]), [Code::W0212, Code::W0212]);
    // `assumevar` on at the start and off at the end: the second pass starts with it off.
    assert_eq!(
        errors(
            "#pragma flag enable assumevar\n#pragma autoproto\nfloat x = 5;\nfloat y = x;\n#pragma flag disable assumevar\n"
        ),
        [Code::E0214]
    );
    // `assumeint` at the end: the enum's names are integers in the second pass.
    assert_eq!(
        errors("#pragma autoproto\nenum { A, B };\nstring s = B;\n#pragma flag enable assumeint\n"),
        []
    );
    assert_eq!(errors("enum { A, B };\nstring s = B;\n"), [Code::E0213]);
    // `attributes` at the end is the second pass's: the first pass reads `[[` as it began.
    assert_eq!(
        errors("#pragma autoproto\n[[eraseable]] float x;\n#pragma flag enable attributes\n"),
        [Code::E0203]
    );
    assert_eq!(
        errors("#pragma flag enable attributes\n[[eraseable]] float x;\n"),
        []
    );
    // `qcfuncs` off at the end: `\9` is a bad character code from the first token.
    assert_eq!(
        errors("#pragma autoproto\nstring s = \"\\9\";\n#pragma flag disable qcfuncs\n"),
        [Code::E0103]
    );
    assert_eq!(errors("string s = \"\\9\";\n"), []);
}

// spec FTEF-12.20: the keywords and the operator priorities carry into the second pass, the
// warning settings are the command line's again
#[test]
fn keywords_priorities_and_settings_in_the_second_pass() {
    // A keyword turned off at the end is off from the second pass's first token.
    let for_loop = "void() g = { for (;;) {} };\n";
    assert_eq!(
        errors(&format!("{for_loop}#pragma keyword disable for\n")),
        []
    );
    assert!(
        !errors(&format!(
            "#pragma autoproto\n{for_loop}#pragma keyword disable for\n"
        ))
        .is_empty()
    );
    // `cpriority` set by a pragma at the end gives the second pass C's priorities, so `!a == b`
    // is `(!a) == b` and warns of nothing; the first pass reads QuakeC's.
    let probe = "float a, b;\nfloat() h = { return !a == b; };\n";
    assert_eq!(
        errors_with(
            &format!("#pragma autoproto\n{probe}#pragma flag enable cpriority\n"),
            &WERROR
        ),
        []
    );
    assert!(!errors_with(&format!("#pragma autoproto\n{probe}"), &WERROR).is_empty());
    // A `#pragma warning disable` of the first pass is not the second's.
    let redeclared = "float x;\nfloat x = 5;\nfloat() h = { return x; };\n";
    assert_eq!(
        errors_with(
            &format!("#pragma autoproto\n{redeclared}#pragma warning disable F329\n"),
            &WERROR
        ),
        errors_with(&format!("#pragma autoproto\n{redeclared}"), &WERROR)
    );
}

// spec FTEF-19.31: under `-Fcaseinsens` a type is found in any case where a declaration reads
// one, `QCC_TypeForName` searching its table so
#[test]
fn type_names_are_found_in_any_case() {
    let insens = ["-Fcaseinsens"];
    for source in [
        "Void() h = #6;\n",
        "VECTOR v;\n",
        "ENTITY e;\n",
        "String s;\n",
        ".Float f;\n",
        "Void(Float a) h = #6;\n",
        "typedef Entity e;\ne x;\n",
        "typedef float Foo;\nfoo x;\n",
        "typedef float foo;\nFOO x;\n",
        "struct S { float a; };\ns x;\n",
        "enum E { A, B };\ne x;\n",
        "class C { float x; };\nc a;\n",
        "__INT32 i;\n",
        "VARIANT v;\n",
    ] {
        assert_eq!(errors_with(source, &insens), [], "{source:?}");
    }
    // The same words as written are no type without the option, and `long` is none either way.
    for source in ["Void() h = #6;\n", "VECTOR v;\n", "typedef Entity e;\n"] {
        assert_eq!(errors(source), [Code::E0203], "{source:?}");
    }
    assert_eq!(errors_with("Long x;\n", &insens), [Code::E0203]);
    assert_eq!(errors_with("Char x;\n", &insens), [Code::E0203]);
}

// spec FTEF-19.31: a typedef of a name differing in case from one made, with another type, is
// "Cannot redeclare typedef"
#[test]
fn a_typedef_in_another_case_is_a_redeclaration() {
    let insens = ["-Fcaseinsens"];
    assert_eq!(
        errors_with("typedef float Foo;\ntypedef string foo;\n", &insens),
        [Code::E0219]
    );
    assert_eq!(
        errors_with("typedef float Foo;\ntypedef float foo;\n", &insens),
        []
    );
    assert_eq!(errors("typedef float Foo;\ntypedef string foo;\n"), []);
}

// spec FTEF-19.31: the members of a class are found in any case, by its name in any case, and
// an enum's value after `Scope::` is the exact one
#[test]
fn members_and_enum_values_in_any_case() {
    let insens = ["-Fcaseinsens"];
    let class = "class C { float x; void() f; };\nvoid() C::f = {};\n";
    assert_eq!(
        errors_with(
            &format!("{class}void() k = {{ local C a; C::F(a); }};\n"),
            &insens
        ),
        []
    );
    assert_eq!(
        errors_with(
            &format!("{class}void() k = {{ local C a; c::f(a); }};\n"),
            &insens
        ),
        []
    );
    // A member given a body again is "redeclaration of function body", in any case.
    assert_eq!(
        errors_with(&format!("{class}void() C::F = {{}};\n"), &insens),
        [Code::E0216]
    );
    assert_eq!(
        errors(&format!("{class}void() C::f = {{}};\n")),
        [Code::E0216]
    );
    // An enum's value after `Scope::` is exact, the scope is a type found in any case.
    let enumeration = "enum E { A, B };\n";
    assert_eq!(
        errors_with(&format!("{enumeration}float y = e::A;\n"), &insens),
        []
    );
    assert_eq!(
        errors_with(&format!("{enumeration}float y = E::a;\n"), &insens),
        [Code::E0301]
    );
    assert_eq!(
        errors_with(&format!("{enumeration}float y = a;\n"), &insens),
        []
    );
    assert_eq!(
        errors_with("enum class E : float { A, B };\nE x = e::A;\n", &insens),
        []
    );
}

// spec FTEF-19.31: an enum's value is a new definition each time, found before an older one
#[test]
fn an_enums_values_are_new_definitions() {
    assert_eq!(
        codes_with(
            "enum { A, B };\n#pragma flag enable assumeint\nenum { A, B };\nstring s = B;\n",
            &[]
        ),
        [Code::W0220]
    );
}

// spec FTEF-19.32: the scope of `Scope::name` is a type, "is not a type" when it is none
#[test]
fn the_scope_of_a_scoped_name_is_a_type() {
    assert_eq!(errors("void() f = { Nope::thing(); };\n"), [Code::E0203]);
    assert_eq!(errors("enum E { A, B };\nfloat y = e::A;\n"), [Code::E0203]);
    assert_eq!(errors("enum E { A, B };\nfloat y = E::A;\n"), []);
    assert_eq!(
        errors("float Other::g = 1;\nvoid() f = { local float n = Other::g; };\n"),
        [Code::E0225, Code::E0203]
    );
    assert_eq!(errors("typedef Void t;\n"), [Code::E0203]);
    assert_eq!(errors_with("typedef Void t;\n", &["-Fcaseinsens"]), []);
}
