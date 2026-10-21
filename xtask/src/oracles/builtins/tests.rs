// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_progs::{Function, ProgsFile, write};
use qc_syntax::{Code, Dialect};
use qc_types::Slot;

use super::super::testing::{fake_oracles, fresh_dir, root};
use super::{
    Case, FIXTURE, Outcome, Project, Tally, ULong, bound, cases, check, check_where, compare,
    expected_difference, graph_cases, graph_differences, oracle, profile, read_fixture,
};
use crate::exec::{Fake, Reply};

fn fte(source: &str) -> Case {
    Case::new(Dialect::Fteqcc, source.to_owned())
}

fn vanilla(source: &str) -> Case {
    Case::new(Dialect::Vanilla, source.to_owned())
}

fn bound_as(number: i32, name: &str) -> Outcome {
    Outcome::Bound {
        number,
        name: name.to_owned(),
    }
}

// spec FTE-23.15: every case of the cross product is recorded, and the profile's outcome is the
// pinned FTEQCC's or id's qcc's, or differs only as QC-27.9, FTE-23.11, FTEF-13.8 and
// FTEF-14.47 say
#[test]
fn the_profile_agrees_with_the_recorded_oracles() {
    let recorded = read_fixture(&root()).unwrap();
    let cases = cases();
    let mut keys: Vec<String> = cases.iter().chain(&graph_cases()).map(Case::key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), recorded.len());
    assert!(keys.iter().all(|key| recorded.contains_key(key)));
    let tally = compare(&cases, &recorded, ULong::Bits64);
    assert_eq!(tally.differ, 0, "{tally}");
    assert_eq!(
        tally.expected.keys().copied().collect::<Vec<_>>(),
        ["FTE-23.11", "FTEF-13.8", "FTEF-14.47", "QC-27.9"]
    );
    assert!(tally.agree > 6000, "{tally}");
}

// spec PRJ-5.13: for every graph case the project graph lists the file that binds `f` exactly
// when the recorded FTEQCC binds `f` from it, and FTEQCC either binds `f` or accepts the
// program without it, so that each case says what it read
#[test]
fn the_graph_lists_what_the_recorded_fteqcc_compiles() {
    let recorded = read_fixture(&root()).unwrap();
    let differ = graph_differences(&recorded);
    assert!(differ.is_empty(), "{differ:#?}");
    let cases = graph_cases();
    assert_eq!(cases.len(), 148);
    let read = cases
        .iter()
        .filter(|case| matches!(recorded[&case.key()], Outcome::Bound { .. }))
        .count();
    assert!(read > 100 && read < 140, "{read} of {} read", cases.len());
}

// spec PRJ-5.13: a graph case whose graph reads otherwise than the oracle, whose oracle refused
// the program, or which is not recorded, is a difference
#[test]
fn graph_differences_are_found() {
    let recorded = read_fixture(&root()).unwrap();
    let cases = graph_cases();
    let first = cases[0].key();
    assert_eq!(recorded[&first], bound_as(5, "f"));
    for (outcome, expected) in [
        (
            Outcome::Accepted,
            "the oracle does not read the file, the graph does",
        ),
        (Outcome::Rejected, "oracle rejected, which reads no file"),
        (bound_as(6, "f"), "oracle bound 6 f, which reads no file"),
    ] {
        let mut changed = recorded.clone();
        changed.insert(first.clone(), outcome);
        let differ = graph_differences(&changed);
        assert_eq!(differ.len(), 1, "{differ:?}");
        assert!(differ[0].ends_with(expected), "{differ:?}");
    }
    let unread = cases
        .iter()
        .find(|case| recorded[&case.key()] == Outcome::Accepted)
        .unwrap()
        .key();
    let mut changed = recorded.clone();
    changed.insert(unread, bound_as(5, "f"));
    let differ = graph_differences(&changed);
    assert!(differ[0].ends_with("the oracle reads the file, the graph does not"));
    let mut missing = recorded;
    missing.remove(&first);
    let differ = graph_differences(&missing);
    assert!(differ[0].ends_with("not recorded in xtask/fixtures/oracles/builtin-outcomes.json"));
}

// spec FTEF-14.50: an oracle where `unsigned long` is 32 bits may read a `#pragma line` number
// of 2^32 or more and below 2^64 otherwise than the profile, and only that; the host's width is
// C's
#[test]
fn a_wide_line_pragma_differs_only_where_unsigned_long_is_32_bits() {
    let ours = bound_as(0, "1");
    let oracle = bound_as(0, "-1");
    for (source, ulong, expected) in [
        (
            "#pragma line 4294967297\nvoid() f = :__LINESTR__;\n",
            ULong::Bits32,
            Some("FTEF-14.50"),
        ),
        (
            "#pragma line 4294967297\nvoid() f = :__LINESTR__;\n",
            ULong::Bits64,
            None,
        ),
        (
            "#pragma LINE(4294967296)\nvoid() f = :__LINESTR__;\n",
            ULong::Bits32,
            Some("FTEF-14.50"),
        ),
        (
            "  #  pragma line 18446744073709551614\n",
            ULong::Bits32,
            Some("FTEF-14.50"),
        ),
        ("#pragma line 18446744073709551615\n", ULong::Bits32, None),
        ("#pragma line 8589934591\n", ULong::Bits32, None),
        (
            "#pragma line 0x100000001\n",
            ULong::Bits32,
            Some("FTEF-14.50"),
        ),
        (
            "#pragma\tline\t4294967297\n",
            ULong::Bits32,
            Some("FTEF-14.50"),
        ),
        (
            "#pragma line -4294967298\n",
            ULong::Bits32,
            Some("FTEF-14.50"),
        ),
        ("#pragma line 037777777777\n", ULong::Bits32, None),
        ("#pragma line -1\n", ULong::Bits32, None),
        (
            "#pragma line 4294967295\nvoid() f = :__LINESTR__;\n",
            ULong::Bits32,
            None,
        ),
        ("#pragma line 18446744073709551616\n", ULong::Bits32, None),
        ("#pragma line 99999999999999999999\n", ULong::Bits32, None),
        ("#pragma linex 4294967297\n", ULong::Bits32, None),
        ("#pragma lin\n", ULong::Bits32, None),
        ("x #pragma line 4294967297\n", ULong::Bits32, None),
        ("void() f = #5; // 4294967297\n", ULong::Bits32, None),
    ] {
        assert_eq!(
            expected_difference(&fte(source), &ours, &[], &oracle, ulong),
            expected,
            "{source:?} {ulong:?}"
        );
    }
    // Only the line `__LINESTR__` names may differ: another builtin number is a failure.
    let case = fte("#pragma line 4294967297\nvoid() f = :__LINESTR__;\n");
    let other = bound_as(5, "-1");
    let difference = expected_difference(&case, &ours, &[], &other, ULong::Bits32);
    assert_eq!(difference, None);
    let rejected = Outcome::Rejected;
    let difference = expected_difference(&case, &ours, &[], &rejected, ULong::Bits32);
    assert_eq!(difference, None);
}

// spec FTE-23.15: the cross product has both dialects, every prefix, value, suffix and
// declarator, and each pair with and without `g` first, each case once
#[test]
fn the_cross_product_holds_every_form_once() {
    let cases = cases();
    let mut keys: Vec<String> = cases.iter().map(Case::key).collect();
    let all = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), all);
    for source in [
        "void() f = #16d;\n",
        "void() f = # 5;\n",
        "void() f = #\"ab\" \"cd\";\n",
        "void() f = : \"ab\" \"cd\":ex_b;\n",
        "void f() = :16d;\n",
        "void() f #5;\n",
        "void() f = #5;\nvoid() f = #5:ex_a;\n",
        "void() g = #5:ex_a;\nvoid() f = #5:ex_a;\nvoid() f = #5:ex_a;\n",
    ] {
        assert!(cases.contains(&fte(source)), "{source:?}");
    }
    assert!(cases.contains(&vanilla("void() f = #5;\nvoid() f = #6;\n")));
    // QC-13.3: the vanilla dialect is given twice only the forms of `#number`.
    assert!(!cases.contains(&vanilla("void() f = :ex_a;\nvoid() f = :ex_a;\n")));
    assert!(cases.contains(&fte("void() f = :ex_a;\nvoid() f = :ex_a;\n")));
    assert!(cases.contains(&vanilla(&format!("void() f = :{};\n", "n".repeat(300)))));
}

// spec FTE-23.15: under FTEQCC the cross product has every character-constant escape, a macro
// name before every ASCII byte that is no part of a name and before UTF-8 characters, in the
// source, a body and an argument, and labels and builtins a macro's argument meets
#[test]
fn the_cross_product_holds_the_macro_and_escape_families() {
    let cases = cases();
    let body = |define: &str, statement: &str| {
        fte(&format!(
            "#define N 5\n{define}\nvoid() f = #5;\nvoid() g = {{ local float x; {statement} }};\n"
        ))
    };
    for escape in ["s", "{6z}", "u041", "\r\n"] {
        assert!(
            cases.contains(&fte(&format!("void() f = :'\\{escape}';\n"))),
            "{escape:?}"
        );
    }
    let mut words = 0;
    for byte in (1..=0x7f_u8).map(char::from) {
        let named = byte.is_ascii_alphanumeric() || byte == '_';
        let follower = format!("x = N{byte}{};", super::word_continuation(byte));
        assert_eq!(cases.contains(&body("", &follower)), !named, "{byte:?}");
        words += usize::from(!named);
    }
    assert_eq!(words, 64);
    assert!(cases.contains(&body("", "x = N\u{10000};")));
    assert!(cases.contains(&body("#define A(a) a!=5", "x = A(N);")));
    assert!(cases.contains(&body("#define V(...) __VA_ARGS__:", "V(lbl ) goto lbl;")));
    assert!(cases.contains(&fte("#define E(n) #5:n\nvoid() f = E(ex_a);\n")));
}

// spec FTE-23.15: under FTEQCC the cross product has every macro-expansion initialiser and
// statement: pastes, strings, empty arguments, recursion and argument lists past a chunk
#[test]
fn the_cross_product_holds_the_expansion_families() {
    let cases = cases();
    for (define, use_) in super::EXPANSION_INITIALISERS {
        let source = format!("{define}\nvoid() f = {use_};\n");
        assert!(cases.contains(&fte(&source)), "{source:?}");
    }
    for (define, statement) in super::EXPANSION_STATEMENTS {
        let source = format!(
            "#define N 5\n{define}\nvoid() f = #5;\nvoid() g = {{ local float x; {statement} }};\n"
        );
        assert!(cases.contains(&fte(&source)), "{source:?}");
    }
    assert_eq!(super::EXPANSION_INITIALISERS.len(), 102);
    assert_eq!(super::EXPANSION_STATEMENTS.len(), 18);
}

// spec FTE-23.15: under FTEQCC the cross product has the macro-text family: separators in an
// argument's character constant, comment or string, stored bodies, parameter lists, variadic
// macros, built-in constants' lines and `__FILE__`, and functions with no body
#[test]
fn the_cross_product_holds_the_macro_text_family() {
    let cases = cases();
    let family = super::macro_text_cases();
    for source in &family {
        assert!(cases.contains(&fte(source)), "{source:?}");
    }
    assert_eq!(family.len(), 188);
    for source in [
        "#define E(n) :n\nvoid() f = E(',');\n",
        "#define M(x) x\n#define K M /**/ (#5)\nvoid() f = K;\n",
        "#define E(n, n) n\nvoid() f = E(#5, 6);\n",
        "#define V(...) :__VA_COUNT__\nvoid() f = V(a, b);\n",
        "void() f = #5;\nvoid() g;\n",
        "#pragma file(ex_a:b)\nvoid() f = :__FILE__;\n",
    ] {
        assert!(family.iter().any(|s| s == source), "{source:?}");
    }
}

// spec FTE-23.15: under FTEQCC the cross product has the directive family: line ends, names,
// conditionals, each form in each place, F303's settings, `#pragma line`, the other pragmas,
// lists, and projects of several files whose keys hold every file
#[test]
fn the_cross_product_holds_the_directive_family() {
    let cases = cases();
    let family = super::directives::cases();
    for case in &family {
        assert!(cases.contains(case), "{case:?}");
    }
    assert_eq!(family.len(), 1338);
    for source in [
        "#define X 1\n#undef X // c \\\nzz zz zz\nvoid() f = #5;\n",
        "#defined N #5\nvoid() f = N;\n",
        "#ifdef Q\n#iffy\n#endif\nzz\n#endif\nvoid() f = #5;\n",
        "#define M(x, y) y\nvoid() f = M(1,\n#pragma line(40)\n#5);\n",
        "#pragma warning disable F303\n#define D \\\n#define Y #5 \\\nY\nvoid() f = D;\n",
        "#pragma line 0x10\nvoid() f = :__LINESTR__;\n",
        "#includelist\n#define N #5\n#endlist\nvoid() f = N;\n",
    ] {
        assert!(family.contains(&fte(source)), "{source:?}");
    }
    let nested = family
        .iter()
        .find(|case| {
            case.project
                .as_ref()
                .is_some_and(|p| p.compiled == "src/a.qc")
        })
        .unwrap();
    let project = nested.project.as_ref().unwrap();
    assert_eq!(nested.compiled(), "src/a.qc");
    assert!(nested.key().starts_with("fteqcc|src/a.qc|#include"));
    for (name, text) in &project.files {
        assert!(nested.key().contains(&format!("|{name}={text}")));
    }
    assert_eq!(fte("x").compiled(), "t.qc");
    assert_eq!(fte("x").key(), "fteqcc|x");
}

// spec FTEF-14.47: an `#eof` past a file's first byte, where FTEQCC crashes, and `#merge` and
// `#datafile`, whose missing files FTEQCC refuses, may differ only when the profile has no error
#[test]
fn fteqcc_may_differ_where_it_crashes_or_opens_a_file() {
    let rejected = Outcome::Rejected;
    let bound = bound_as(5, "f");
    let eof = fte("void() f = #5;\n#eof\nzz\n");
    let first = fte("#eof\nzz\n");
    let merge = fte("#merge \"x.dat\"\nvoid() f = #5;\n");
    let mut datafile = fte("#include \"b.qh\"\nvoid() f = #5;\n");
    datafile.project = Some(Project {
        compiled: "t.qc".to_owned(),
        files: vec![("b.qh".to_owned(), "#datafile \"x\"\n".to_owned())],
        include_dirs: Vec::new(),
    });
    let plain = fte("void() f = #5;\n");
    let indented = fte("void() f = #5;\n \t#eofx\n");
    let commented = fte("void() f = #5; // #eof\n/* #eof */\n");
    let quoted = fte("void() f = #5;\nstring s = \"#eof\";\n");
    let mut included = fte("#include \"b.qh\"\nvoid() f = #5;\n");
    included.project = Some(Project {
        compiled: "t.qc".to_owned(),
        files: vec![("b.qh".to_owned(), "zz\n#eof\n".to_owned())],
        include_dirs: Vec::new(),
    });
    for (case, errors, oracle, expected) in [
        (&eof, &[][..], &rejected, Some("FTEF-14.47")),
        (&eof, &[Code::E0121], &rejected, None),
        (&eof, &[], &Outcome::Accepted, None),
        (&first, &[], &rejected, None),
        (&indented, &[], &rejected, Some("FTEF-14.47")),
        (&commented, &[], &rejected, None),
        (&quoted, &[], &rejected, None),
        (&included, &[], &rejected, Some("FTEF-14.47")),
        (&merge, &[], &rejected, Some("FTEF-14.47")),
        (&datafile, &[], &rejected, Some("FTEF-14.47")),
        (&plain, &[], &rejected, None),
    ] {
        assert_eq!(
            expected_difference(case, &bound, errors, oracle, ULong::Bits64),
            expected,
            "{case:?} {errors:?} {oracle}"
        );
    }
}

// spec FTE-23.15: an outcome is written and read back in one form; any other text is refused
#[test]
fn outcomes_round_trip() {
    for outcome in [
        bound_as(-5, "ex_a"),
        bound_as(0, "a b"),
        bound_as(0, ""),
        Outcome::Accepted,
        Outcome::Redeclared,
        Outcome::Rejected,
    ] {
        assert_eq!(Outcome::parse(&outcome.to_string()), Some(outcome));
    }
    for text in ["bound 5", "bound x f", "", "accepted 1", "rejected "] {
        assert_eq!(Outcome::parse(text), None, "{text}");
    }
}

// spec QC-27.9: the only expected difference is the vanilla profile's error for a builtin
// number not above 0, where id's qcc does not refuse; without that error the outcomes agree
#[test]
fn only_a_vanilla_number_not_above_zero_may_differ() {
    let case = vanilla("void() f = #0;\n");
    let pair = vanilla("void() f = #0;\nvoid() f = #5;\n");
    for (case, errors, oracle, expected) in [
        (
            &case,
            &[Code::E0507][..],
            Outcome::Accepted,
            Some("QC-27.9"),
        ),
        (&case, &[Code::E0507], Outcome::Redeclared, None),
        (&case, &[Code::E0507], Outcome::Rejected, None),
        (&case, &[Code::E0507], bound_as(0, "f"), None),
        (&case, &[Code::E0209], Outcome::Accepted, None),
        (&case, &[], Outcome::Accepted, None),
        (
            &pair,
            &[Code::E0507, Code::E0206],
            Outcome::Redeclared,
            Some("QC-27.9"),
        ),
        (&pair, &[Code::E0507, Code::E0206], Outcome::Rejected, None),
        (
            &pair,
            &[Code::E0507, Code::E0209],
            Outcome::Rejected,
            Some("QC-27.9"),
        ),
        (
            &pair,
            &[Code::E0507, Code::E0209],
            Outcome::Redeclared,
            None,
        ),
        (
            &fte("void() f = #0;\n"),
            &[Code::E0507],
            Outcome::Accepted,
            None,
        ),
    ] {
        assert_eq!(
            expected_difference(case, &Outcome::Rejected, errors, &oracle, ULong::Bits64),
            expected,
            "{case:?} {errors:?} {oracle}"
        );
    }
}

// spec FTE-23.11: a macro after `#` that leaves `f` initialised with a number is FTEQCC's
// "Cannot cast" error and no error here; only that case may differ so
// spec FTEF-13.8: a case FTEQCC never finishes may differ, as the profile ends
#[test]
fn fteqcc_differs_only_where_it_casts_or_hangs() {
    let number = fte("#define N 5\nvoid() f = #N;\n");
    let string = fte("#define N \"ex_a\"\nvoid() f = #N;\n");
    let plain = fte("void() f = 5;\n");
    let hangs = fte("void() f = :\"\\UFFFFFFFF\";\n");
    let accepted = Outcome::Accepted;
    let rejected = Outcome::Rejected;
    for (case, ours, errors, oracle, expected) in [
        (&number, &accepted, &[][..], &rejected, Some("FTE-23.11")),
        (&number, &accepted, &[Code::E0209], &rejected, None),
        (&number, &rejected, &[], &accepted, None),
        (&number, &bound_as(5, "f"), &[], &rejected, None),
        (&string, &accepted, &[], &rejected, None),
        (&plain, &accepted, &[], &rejected, None),
        (
            &hangs,
            &bound_as(0, "x"),
            &[],
            &Outcome::Hangs,
            Some("FTEF-13.8"),
        ),
        (&hangs, &bound_as(0, "x"), &[], &rejected, None),
        // A case that is not one FTEQCC never finishes is a failure when it times out.
        (&plain, &accepted, &[], &Outcome::Hangs, None),
    ] {
        assert_eq!(
            expected_difference(case, ours, errors, oracle, ULong::Bits64),
            expected,
            "{case:?} {ours} {errors:?} {oracle}"
        );
    }
}

// spec FTE-23.10: the profile's binding is read from the preprocessor's text, so a macro after
// `#` or `:` binds what it stands for; a builtin's own long name is found by its name
#[test]
fn bindings_are_read_after_the_preprocessor() {
    assert_eq!(
        profile(&fte("#define N #5:ex_a\nvoid() f = N;\n")).0,
        bound_as(5, "ex_a")
    );
    assert_eq!(
        profile(&fte("#define N ex_a\nvoid() f = :N;\n")).0,
        bound_as(0, "ex_a")
    );
    assert_eq!(
        profile(&fte("#define N 5\nvoid() f = #N;\n")).0,
        Outcome::Accepted
    );
    let long = "n".repeat(254);
    assert_eq!(
        profile(&fte(&format!("void() {long} = #5;\n"))).0,
        bound_as(5, &long)
    );
    // QC-9.16: a builtin a macro body writes binds as written.
    assert_eq!(
        profile(&fte("#define B #5\nvoid() f = B;\n")).0,
        bound_as(5, "f")
    );
    for (source, name) in [
        ("void f() = #5;\n", "f"),
        ("void() n = #5;\n", "f"),
        ("void() nx = #5;\n", "f"),
        ("void() nn = #5;\n", "nn"),
        ("void() g = #5;\nvoid() f = #5;\n", "f"),
    ] {
        assert_eq!(super::function_name(&fte(source)), name, "{source:?}");
    }
}

// spec QC-13.4: id's qcc's `#0` is found from the source text, whatever the number's form
#[test]
fn a_vanilla_zero_is_read_from_the_source() {
    for (source, zero) in [
        ("void() f = #0;\n", true),
        ("void() f = # -0;\n", true),
        ("void() f = #0.0;\n", true),
        ("void() g = #0;\nvoid() f = #0;\nvoid() f = #5;\n", true),
        ("void() f = #5;\n", false),
        ("void() f = #0.5;\n", false),
        ("void() f = #-;\n", false),
        ("void() f;\n", false),
    ] {
        assert_eq!(super::vanilla_zero(&vanilla(source)), zero, "{source:?}");
    }
}

// spec FTE-23.7: under FTEQCC a redeclaration is reported only when it is the only error, since
// FTEQCC reads the initialiser first; under the vanilla dialect when it is the first, since
// id's qcc stops at the `=`
#[test]
fn the_profile_classifies_its_errors_as_each_compiler_stops() {
    let twice_bad = "void() f = #5;\nvoid() f = #16d;\n";
    assert_eq!(profile(&fte(twice_bad)).0, Outcome::Rejected);
    assert_eq!(
        profile(&fte("void() f = #5;\nvoid() f = #6;\n")),
        (Outcome::Redeclared, vec![Code::E0206])
    );
    assert_eq!(
        profile(&vanilla("void() f = #5;\nvoid() f = #5x;\n")).0,
        Outcome::Redeclared
    );
    assert_eq!(
        profile(&vanilla("void() f = #0;\nvoid() f = #5;\n")),
        (Outcome::Rejected, vec![Code::E0507, Code::E0206])
    );
    assert_eq!(
        profile(&fte("void() f = #5:ex_a;\n")).0,
        bound_as(5, "ex_a")
    );
    assert_eq!(profile(&fte("void() f = #\"\";\n")).0, bound_as(0, "f"));
    assert_eq!(profile(&vanilla("void() f = #16;\n")).0, bound_as(16, "f"));
    // A program with no builtin `f` binds nothing.
    assert_eq!(profile(&fte("void() g = #5;\n")).0, Outcome::Accepted);
    assert_eq!(profile(&fte("float f;\n")).0, Outcome::Accepted);
}

/// A `progs.dat` whose functions after the null one are `functions`, each a builtin number
/// and a name.
fn progs_with(functions: &[(i32, &str)]) -> Vec<u8> {
    let mut strings = vec![0_u8];
    let mut records = vec![Function::default()];
    for (number, name) in functions {
        let s_name = i32::try_from(strings.len()).unwrap();
        // Each `char` one byte (Latin-1), as the oracle reads a name back.
        strings.extend(name.chars().map(|c| u8::try_from(u32::from(c)).unwrap()));
        strings.push(0);
        records.push(Function {
            first_statement: number.wrapping_neg(),
            s_name,
            ..Function::default()
        });
    }
    let file = ProgsFile {
        entityfields: 0,
        statements: vec![qc_progs::Statement::default()],
        functions: records,
        strings,
        globals: vec![Slot(0); 28],
        ..ProgsFile::default()
    };
    write(&file).unwrap()
}

fn read(bytes: &[u8]) -> qc_progs::Progs<'_> {
    let options = qc_progs::ReadOptions {
        check_header_crc: false,
    };
    qc_progs::Progs::read_with(bytes, &options).unwrap()
}

// spec FTE-23.15: `f`'s function is the first, or the second after `g`'s; FTEQCC writes every
// builtin's number as `-first_statement`, id's qcc only a nonzero one's (QC-13.4)
#[test]
fn bound_reads_f_s_function_by_its_place() {
    let bytes = progs_with(&[(5, "ex_g"), (-5, "ex_f")]);
    let progs = read(&bytes);
    assert_eq!(
        bound(&progs, &fte("void() f = #5;\n")),
        Some(bound_as(5, "ex_g"))
    );
    assert_eq!(
        bound(&progs, &fte("void() g = #5;\nvoid() f = #-5;\n")),
        Some(bound_as(-5, "ex_f"))
    );
    assert_eq!(
        bound(&progs, &vanilla("void() g = #5;\nvoid() f = #-5;\n")),
        Some(Outcome::Accepted)
    );
    assert_eq!(
        bound(&progs, &vanilla("void() f = #5;\n")),
        Some(bound_as(5, "ex_g"))
    );
    let zero = progs_with(&[(0, "f")]);
    assert_eq!(
        bound(&read(&zero), &vanilla("void() f = #0;\n")),
        Some(Outcome::Accepted)
    );
    assert_eq!(
        bound(&read(&zero), &fte("void() f = #0;\n")),
        Some(bound_as(0, "f"))
    );
    // FTE-23.11: FTEQCC makes no function for `f` initialised with a string; a function
    // missing before `f`'s place is no such case.
    let none = progs_with(&[]);
    assert_eq!(
        bound(&read(&none), &fte("void() f = #0;\n")),
        Some(Outcome::Accepted)
    );
    assert_eq!(bound(&read(&none), &vanilla("void() f = #5;\n")), None);
    let after_g = fte("void() g = #5;\nvoid() f = #5;\n");
    assert_eq!(bound(&read(&none), &after_g), None);
}

/// What the fake compilers write for each outcome: FTEQCC's and qcc's own words.
fn reply(dir: &Path, fteqcc: bool, case: &Case, outcome: &Outcome) -> Reply {
    let failed = |line: &str| Reply {
        code: 1,
        stdout: format!(
            "compiling t.qc\nt.qc:1: warning: w\n{line}\nt.qc:1:    f is defined here\n"
        ),
        ..Reply::default()
    };
    match outcome {
        Outcome::Hangs => Reply {
            hangs: true,
            ..Reply::default()
        },
        Outcome::Rejected => failed("t.qc:1: error: Bad builtin immediate"),
        Outcome::Redeclared if fteqcc => failed("t.qc:2: error: incompatible redeclaration"),
        Outcome::Redeclared => failed("t.qc:2:f redeclared"),
        // FTEQCC accepts with no builtin only a function it makes none for (FTE-23.11).
        Outcome::Accepted if fteqcc => Reply {
            files: vec![(dir.join("progs.dat"), progs_with(&[]))],
            ..Reply::exit(0)
        },
        Outcome::Accepted => Reply {
            files: vec![(dir.join("progs.dat"), b"not a progs".to_vec())],
            ..Reply::exit(0)
        },
        Outcome::Bound { number, name } => {
            let mut functions = vec![(*number, name.as_str())];
            if case.source.starts_with("void() g") {
                functions.insert(0, (1, "g"));
            }
            Reply {
                files: vec![(dir.join("progs.dat"), progs_with(&functions))],
                ..Reply::exit(0)
            }
        }
    }
}

/// Fake compilers that give each case the outcome `recorded` has for it.
fn answering(recorded: BTreeMap<String, Outcome>) -> impl Fn(&Command) -> Reply {
    move |command: &Command| {
        let dir = command.get_current_dir().unwrap().to_path_buf();
        let fteqcc = command.get_program() == "fteqcc";
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let include_dirs: Vec<String> = args
            .iter()
            .filter_map(|arg| arg.strip_prefix("-I").map(str::to_owned))
            .collect();
        assert_eq!(
            args.len(),
            include_dirs.len().saturating_add(usize::from(fteqcc))
        );
        assert!(fteqcc || include_dirs.is_empty());
        let list = fs::read_to_string(dir.join("progs.src")).unwrap();
        let compiled = list.strip_prefix("progs.dat\n").unwrap().trim_end();
        assert!(!dir.join("progs.dat").exists());
        let dialect = if fteqcc {
            Dialect::Fteqcc
        } else {
            Dialect::Vanilla
        };
        let source = fs::read_to_string(dir.join(compiled)).unwrap();
        let mut files = Vec::new();
        written(&dir, &dir, &mut files);
        files.retain(|(name, _)| name != "progs.src" && name != compiled);
        files.sort();
        let project = (dir.ends_with("project")).then(|| Project {
            compiled: compiled.to_owned(),
            files,
            include_dirs,
        });
        let case = Case {
            dialect,
            source,
            project,
        };
        reply(&dir, fteqcc, &case, &recorded[&case.key()])
    }
}

/// Each file under `dir`, by its name from `root` with `/` between its parts, and its text.
fn written(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            written(root, &path, files);
            continue;
        }
        let parts: Vec<String> = path
            .strip_prefix(root)
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        files.push((parts.join("/"), fs::read_to_string(&path).unwrap()));
    }
}

/// A repository whose fixture is `fixture`.
fn repository(name: &str, fixture: &BTreeMap<String, Outcome>) -> PathBuf {
    let dir = fresh_dir(name);
    let path = dir.join(FIXTURE);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let raw: BTreeMap<&String, String> = fixture.iter().map(|(k, v)| (k, v.to_string())).collect();
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();
    dir
}

// spec FTE-23.15: the check runs every case through the oracle of its dialect and passes when
// the oracles give what the fixture records and the profile agrees
#[test]
fn check_passes_when_the_oracles_give_the_recording() {
    let recorded = read_fixture(&root()).unwrap();
    let repo = repository("builtins-agree", &recorded);
    let fake = Fake::new(answering(recorded.clone()));
    let oracles = fake_oracles(&repo.join("work"), &fake);
    assert!(check(&repo, &oracles, false).unwrap());
    assert_eq!(fake.seen.borrow().len(), recorded.len());
    assert!(fake.seen.borrow().iter().any(|line| line == "fteqcc -Tid"));
    assert!(fake.seen.borrow().iter().any(|line| line == "qcc"));
}

// spec FTE-23.15: an oracle that gives another outcome than the fixture fails the check, and
// `--record` writes what the oracles gave
#[test]
fn check_fails_on_a_stale_recording_and_records_anew() {
    let recorded = read_fixture(&root()).unwrap();
    let key = fte("void() f = #5;\n").key();
    let mut stale = recorded.clone();
    stale.insert(key.clone(), bound_as(6, "f"));
    let repo = repository("builtins-stale", &stale);
    let fake = Fake::new(answering(recorded.clone()));
    let oracles = fake_oracles(&repo.join("work"), &fake);
    assert!(!check(&repo, &oracles, false).unwrap());
    // spec FTEF-14.50: where `unsigned long` is 32 bits nothing is recorded, and a wide
    // `#pragma line` the oracle reads otherwise there is no stale recording
    assert!(check_where(&repo, &oracles, true, ULong::Bits32).is_err());
    assert_eq!(read_fixture(&repo).unwrap(), stale);
    assert!(check_where(&repo, &oracles, true, ULong::Bits64).unwrap());
    assert_eq!(read_fixture(&repo).unwrap(), recorded);
    let wide = fte("#pragma line 4294967297\nvoid() f = :__LINESTR__;\n");
    let mut llp64 = recorded.clone();
    llp64.insert(wide.key(), bound_as(0, "-1"));
    let fake32 = Fake::new(answering(llp64));
    let oracles32 = fake_oracles(&repo.join("work"), &fake32);
    assert!(check_where(&repo, &oracles32, false, ULong::Bits32).unwrap());
    assert!(!check_where(&repo, &oracles32, false, ULong::Bits64).unwrap());
    // A recording of a case the cross product no longer makes is stale too.
    let mut extra = recorded.clone();
    extra.insert("fteqcc|gone".to_owned(), Outcome::Rejected);
    let repo = repository("builtins-extra", &extra);
    let oracles = fake_oracles(&repo.join("work"), &fake);
    assert!(!check_where(&repo, &oracles, false, ULong::Bits64).unwrap());
}

// spec FTE-23.15: an oracle whose outcome the profile does not share fails the check, even
// when the recording has it
#[test]
fn check_fails_when_the_profile_differs() {
    let mut recorded = read_fixture(&root()).unwrap();
    recorded.insert(fte("void() f = #16d;\n").key(), bound_as(16, "f"));
    let repo = repository("builtins-differ", &recorded);
    let fake = Fake::new(answering(recorded.clone()));
    let oracles = fake_oracles(&repo.join("work"), &fake);
    assert!(!check(&repo, &oracles, false).unwrap());
    let tally = compare(&cases(), &recorded, ULong::Bits64);
    assert_eq!(tally.differ, 1);
    let mut missing = recorded;
    missing.remove(&fte("void() f = #5;\n").key());
    assert_eq!(compare(&cases(), &missing, ULong::Bits64).differ, 2);
}

// spec FTE-23.15: a compiler that cannot run, a progs it does not write, and an FTEQCC progs
// qc-progs refuses are errors of the check, not outcomes
#[test]
fn oracle_failures_are_errors() {
    let dir = fresh_dir("builtins-failures");
    let case = fte("void() f = #5;\n");
    let cannot = Fake::new(|_: &Command| Reply {
        cannot_run: true,
        ..Reply::default()
    });
    let err = oracle(&fake_oracles(&dir, &cannot), &dir, &case).unwrap_err();
    assert!(err.contains("cannot run fteqcc"), "{err}");
    let err = oracle(
        &fake_oracles(&dir, &cannot),
        &dir,
        &vanilla("void() f = #5;\n"),
    )
    .unwrap_err();
    assert!(err.contains("cannot run qcc"), "{err}");
    let silent = Fake::new(|_: &Command| Reply::exit(0));
    let err = oracle(&fake_oracles(&dir, &silent), &dir, &case).unwrap_err();
    assert!(err.contains("progs.dat"), "{err}");
    let garbage = dir.join("progs.dat");
    let refused = Fake::new(move |_: &Command| Reply {
        files: vec![(garbage.clone(), b"not a progs".to_vec())],
        ..Reply::exit(0)
    });
    let err = oracle(&fake_oracles(&dir, &refused), &dir, &case).unwrap_err();
    assert!(err.contains("qc-progs refuses"), "{err}");
    assert!(err.starts_with("fteqcc \"void() f = #5;\\n\": "), "{err}");
    assert_eq!(
        oracle(
            &fake_oracles(&dir, &refused),
            &dir,
            &vanilla("void() f = #5;\n")
        )
        .unwrap(),
        Outcome::Accepted
    );
    // QC-13.4: whatever id's qcc wrote for `#0` is not recorded.
    let heap = dir.join("progs.dat");
    let uninitialised = Fake::new(move |_: &Command| Reply {
        files: vec![(heap.clone(), progs_with(&[(7, "f")]))],
        ..Reply::exit(0)
    });
    for (case, outcome) in [
        (vanilla("void() f = #0;\n"), Outcome::Accepted),
        (vanilla("void() f = #-0;\n"), Outcome::Accepted),
        (vanilla("void() f = #7;\n"), bound_as(7, "f")),
        (fte("void() f = #0;\n"), bound_as(7, "f")),
    ] {
        assert_eq!(
            oracle(&fake_oracles(&dir, &uninitialised), &dir, &case).unwrap(),
            outcome,
            "{case:?}"
        );
    }
    let empty = dir.join("progs.dat");
    let no_function = Fake::new(move |_: &Command| Reply {
        files: vec![(empty.clone(), progs_with(&[]))],
        ..Reply::exit(0)
    });
    let after_g = fte("void() g = #5;\nvoid() f = #5;\n");
    let err = oracle(&fake_oracles(&dir, &no_function), &dir, &after_g).unwrap_err();
    assert!(err.contains("no function f"), "{err}");
}

// spec FTE-23.15: an oracle killed at its time limit hangs, an FTEQCC with nothing to write
// accepts, and one that writes no progs for another reason is an error of the check
#[test]
fn oracles_that_hang_or_write_nothing() {
    let dir = fresh_dir("builtins-hang");
    let case = fte("void() f = :\"\\UFFFFFFFF\";\n");
    let hangs = Fake::new(|_: &Command| Reply {
        hangs: true,
        ..Reply::default()
    });
    assert_eq!(
        oracle(&fake_oracles(&dir, &hangs), &dir, &case).unwrap(),
        Outcome::Hangs
    );
    let says = |stdout: &'static str| {
        Fake::new(move |_: &Command| Reply {
            stdout: stdout.to_owned(),
            ..Reply::exit(0)
        })
    };
    let zero = fte("#define N 0\nvoid() f = #N;\n");
    assert_eq!(
        oracle(
            &fake_oracles(&dir, &says("nothing to write\n")),
            &dir,
            &zero
        )
        .unwrap(),
        Outcome::Accepted
    );
    let err = oracle(&fake_oracles(&dir, &says("done\n")), &dir, &zero).unwrap_err();
    assert!(
        err.contains("No such file") || err.contains("cannot find"),
        "{err}"
    );
    let vanilla_nothing = vanilla("void() f = #5;\n");
    assert!(
        oracle(
            &fake_oracles(&dir, &says("nothing to write\n")),
            &dir,
            &vanilla_nothing
        )
        .is_err()
    );
}

// spec FTE-23.15: a refusal is a redeclaration only when the first error the compiler reports at
// a line of the file is its redeclaration error; warnings and "defined here" notes are skipped
#[test]
fn oracle_reads_the_first_error() {
    let dir = fresh_dir("builtins-errors");
    for (fteqcc, stdout, outcome) in [
        (
            true,
            "t.qc:1: warning: x\nt.qc:2: error: incompatible redeclaration\n",
            Outcome::Redeclared,
        ),
        (
            true,
            "t.qc:2:    const void() f is defined here\nt.qc:2: error: Bad builtin immediate\n\
             t.qc:3: error: incompatible redeclaration\n",
            Outcome::Rejected,
        ),
        (true, "t.qc:2:f redeclared\n", Outcome::Rejected),
        (false, "t.qc:2:f redeclared\n", Outcome::Redeclared),
        (
            false,
            "t.qc:1:Bad builtin immediate\nt.qc:2:f redeclared\n",
            Outcome::Rejected,
        ),
        (false, "compilation errors\n", Outcome::Rejected),
    ] {
        let fake = Fake::new(move |_: &Command| Reply {
            code: 1,
            stderr: stdout.to_owned(),
            ..Reply::default()
        });
        let dialect = if fteqcc {
            Dialect::Fteqcc
        } else {
            Dialect::Vanilla
        };
        let case = Case::new(dialect, "void() f = #5;\n".to_owned());
        assert_eq!(
            oracle(&fake_oracles(&dir, &fake), &dir, &case).unwrap(),
            outcome,
            "{stdout}"
        );
    }
}

// spec FTE-23.15: a fixture that is not JSON, or holds an outcome in no known form, is refused
#[test]
fn a_bad_fixture_is_refused() {
    let dir = fresh_dir("builtins-bad-fixture");
    assert!(
        read_fixture(&dir)
            .unwrap_err()
            .contains("builtin-outcomes.json")
    );
    let path = dir.join(FIXTURE);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "[").unwrap();
    assert!(read_fixture(&dir).is_err());
    fs::write(&path, "{\"fteqcc|x\": \"maybe\"}").unwrap();
    assert!(
        read_fixture(&dir)
            .unwrap_err()
            .contains("bad outcome `maybe`")
    );
}

// spec FTE-23.15: the tally names each statement an expected difference is recorded under
#[test]
fn the_tally_prints_each_count() {
    let mut tally = Tally {
        agree: 3,
        differ: 1,
        ..Tally::default()
    };
    tally.expected.insert("QC-27.9", 2);
    assert_eq!(
        tally.to_string(),
        "3 agree with the profile, 2 differ as QC-27.9 says, 1 differ"
    );
}
