// SPDX-License-Identifier: GPL-3.0-or-later

//! The project graph's reader of an FTEQCC file (PRJ-5.13, PRJ-5.14): what it meets, read as
//! `QCC_PR_Precompiler` reads the directives and `QCC_PR_Lex` the code between them.

use crate::{FileStart, FteEvent, FteFileReader, ModuleState};

/// A spec of one reading: the files read in turn with one module's state, each with how it
/// starts, and the module's defines. By default one compiled file and an FTEQCC module.
#[derive(Clone, Debug)]
struct ReadSpec<'a> {
    files: &'a [(&'a str, FileStart)],
    defines: &'a [(&'a str, &'a str)],
    fteqcc: bool,
}

impl Default for ReadSpec<'_> {
    fn default() -> Self {
        Self {
            files: &[],
            defines: &[],
            fteqcc: true,
        }
    }
}

/// Every event of each file of `spec` in turn, written `kind name @line`, and the state after.
fn read_all(spec: &ReadSpec<'_>) -> (Vec<String>, ModuleState) {
    let defines = spec
        .defines
        .iter()
        .map(|&(name, value)| (name.to_owned(), value.to_owned()));
    let mut state = if spec.fteqcc {
        ModuleState::fteqcc(defines)
    } else {
        ModuleState::new(defines)
    };
    let mut events = Vec::new();
    for &(text, start) in spec.files {
        let mut reader = FteFileReader::new(text.as_bytes().to_vec(), start);
        while let Some(event) = reader.next(&mut state) {
            events.push(show(&event));
        }
    }
    (events, state)
}

fn show(event: &FteEvent) -> String {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    match event {
        FteEvent::Include { name, line } => format!("include {} @{line}", text(name)),
        FteEvent::ListEntry { name, line } => format!("entry {} @{line}", text(name)),
        FteEvent::Output { name, line } => format!("output {} @{line}", text(name)),
        FteEvent::Pragma {
            name,
            text: t,
            line,
        } => {
            format!("pragma {} {} @{line}", text(name), text(t))
        }
        FteEvent::UnclosedList { line } => format!("unclosed @{line}"),
    }
}

/// The events of `text`, a compiled file of an FTEQCC module.
fn events(text: &str) -> Vec<String> {
    read_all(&ReadSpec {
        files: &[(text, FileStart::Compiled)],
        ..ReadSpec::default()
    })
    .0
}

/// The names `text` includes or lists.
fn brought(text: &str) -> Vec<String> {
    events(text)
        .into_iter()
        .filter(|e| e.starts_with("include ") || e.starts_with("entry "))
        .map(|e| e.split(' ').nth(1).unwrap_or_default().to_owned())
        .collect()
}

// spec PRJ-5.13: the reader gives each `#include`, list entry, `#output`, pragma and unclosed
// list with the line it is on
#[test]
fn the_reader_gives_what_the_graph_acts_on() {
    let text = "#include \"a.qh\"\n\n#includelist\n a.qc,b.qc\n#endlist\n#output \"p.dat\"\n\
                #pragma includedir(x)\n#pragma sourcefile c.src\n#includelist\nc.qc\n";
    assert_eq!(
        events(text),
        [
            "include a.qh @1",
            "entry a.qc @4",
            "entry b.qc @4",
            "output p.dat @6",
            "pragma includedir (x) @7",
            "pragma sourcefile c.src @8",
            "entry c.qc @10",
            "unclosed @9",
        ]
    );
    assert_eq!(
        events("#include <a.qh>\n#include \"\"\n#output\n"),
        ["include a.qh @1"]
    );
    assert!(events("").is_empty());
}

// spec PRJ-5.13: a directive is at a compiled file's start after spaces and tabs, at an included
// file's first byte only, and at each line's start that the white space reaches
#[test]
fn directives_stand_where_qcc_pr_newline_reaches() {
    let read = |text: &str, start| {
        read_all(&ReadSpec {
            files: &[(text, start)],
            ..ReadSpec::default()
        })
        .0
    };
    let indented = "  #include \"a.qh\"\n";
    assert_eq!(read(indented, FileStart::Compiled), ["include a.qh @1"]);
    assert!(read(indented, FileStart::Included).is_empty());
    assert_eq!(
        read("#include \"a.qh\"", FileStart::Included),
        ["include a.qh @1"]
    );
    assert_eq!(
        read("\n  \t#include \"a.qh\"", FileStart::Included),
        ["include a.qh @2"]
    );
    for none in [
        "/* c */ #include \"a.qh\"\n",
        "/* c\n*/ #include \"a.qh\"\n",
        "x; #include \"a.qh\"\n",
        "/* c\n#include \"a.qh\"\n*/\n",
    ] {
        assert!(brought(none).is_empty(), "{none:?}");
    }
    assert_eq!(brought("// c\n#include \"a.qh\"\n"), ["a.qh"]);
    assert_eq!(brought("\u{feff}#include \"a.qh\"\n"), ["a.qh"]);
}

// spec PRJ-5.13: names are matched by their first bytes, and each directive's line ends where
// `QCC_PR_SkipToEndOfLine` ends it
#[test]
fn names_and_line_ends_are_fteqccs() {
    let (_, state) = read_all(&ReadSpec {
        files: &[(
            "#defined N 1\n#appendix N 2\n#undefM\n",
            FileStart::Compiled,
        )],
        defines: &[("M", "")],
        ..ReadSpec::default()
    });
    assert_eq!(state.value("N"), Some("1 2"));
    assert!(!state.is_defined("M"));
    assert_eq!(brought("#includes \"a.qh\"\n#include \"b.qh\"\n"), ["b.qh"]);
    for hidden in [
        "#undef N // c \\\n#include \"a.qh\"\n",
        "#define N 1 // c \\\n#include \"a.qh\"\n",
        "#undef N \\\n#include \"a.qh\"\n",
        "#ifndef N // c \\\n#include \"a.qh\"\n#endif\n",
    ] {
        assert!(brought(hidden).is_empty(), "{hidden:?}");
    }
    for shown in [
        "#undef N /* c\n */\n#include \"a.qh\"\n",
        "#pragma once // c \\\n#include \"a.qh\"\n",
        "#define N 1 // c\n#include \"a.qh\"\n",
        "#foo\n#include \"a.qh\"\n",
        "#\n#include \"a.qh\"\n",
    ] {
        assert_eq!(brought(shown), ["a.qh"], "{shown:?}");
    }
}

// spec PRJ-5.13: an `#append` adds to a macro of the same parameters, and replaces one of others
#[test]
fn appends_follow_the_parameters() {
    let value = |text: &str| {
        let (_, state) = read_all(&ReadSpec {
            files: &[(text, FileStart::Compiled)],
            ..ReadSpec::default()
        });
        state.value("A").map(str::to_owned)
    };
    assert_eq!(value("#append A 2\n").as_deref(), Some("2"));
    assert_eq!(value("#define A 1\n#append A 2 \n").as_deref(), Some("1 2"));
    assert_eq!(value("#define A(x) 1\n#append A 2\n").as_deref(), Some("2"));
    assert_eq!(
        value("#define A(x) 1\n#append A(x) 2\n").as_deref(),
        Some("1 2")
    );
    assert_eq!(value("#define A 1\n#define A 3\n").as_deref(), Some("3"));
    let long = format!("#define {} 1\n", "n".repeat(64));
    let (_, state) = read_all(&ReadSpec {
        files: &[(&long, FileStart::Compiled)],
        ..ReadSpec::default()
    });
    assert!(!state.is_defined(&"n".repeat(64)));
}

// spec PRJ-5.13: `#if` and `#elif` read as `ParsePrecompilerIf` reads them, and the sides that
// are not taken are skipped by `QCC_PR_FalsePreProcessorIf`
#[test]
fn conditionals_are_fteqccs() {
    for (text, expected) in [
        ("#if 0 && 0 || 1\n#include \"a\"\n#endif\n", &[][..]),
        ("#if 1||0\n#include \"a\"\n#endif\n", &["a"][..]),
        ("#if (1\n#include \"a\"\n#endif\n", &[]),
        ("#define V 3\n#if V == 3\n#include \"a\"\n#endif\n", &["a"]),
        (
            "#if 0\n#elif 1\n#include \"a\"\n#else\n#include \"b\"\n#endif\n",
            &["a"],
        ),
        (
            "#if 0\n#elif (1\n#include \"a\"\n#else\n#include \"b\"\n#endif\n",
            &["b"],
        ),
        (
            "#if 1\n#include \"a\"\n#elif 1\n#include \"b\"\n#endif\n",
            &["a"],
        ),
        (
            "#ifdef Q\n#elifdef Q\n#include \"a\"\n#elifndef Q\n#include \"b\"\n#endif\n",
            &["b"],
        ),
        (
            "#define Q\n#ifndef Q\n#elifdef Q\n#include \"a\"\n#endif\n",
            &["a"],
        ),
        (
            "#ifdef Q\n#elifdefQ\n#include \"a\"\n#else\n#include \"b\"\n#endif\n",
            &["b"],
        ),
        (
            "#ifdef Q\n// c \\\n#endif\n#endif\n#include \"a\"\n",
            &["a"],
        ),
        (
            "#ifdef Q\n#ifdef R\n#else\n#include \"a\"\n#endif\n#else\n#include \"b\"\n#endif\n",
            &["b"],
        ),
        (
            "#ifdef Q\n#else\n#include \"a\"\n#else\n#include \"b\"\n#endif\n",
            &["a"],
        ),
        (
            "#if 1\n#else\n#include \"a\"\n#endif\n#include \"b\"\n",
            &["b"],
        ),
        ("#ifdefQ\n#include \"a\"\n", &["a"]),
        ("#if(0)\n#include \"a\"\n", &["a"]),
        ("#ifdef\n#include \"a\"\n#endif\n", &[]),
        ("#ifndef\n#include \"a\"\n#endif\n", &["a"]),
        (
            "#else\n#include \"a\"\n#endif\n#include \"b\"\n",
            &["a", "b"],
        ),
        ("#ifdef Q\n#include \"a\"\n", &[]),
        (
            "#ifdef FTEQCC\n#include \"a\"\n#endif\n#if FTEQCC\n#include \"b\"\n#endif\n",
            &["a"],
        ),
    ] {
        assert_eq!(brought(text), expected, "{text:?}");
    }
    let vanilla = read_all(&ReadSpec {
        files: &[(
            "#ifdef FTEQCC\n#include \"a\"\n#endif\n",
            FileStart::Compiled,
        )],
        fteqcc: false,
        ..ReadSpec::default()
    });
    assert!(vanilla.0.is_empty());
}

// spec PRJ-5.13: one count of open sides runs from file to file, so a side one file leaves open
// is ended by an `#else` or closed by an `#endif` in the next
#[test]
fn open_sides_run_from_file_to_file() {
    let read = |files: &[(&str, FileStart)]| {
        read_all(&ReadSpec {
            files,
            ..ReadSpec::default()
        })
        .0
    };
    let opened = ("#if 1\n", FileStart::Included);
    let ended = (
        "#else\n#include \"a\"\n#endif\n#include \"b\"\n",
        FileStart::Compiled,
    );
    assert_eq!(read(&[opened, ended]), ["include b @4"]);
    assert_eq!(read(&[ended]), ["include a @2", "include b @4"]);
}

// spec PRJ-5.13: `#eof` and `#package` end the file; `DONT_COMPILE_THIS_FILE` passes over the
// rest with its directives still run
#[test]
fn the_text_ends_where_fteqcc_ends_it() {
    assert!(brought("#eof\n#include \"a\"\n").is_empty());
    assert!(brought("#eofx\n#include \"a\"\n").is_empty());
    assert!(brought("#package\n#include \"a\"\n").is_empty());
    assert!(brought("#includelist\n#eof\na.qc\n#endlist\n").is_empty());
    assert_eq!(events("#includelist\n#eof\n"), ["unclosed @1"]);
    let (_, state) = read_all(&ReadSpec {
        files: &[(
            "#pragma DONT_COMPILE_THIS_FILE\nzz \"#define Q\n#define N 1\n",
            FileStart::Compiled,
        )],
        ..ReadSpec::default()
    });
    assert!(state.is_defined("N"));
    assert!(!state.is_defined("Q"));
    assert_eq!(
        brought("#pragma DONT_COMPILE_THIS_FILE\n#includelist\na.qc\n#endlist\n"),
        ["a.qc"]
    );
    assert_eq!(
        events("#includelist\n#pragma DONT_COMPILE_THIS_FILE\na.qc\n#endlist\n"),
        ["unclosed @1"]
    );
}

// spec PRJ-5.14: an `#include` in the text `DONT_COMPILE_THIS_FILE` passes over brings in no file
#[test]
fn an_include_that_is_not_compiled_brings_nothing_in() {
    assert!(brought("#pragma DONT_COMPILE_THIS_FILE\n#include \"a\"\n").is_empty());
    assert_eq!(
        brought("#pragma DONT_COMPILE_THIS_FILE\n#includelist\n#include \"a\"\n#endlist\n"),
        ["a"]
    );
}

// spec PRJ-5.13: an `#includelist`'s words are `QCC_PR_SimpleGetToken`'s, after white space and
// comments, with the directives on its lines run, `#endlist` ending the list wherever it stands
#[test]
fn list_words_are_fteqccs() {
    for (text, expected) in [
        (
            "#includelist\n#ifdef Q\nx.qc\n#endif\nl.qc\n#endlist\n",
            &["l.qc"][..],
        ),
        ("#includelist\n// c\nl.qc // x.qc\n#endlist\n", &["l.qc"]),
        ("#includelist\n/* x.qc */ l.qc\n#endlist\n", &["l.qc"]),
        ("#includelist\n\"l.qc\"\n#endlist\n", &["\"l.qc\""]),
        (
            "#includelist\n  l.qc  #endlist\n#include \"a\"\n",
            &["l.qc", "a"],
        ),
        ("#includelist // c \\\nx.qc\nl.qc\n#endlist\n", &["l.qc"]),
        (
            "#includelist\r\na.qc\r\nb.qc;(c.qc)]\r\n#endlist\r\n",
            &["a.qc", "b.qc", "c.qc"],
        ),
        ("#includelist\n#include \"a\"\n#endlist\n", &["a"]),
        (
            "#includelist\n#includelist\na\n#endlist\nb\n#endlist\n",
            &["a", "b"],
        ),
        ("#endlist\n#include \"a\"\n", &["a"]),
    ] {
        assert_eq!(brought(text), expected, "{text:?}");
    }
}

// spec PRJ-5.14: an `#includelist` met while 64 others are open ends the file
#[test]
fn nested_lists_are_bounded() {
    let text = format!("{}a.qc\n", "#includelist\n".repeat(65));
    let found = events(&text);
    assert_eq!(found.len(), 64, "{found:?}");
    assert!(found.iter().all(|e| e.starts_with("unclosed")));
    assert_eq!(found[0], "unclosed @64");
    let text = format!("{}a.qc\n", "#includelist\n".repeat(64));
    assert_eq!(events(&text)[0], "entry a.qc @65");
}

// spec PRJ-5.13: code is passed over a token at a time, as `QCC_PR_Lex` reads it, so no
// string, character constant or comment starts a directive or a comment, and a `\` before a line
// break hides the next line's directive
#[test]
fn code_is_passed_over_as_qcc_pr_lex_reads_it() {
    for (code, read) in [
        ("string s = \"// \\\" #\";\n", true),
        ("string s = \"/*\";\n", true),
        ("string s = R\"x(\n#include \"b\"\n)x\";\n", true),
        ("string s = R\"x(\n)y\"\n)x\";\n", true),
        ("string s = Q\"/*\";\n", true),
        ("string s = u8\"/*\";\n", true),
        ("float c = '\"';\n", true),
        ("float c = '\\'';\n", true),
        ("float c = '\\\"';\n", true),
        ("float c = '\u{e9}';\n", true),
        ("vector v = '1 2 3';\n", true),
        ("float g; \\\n", false),
        ("float g; \\\r\n", false),
        ("float g = 1.5e3;\n", true),
        ("float g = .5;\n", true),
        ("string s = \"a\n", true),
        ("$frame a b\n", true),
    ] {
        let text = format!("{code}#include \"a\"\n");
        let expected: &[&str] = if read { &["a"] } else { &[] };
        assert_eq!(brought(&text), expected, "{code:?}");
    }
    assert!(brought("string s = R\"x(\n#include \"a\"\n").is_empty());
    assert!(brought("string s = R\"x\n#include \"a\"\n").is_empty());
}
