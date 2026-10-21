// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's directives as `QCC_PR_Precompiler` reads them (FTEF-14.33 to FTEF-14.47): the reading
//! in `fte_directive` and the preprocessor that runs it, wherever FTEQCC meets a directive.

use std::path::{Path, PathBuf};

use super::{codes, one_fte, tokens};
use crate::fte_directive::{
    Definer, Directive, Earlier, ElifTest, FteState, Gap, IfTest, Junk, LineEnd, Pragma, Side,
    TextStart, com_parse, false_side, false_side_after, include_candidates, join_paths, read,
    simple_get_token, skip_to_end_of_line, starts_line, whitespace,
};
use crate::{
    Code, FileOutput, FteWarning, MemoryLoader, Preprocessor, WarningAction, WarningSettings,
};

fn fte(text: &str) -> (String, Vec<Code>) {
    let (_, out) = one_fte(text);
    (tokens(&out), codes(&out))
}

/// Compiles `compiled` under FTEQCC with `files` beside it.
fn compile(files: &[(&str, &str)], compiled: &str) -> FileOutput {
    compile_in(files, compiled, qc_lexer::Dialect::Fteqcc)
}

/// Compiles `compiled` under `dialect` with `files` beside it.
fn compile_in(files: &[(&str, &str)], compiled: &str, dialect: qc_lexer::Dialect) -> FileOutput {
    let mut loader = MemoryLoader::new();
    for (path, text) in files {
        loader.insert(*path, text.as_bytes());
    }
    let mut pp = Preprocessor::new(loader).with_dialect(dialect);
    pp.preprocess_file(Path::new(compiled)).unwrap()
}

fn end(at: usize, junk: bool) -> LineEnd {
    LineEnd { at, junk }
}

// spec FTEF-14.52: a `#` is read as a directive after a line ending (a LF, or a CR not before a
// LF) and spaces and tabs, at a compiled file's start after spaces and tabs, at an included
// file's first byte only, and never at a chunk's start or after another byte on its line
#[test]
fn directives_start_where_new_line_reads() {
    let at = |text: &str| text.find('#').unwrap();
    for (text, start, want) in [
        ("#x", TextStart::Compiled, true),
        ("  \t#x", TextStart::Compiled, true),
        ("  #x", TextStart::Included, false),
        ("#x", TextStart::Included, true),
        ("#x", TextStart::Chunk, false),
        ("a\n  #x", TextStart::Chunk, true),
        ("a\r#x", TextStart::Chunk, true),
        ("a\r\n#x", TextStart::Included, true),
        ("a #x", TextStart::Compiled, false),
        ("/* */#x", TextStart::Compiled, false),
        ("a\n\x0b#x", TextStart::Compiled, false),
    ] {
        assert_eq!(
            starts_line(text.as_bytes(), 0, at(text), start),
            want,
            "{text:?}"
        );
    }
}

// spec FTEF-14.53: in code a `\` before a line break joins the lines with no `QCC_PR_NewLine`,
// so a `#` after them and spaces and tabs starts no directive; one in a `//` comment, or a line
// ending the white-space reader passes after it, leaves the `#` a directive's
#[test]
fn a_joined_line_starts_no_directive() {
    for (text, from, start, want) in [
        ("a \\\n#x", 1, TextStart::Compiled, false),
        ("a \\\r\n#x", 1, TextStart::Compiled, false),
        ("a \\\r#x", 1, TextStart::Included, false),
        ("a \\\n \t#x", 1, TextStart::Chunk, false),
        ("a /* c */ \\\n#x", 1, TextStart::Compiled, false),
        ("a \\\n \\\r\n#x", 1, TextStart::Compiled, false),
        ("\\\n#x", 0, TextStart::Compiled, false),
        ("a \\\n\n#x", 1, TextStart::Compiled, true),
        ("a \\\n\r#x", 1, TextStart::Compiled, true),
        ("a // c \\\n#x", 1, TextStart::Compiled, true),
        ("a \\\n// c\n#x", 1, TextStart::Compiled, true),
        ("a \\ \n#x", 1, TextStart::Compiled, true),
        // Reading went on at the line break, after a directive whose text held the `\`.
        ("#pragma p \\\n#x", 11, TextStart::Compiled, true),
        ("a\n#x", 1, TextStart::Compiled, true),
        // Reading went on after the line break before the `#`, so nothing read joins them.
        ("a\n#x", 2, TextStart::Compiled, true),
        ("a\n \t#x", 3, TextStart::Compiled, true),
        ("a \\\n#x", 4, TextStart::Compiled, true),
    ] {
        let hash = text.rfind('#').unwrap();
        assert_eq!(
            starts_line(text.as_bytes(), from, hash, start),
            want,
            "{text:?}"
        );
    }
}

// spec FTEF-14.45: `QCC_PR_LexWhitespace(false)` passes white space and comments and stops at
// a `#` that starts a line, after a line ending in white space or a `//` comment's
#[test]
fn white_space_stops_at_a_directive() {
    let gap = |text: &str| whitespace(text.as_bytes(), 0);
    assert_eq!(gap("  x"), Gap::Text(2));
    assert_eq!(gap(" \n  #x"), Gap::LineStart(4));
    assert_eq!(gap("// c\n#x"), Gap::LineStart(5));
    assert_eq!(gap("// c\r#x"), Gap::LineStart(5));
    assert_eq!(gap("\r\n\t#x"), Gap::LineStart(3));
    assert_eq!(gap("/* \n */#x"), Gap::Text(7));
    assert_eq!(gap("\n x"), Gap::Text(2));
    assert_eq!(gap("// c"), Gap::Text(4));
    // FTEF-14.67: FTEQCC reports "EOF inside comment" there.
    assert_eq!(gap("/* c"), Gap::Unclosed(0));
    assert_eq!(gap(""), Gap::Text(0));
    assert_eq!(gap("/x"), Gap::Text(0));
    assert_eq!(gap("\n\n#"), Gap::LineStart(2));
}

// spec FTEF-14.35: `QCC_PR_SkipToEndOfLine` reads to the line break past `/* */` comments,
// strings before a `//` and `\` line breaks, in a `//` comment too, and with `errorifnonwhite`
// finds the junk outside comments, a string included
#[test]
fn lines_end_where_skip_to_end_of_line_ends_them() {
    let skip = |text: &str, check: bool| {
        let junk = if check { Junk::Checked } else { Junk::Ignored };
        skip_to_end_of_line(text.as_bytes(), 0, junk)
    };
    assert_eq!(skip(" // c \\\nzz\nx", true), end(10, false));
    assert_eq!(skip(" // c \\\r\nzz\nx", true), end(11, false));
    assert_eq!(skip(" /* c\n */\nx", true), end(9, false));
    assert_eq!(skip(" /* c", true), end(5, false));
    assert_eq!(skip(" \\\nzz\n", true), end(5, true));
    assert_eq!(skip(" \"s\\\"//\" x\n", true), end(10, true));
    assert_eq!(skip(" \"s\\\"//\"\n", false), end(8, false));
    assert_eq!(skip(" \"s\\\\\" x\n", false), end(8, false));
    assert_eq!(skip(" \"s\n", false), end(3, false));
    assert_eq!(skip(" \"s", false), end(3, false));
    assert_eq!(skip(" zz\n", true), end(3, true));
    assert_eq!(skip(" \"s\"\n", true), end(4, true));
    assert_eq!(skip(" zz\n", false), end(3, false));
    assert_eq!(skip(" \t\r\x0b\n", true), end(4, false));
    assert_eq!(skip(" // \"s\" zz\n", true), end(10, false));
    assert_eq!(skip("", true), end(0, false));
    assert_eq!(skip("a\0b\n", true), end(1, true));
}

// spec FTEF-14.57: `QCC_PR_SimpleGetToken` reads a word after white space on the line, up to
// white space, `,`, `;`, `(`, `)`, `]` or `//`, and none at a line break, a comment or the end
#[test]
fn simple_tokens() {
    let token = |text: &str| {
        let (word, stop) = simple_get_token(text.as_bytes(), 0);
        (word.map(|w| text.get(w).unwrap().to_owned()), stop)
    };
    assert_eq!(token("  ab cd"), (Some("ab".to_owned()), 4));
    for stop in [",", ";", "(", ")", "]", "//", " ", "\t"] {
        assert_eq!(
            token(&format!("a.b{stop}x")).0.as_deref(),
            Some("a.b"),
            "{stop:?}"
        );
    }
    assert_eq!(token("a/b"), (Some("a/b".to_owned()), 3));
    assert_eq!(token("  \nab"), (None, 2));
    assert_eq!(token(" // c\nab"), (None, 5));
    assert_eq!(token(" /* c */ab"), (None, 1));
    assert_eq!(token(" (a"), (None, 1));
    assert_eq!(token("  "), (None, 2));
}

// spec FTEF-14.70: `QCC_COM_Parse` reads a string with its escapes, a punctuation byte or a word,
// after white space and comments
#[test]
fn com_parse_reads_tokens() {
    let parse = |text: &str| {
        com_parse(text.as_bytes(), 0).map(|(t, at)| (String::from_utf8(t).unwrap(), at))
    };
    assert_eq!(parse("  ab cd"), Some(("ab".to_owned(), 4)));
    assert_eq!(parse(" // c\n /* d */ ab"), Some(("ab".to_owned(), 17)));
    assert_eq!(
        parse(" \"a\\\"b\\\\c\\nd\\re\\tf\" x"),
        Some(("a\"b\\c\nd\re\tf".to_owned(), 19))
    );
    assert_eq!(parse("\"ab"), Some(("ab".to_owned(), 3)));
    assert_eq!(parse("\"a\\\"\nb\""), Some(("a\"".to_owned(), 5)));
    assert_eq!(parse("\"a\nb\""), Some(("a\nb".to_owned(), 5)));
    for single in ["{", "}", "(", ")", "'", ":", ","] {
        assert_eq!(parse(&format!("{single}x")), Some((single.to_owned(), 1)));
    }
    assert_eq!(parse("ab(c"), Some(("ab".to_owned(), 2)));
    assert_eq!(parse("ab\"c"), Some(("ab".to_owned(), 2)));
    assert_eq!(parse("ab"), Some(("ab".to_owned(), 2)));
    assert_eq!(parse("  "), None);
    assert_eq!(parse("/* c"), None);
}

// spec FTEF-14.52: a directive's name is matched by its first bytes, in FTEQCC's order, after
// every byte up to a space but a line break; a `#` before a line break hangs; no match is no
// directive
#[test]
fn directive_names_match_by_their_first_bytes() {
    let directive = |text: &str| read(text.as_bytes(), 0);
    assert!(matches!(
        directive("#defined N 5"),
        Some(Directive::Define {
            definer: Definer::Define,
            word_end: 8
        })
    ));
    assert!(matches!(
        directive("# \t\x0b\x01\u{80}append N"),
        Some(Directive::Define {
            definer: Definer::Append,
            ..
        })
    ));
    assert!(matches!(
        directive("#undefine N"),
        Some(Directive::Undef { .. })
    ));
    assert!(matches!(
        directive("#iffy"),
        Some(Directive::NotIf { word }) if word == (1..5)
    ));
    assert!(matches!(
        directive("#elsewhere"),
        Some(Directive::Else {
            end: LineEnd { junk: true, .. }
        })
    ));
    assert!(matches!(
        directive("#elif 1"),
        Some(Directive::Else {
            end: LineEnd { junk: false, .. }
        })
    ));
    assert!(matches!(
        directive("#endiff"),
        Some(Directive::Endif { .. })
    ));
    assert!(matches!(directive("#eofx"), Some(Directive::Eof)));
    assert!(matches!(
        directive("#errors"),
        Some(Directive::Error { .. })
    ));
    assert!(matches!(
        directive("#warnings"),
        Some(Directive::Warning { .. })
    ));
    for build in [
        "#messages",
        "#copyrights",
        "#forcecrcx",
        "#merge",
        "#datafile",
    ] {
        assert!(
            matches!(directive(build), Some(Directive::Build { .. })),
            "{build}"
        );
    }
    assert!(matches!(
        directive("#output \x0b\"p\\\\\\n.dat\""),
        Some(Directive::Output { name: Some(name), .. }) if name == b"p\\\n.dat"
    ));
    assert!(matches!(
        directive("#output"),
        Some(Directive::Output { name: None, .. })
    ));
    assert!(matches!(
        directive("#packages"),
        Some(Directive::Package { .. })
    ));
    assert!(matches!(directive("#packx"), Some(Directive::Pack { .. })));
    assert!(matches!(
        directive("#includelists"),
        Some(Directive::IncludeList { .. })
    ));
    assert!(matches!(
        directive("#includes"),
        Some(Directive::Include { name: Err(_), .. })
    ));
    assert!(matches!(
        directive("#pragmas"),
        Some(Directive::Pragma { .. })
    ));
    assert_eq!(directive("# \n"), Some(Directive::Hanging(end(2, false))));
    assert_eq!(directive("#\r\n"), Some(Directive::Hanging(end(2, false))));
    for none in ["#foo", "#endlist", "#5", "#", "# ", "#PRAGMA x"] {
        assert_eq!(directive(none), None, "{none:?}");
    }
}

// spec FTEF-14.57: `#undef`, `#ifdef` and `#ifndef` read a name after their own; a byte that is
// no white space after an `#if`'s name makes it no conditional
#[test]
fn operands_of_undef_and_ifdef() {
    let directive = |text: &str| read(text.as_bytes(), 0).unwrap();
    assert_eq!(
        directive("#undef  X Y\n"),
        Directive::Undef {
            name: 8..9,
            end: end(11, true)
        }
    );
    assert_eq!(
        directive("#undef(X)\n"),
        Directive::Undef {
            name: 6..6,
            end: end(9, true)
        }
    );
    assert_eq!(
        directive("#ifdef X // c\n"),
        Directive::If {
            test: IfTest::Defined,
            name: 7..8,
            end: end(13, false)
        }
    );
    assert_eq!(
        directive("#ifndef\n"),
        Directive::If {
            test: IfTest::NotDefined,
            name: 7..7,
            end: end(7, false)
        }
    );
    assert!(matches!(
        directive("#if 1"),
        Directive::If {
            test: IfTest::Expression(3),
            ..
        }
    ));
    assert_eq!(directive("#if(1)"), Directive::NotIf { word: 1..3 });
    assert_eq!(directive("#ifdefX"), Directive::NotIf { word: 1..7 });
    assert_eq!(directive("#if"), Directive::NotIf { word: 1..3 });
}

// spec FTEF-14.37: `QCC_PR_FalsePreProcessorIf` nests on `if` and `endif` prefixes, takes an
// `#else` or `#elif` at its level only while no side was taken, needs a blank after `elif`, and
// at the end of the text reports none
#[test]
fn skipped_sides_end_where_false_preprocessor_if_ends_them() {
    let side = |text: &str, had_true: bool| {
        let mut level = 1;
        let earlier = if had_true {
            Earlier::SideTaken
        } else {
            Earlier::NoSideTaken
        };
        (false_side(text.as_bytes(), 0, earlier, &mut level), level)
    };
    assert_eq!(side("\nx\n#endif\ny", false), (Side::Endif(9), 0));
    assert_eq!(side("\n  #  endif x\n", false), (Side::Endif(13), 0));
    assert_eq!(
        side("\n#iffy\n#endif\n#endif\n", false),
        (Side::Endif(20), 0)
    );
    assert_eq!(side("\n#endifx\n", false), (Side::Endif(8), 0));
    assert_eq!(
        side("\n#else x\n", false),
        (
            Side::Else {
                hash: 1,
                end: end(8, true)
            },
            1
        )
    );
    assert_eq!(side("\n#else\n#endif\n", true), (Side::Endif(13), 0));
    assert_eq!(
        side("\n#elifdef  X\n", false),
        (
            Side::Elif {
                hash: 1,
                test: ElifTest::Defined,
                at: 9,
                malformed: false
            },
            1
        )
    );
    assert_eq!(
        side("\n#elifndefX\n", false),
        (
            Side::Elif {
                hash: 1,
                test: ElifTest::NotDefined,
                at: 10,
                malformed: true
            },
            1
        )
    );
    assert_eq!(
        side("\n#elif\n", false),
        (
            Side::Elif {
                hash: 1,
                test: ElifTest::Expression,
                at: 6,
                malformed: true
            },
            1
        )
    );
    assert_eq!(side("\n#if 1\n#else\n#elif 1\n", false), (Side::Eof, 2));
    assert_eq!(side("\n// c \\\n#endif\n", false), (Side::Eof, 1));
    assert_eq!(side("\n\"#endif\n", false), (Side::Eof, 1));
    assert_eq!(side("", false), (Side::Eof, 1));
    assert_eq!(side("\nx", false), (Side::Eof, 1));
    let mut level = 1;
    let text = b"0\nzz\n#endif\n";
    assert_eq!(
        false_side_after(text, 1, Earlier::NoSideTaken, &mut level),
        Side::Endif(11)
    );
    let mut level = 1;
    assert_eq!(
        false_side_after(b"0", 1, Earlier::NoSideTaken, &mut level),
        Side::Eof
    );
}

// spec FTEF-14.40: `QCC_JoinPaths` and `QCC_FindBestInclude` try the name beside the file being
// compiled and then in each include directory
#[test]
fn includes_are_found_beside_the_compiled_file() {
    assert_eq!(join_paths(b"c.qh", b"src/x/a.qc"), b"src/x/c.qh");
    assert_eq!(join_paths(b"c.qh", b"src\\a.qc"), b"src\\c.qh");
    assert_eq!(join_paths(b"\\c.qh", b"src/a.qc"), b"\\c.qh");
    let dirs = [b"inc/".to_vec(), b"more/".to_vec()];
    assert_eq!(
        include_candidates(b"c.qh", b"src/a.qc", &dirs),
        [
            b"src/c.qh".to_vec(),
            b"inc/c.qh".to_vec(),
            b"more/c.qh".to_vec()
        ]
    );
}

// spec FTEF-14.70: `#pragma includedir` adds its first token with a `/`, joined to the compiled
// file, at most 8, each once, an empty one as `.`
#[test]
fn include_directories() {
    let dir = |message: &[u8]| pragma(message).include_dir(b"src/a.qc");
    assert_eq!(dir(b"x"), b"src/x/");
    assert_eq!(dir(b"\"x/y\\\\\" // c"), b"src/x/y\\");
    assert_eq!(dir(b"\"x/y\\\" // c"), b"src/x/y\" // c/");
    assert_eq!(dir(b""), b"src/");
    let mut state = FteState::default();
    state.add_include_dir(b"a/");
    state.add_include_dir(b"a/");
    state.add_include_dir(b"b");
    state.add_include_dir(b"");
    assert_eq!(
        state.include_dirs,
        [b"a/".to_vec(), b"b/".to_vec(), b"./".to_vec()]
    );
    for i in 0..10 {
        state.add_include_dir(format!("d{i}/").as_bytes());
    }
    assert_eq!(state.include_dirs.len(), 8);
}

/// A pragma with the text `message`.
fn pragma(message: &[u8]) -> Pragma {
    Pragma {
        name: b"p".to_vec(),
        message: message.to_vec(),
    }
}

// spec FTEF-14.32: `#pragma file` names the file its text names, or within the `(` it starts
// with and the first `)` after it
#[test]
fn pragma_file_names() {
    for (text, name) in [
        (&b"(a)b"[..], &b"a"[..]),
        (b"( a ", b" a "),
        (b"a)", b"a)"),
        (b"", b""),
    ] {
        assert_eq!(pragma(text).file_name(), name, "{text:?}");
    }
}

// spec FTEF-14.43: `#pragma line` reads its number with `strtoul` in base 0, keeps the low 32
// bits, and numbers the next line with it
#[test]
fn pragma_line_reads_strtoul() {
    for (text, line) in [
        ("10", 10),
        ("(10)", 10),
        ("( 10 )", 10),
        ("0x10", 16),
        ("0X1f", 31),
        ("0x", 0),
        ("010", 8),
        ("08", 0),
        ("+7", 7),
        ("-1", -1),
        ("", 0),
        ("x", 0),
        ("4294967297", 1),
        ("2147483648", i32::MIN),
        ("99999999999999999999", -1),
        ("-99999999999999999999", -1),
        ("\t\x0c 12zz", 12),
    ] {
        assert_eq!(pragma(text.as_bytes()).line(), line, "{text:?}");
    }
}

// spec FTEF-14.70: `#pragma warning STATE NAME...` sets F303 as each state says, `toggle` a
// warning for an error, and leaves it for another name or state
// spec FTEF-14.63: the names are matched in any case, and each changes the setting of its own
// warning
#[test]
fn pragma_warning_sets_f303() {
    let after = |from: WarningAction, text: &str| {
        let state = match from {
            WarningAction::Ignore => "disable F303",
            WarningAction::Warn => "enable F303",
            WarningAction::Error => "error F303",
        };
        let (settings, _) = WarningSettings::default().after_pragma(state.as_bytes());
        let (settings, _) = settings.after_pragma(text.as_bytes());
        settings.action(FteWarning::EvilPreprocessor)
    };
    use WarningAction::{Error, Ignore, Warn};
    for (from, text, want) in [
        (Error, "disable F303", Ignore),
        (Error, "off f303", Ignore),
        (Error, "IGNORE F303", Ignore),
        (Ignore, "enable F303", Warn),
        (Ignore, "on F303", Warn),
        (Warn, "error F303", Error),
        (Error, "toggle F303", Warn),
        (Warn, "toggle F303", Warn),
        (Ignore, "toggle F303", Ignore),
        (Error, "disable F302 F303", Ignore),
        (Error, "disable F302", Error),
        (Error, "disabled F303", Error),
        (Error, "", Error),
    ] {
        assert_eq!(after(from, text), want, "{from:?} {text:?}");
    }
}

// spec FTEF-14.35: every directive's line ends where `QCC_PR_SkipToEndOfLine` ends it, past a `\`
// and line break in a `//` comment, and its junk is E0121
// spec QCD-8.14: E0121 is FTEQCC's preprocessor error with FTEQCC's text
#[test]
fn every_directive_ends_its_line_as_fteqcc_does() {
    for directive in [
        "#define X 1",
        "#undef X",
        "#ifdef X\n#endif",
        "#ifndef X\n#endif",
        "#if 1\n#endif",
        "#forcecrc 1",
        "#message x",
        "#copyright x",
    ] {
        let source = directive.replacen('\n', " // c \\\nzz zz\n", 1);
        let first = format!("{source} // c \\\nzz zz\ny\n");
        assert_eq!(fte(&first).0, "y", "{first:?}");
    }
    let (toks, errs) = fte("#define X\n#ifdef X Y\ny\n#endif X\n#endif // c \\\nzz\n");
    let errors = vec![Code::E0121, Code::E0121, Code::E0109];
    assert_eq!((toks.as_str(), errs), ("y", errors));
    let (_, out) = one_fte("#define X\n#undef X Y\n");
    assert_eq!(
        out.diagnostics[0].message,
        "unexpected tokens at end of line"
    );
    assert_eq!(
        fte("#include \"\" x\ny\n"),
        ("y".to_owned(), vec![Code::E0121])
    );
    assert_eq!(
        fte("#include \"\" // c \\\nzz\ny\n"),
        ("y".to_owned(), vec![])
    );
    // A `#pragma`'s line ends at its break, whatever its text holds.
    assert_eq!(fte("#pragma once // c \\\ny\n"), ("y".to_owned(), vec![]));
}

// spec FTEF-14.52: `#name` that matches no directive is read as code: a macro's use, or a `#`
// and a name the parser reads, first on a line or in the rest of one; `# ` before a line break
// hangs
#[test]
fn hashes_that_start_no_directive() {
    assert_eq!(fte("#define N 5\n#N\n"), ("5".to_owned(), vec![]));
    assert_eq!(fte("#foo\nx\n"), ("# foo x".to_owned(), vec![]));
    assert_eq!(fte("/* c */#undef N\n"), ("# undef N".to_owned(), vec![]));
    assert_eq!(fte("x #foo\n"), ("x # foo".to_owned(), vec![]));
    assert_eq!(fte("#5\n").1, vec![]);
    assert_eq!(fte("# \nx\n"), ("x".to_owned(), vec![Code::E0121]));
    let (_, out) = one_fte("#\nx\n");
    assert_eq!(out.diagnostics[0].message, "Hanging # with no directive");
    // `#  define` is a directive, a vertical tab too; after a `/* */` it is code.
    assert_eq!(fte("#  define N 5\n#\x0bdefine M 6\nN M\n").0, "5 6");
    assert_eq!(
        fte("/* */#define N 5\n"),
        ("# define N 5".to_owned(), vec![])
    );
}

// spec FTEF-14.57: `#undefine N` undefines `ine`; `#ifdef` alone is false and `#ifndef` alone true;
// `#if(1)`, `#ifdefX` and `#iffy` open no block and leave the rest of the line as code
#[test]
fn undef_and_ifdef_read_their_operands() {
    assert_eq!(fte("#define ine 1\n#undefine\nine\n").0, "ine");
    // FTEF-14.57: `ine` is no macro: "Precompiler constant ine was not defined".
    assert_eq!(
        fte("#define N 1\n#undefine N\nN\n"),
        ("1".to_owned(), vec![Code::W0114, Code::E0121])
    );
    assert_eq!(fte("#define N 1\n#undefN\nN\n").0, "N");
    assert_eq!(fte("#ifdef\nx\n#endif\ny\n").0, "y");
    assert_eq!(fte("#ifndef\nx\n#endif\n").0, "x");
    assert_eq!(fte("#if(1)\n").0, "( 1 )");
    assert_eq!(fte("#ifdefX y\n").0, "y");
    // FTEF-14.57: "Unknown pragma", Q120, for the `#if` form.
    assert_eq!(
        fte("#iffy\n#endif\n"),
        (String::new(), vec![Code::W0113, Code::E0109])
    );
    let (_, out) = one_fte("#iffy\n");
    assert_eq!(out.diagnostics[0].message, "Unknown pragma 'iffy'");
}

// spec FTEF-14.37: one count of open sides runs on from file to file; skipped sides nest by
// prefix, end their lines as `QCC_PR_SkipToEndOfLine` does, and take the first true `#elif` or
// the `#else`; a taken side's `#else` skips to the block's end
// spec QCD-8.16: E0109 and E0110 have FTEQCC's texts under FTEQCC
#[test]
fn conditionals_count_and_skip_as_fteqcc_does() {
    assert_eq!(fte("#ifdef Q\n#iffy\n#endif\nzz\n#endif\ny\n").0, "y");
    assert_eq!(fte("#ifdef Q\n// c \\\n#endif\nzz\n#endif\ny\n").0, "y");
    assert_eq!(fte("#ifdef Q\n#else\ny\n#else\nzz\n#endif\n").0, "y");
    assert_eq!(fte("#if 1\ny\n#else\nzz\n#else\nzz\n#endif\n").0, "y");
    assert_eq!(fte("#ifdef Q\n#elsewhere\ny\n#endif\n").0, "y");
    assert_eq!(
        fte("#ifdef Q\n#else zz\ny\n#endif\n"),
        ("y".to_owned(), vec![Code::E0121])
    );
    let (toks, errs) = fte("#ifdef Q\n#elif\nzz\n#endif\ny\n");
    assert_eq!((toks.as_str(), errs), ("y", vec![Code::E0121, Code::E0121]));
    assert_eq!(fte("#ifdef Q\n#elif 0\nzz\n#elif 1\ny\n#endif\n").0, "y");
    assert_eq!(
        fte("#ifdef Q\n#elif 1 zz\ny\n#endif\n"),
        ("y".to_owned(), vec![Code::E0121])
    );
    assert_eq!(
        fte("#ifdef Q\n#elif (1\nzz\n#endif\ny\n"),
        ("y".to_owned(), vec![Code::E0121])
    );
    assert_eq!(
        fte("#ifdef Q\n#elifdef Q\nzz\n#elifndef Q\ny\n#endif\n").0,
        "y"
    );
    assert_eq!(fte("#define Q\n#ifndef Q\n#elifdef Q\ny\n#endif\n").0, "y");
    assert_eq!(
        fte("#ifdef Q\n#elifndef Q x\ny\n#endif\n").1,
        vec![Code::E0121]
    );
    assert_eq!(
        fte("#if 1\ny\n#elif garbage(\nzz\n#endif\n"),
        ("y".to_owned(), vec![])
    );
    assert_eq!(
        fte("#if 1\ny\n#elif 1\n"),
        ("y".to_owned(), vec![Code::E0110])
    );
    for (text, code, message) in [
        ("#if 0\nzz\n", Code::E0110, "#if with no endif"),
        ("#endif\n", Code::E0109, "unmatched #endif"),
        ("#else\n", Code::E0109, "#else outside of #if"),
        ("#elif 1\n", Code::E0109, "#else outside of #if"),
    ] {
        let (_, out) = one_fte(text);
        assert_eq!(codes(&out), vec![code], "{text:?}");
        assert_eq!(out.diagnostics[0].message, message, "{text:?}");
    }
    let out = compile(
        &[
            ("a.qc", "#if 1\n#include \"b.qh\"\ny\n"),
            ("b.qh", "#endif\n"),
        ],
        "a.qc",
    );
    assert_eq!((tokens(&out), codes(&out)), ("y".to_owned(), vec![]));
}

// spec FTEF-14.38: the texts of `#warning` and `#error` reach their line's break and no further,
// whatever comment the line holds
#[test]
fn message_lines_end_as_fteqcc_ends_them() {
    let (_, out) = one_fte("#warning a /* b\n c */ d\ny\n");
    assert_eq!(out.diagnostics[0].message, "#warning:  a /* b");
    assert_eq!(tokens(&out), "y");
    let (_, out) = one_fte("#error a // b \\\nzz\ny\n");
    assert_eq!(tokens(&out), "y");
}

// spec PRJ-5.12: under GMQCC a nested `#include` is found beside the file that holds it, then
// beside the compiled file; a missing one names the first
#[test]
fn gmqcc_includes_are_found_beside_the_including_file_first() {
    let gmqcc = qc_lexer::Dialect::Gmqcc;
    let files = [
        ("src/a.qc", "#include \"x/b.qh\"\n"),
        ("src/x/b.qh", "#include \"c.qh\"\n"),
        ("src/x/c.qh", "beside\n"),
        ("src/c.qh", "compiled\n"),
    ];
    let out = compile_in(&files, "src/a.qc", gmqcc);
    assert_eq!((tokens(&out), codes(&out)), ("beside".to_owned(), vec![]));
    let out = compile_in(&[files[0], files[1], files[3]], "src/a.qc", gmqcc);
    assert_eq!((tokens(&out), codes(&out)), ("compiled".to_owned(), vec![]));
    let out = compile_in(&files[..2], "src/a.qc", gmqcc);
    assert_eq!(codes(&out), vec![Code::E0111]);
    assert!(
        out.diagnostics[0]
            .message
            .starts_with("cannot include src/x/c.qh")
    );
    // `#pragma once` marks, and `#pragma includedir` is joined to, the file holding it.
    let once = [
        (
            "src/a.qc",
            "#include \"x/b.qh\"\n#include \"x/b.qh\"\n#include \"d.qh\"\n",
        ),
        ("src/x/b.qh", "#pragma once\n#pragma includedir y\nb\n"),
        ("src/x/y/d.qh", "d\n"),
    ];
    let out = compile_in(&once, "src/a.qc", gmqcc);
    assert_eq!((tokens(&out), codes(&out)), ("b d".to_owned(), vec![]));
}

// spec FTEF-14.40: a nested `#include` is found beside the compiled file, then in the include
// directories; `<>` names a file too; a missing one is E0111 and a malformed one E0113; an
// included file's directive must start its first byte
// spec QCD-8.16: E0113 has FTEQCC's texts under FTEQCC
#[test]
fn includes_follow_find_best_include() {
    let files = [
        ("src/a.qc", "#include \"x/b.qh\"\n"),
        ("src/x/b.qh", "#include \"c.qh\"\n"),
        ("src/x/c.qh", "wrong\n"),
    ];
    let out = compile(&files, "src/a.qc");
    assert_eq!(codes(&out), vec![Code::E0111]);
    assert!(
        out.diagnostics[0]
            .message
            .starts_with("cannot include src/c.qh")
    );
    let out = compile(
        &[
            (
                "src/a.qc",
                "#pragma includedir x\n#include <c.qh>\n__FILE__\n",
            ),
            ("src/x/c.qh", "right __FILE__\n"),
        ],
        "src/a.qc",
    );
    assert_eq!(tokens(&out), "right \"src/x/c.qh\" \"src/a.qc\"");
    let out = compile(&[("a.qc", "#include b.qh\n#include \"b\nx\n")], "a.qc");
    assert_eq!(
        (tokens(&out), codes(&out)),
        ("x".to_owned(), vec![Code::E0113, Code::E0113])
    );
    assert_eq!(
        out.diagnostics[0].message,
        "Not a string literal (on a #include)"
    );
    assert_eq!(
        out.diagnostics[1].message,
        "#include continued over line boundry"
    );
    let out = compile(
        &[
            ("a.qc", "#include \"b.qh\"\n#include \"c.qh\"\n"),
            ("b.qh", "#define N 5\nN\n"),
            ("c.qh", "  #define M 6\nM\n"),
        ],
        "a.qc",
    );
    assert_eq!(tokens(&out), "5 # define M 6 M");
    let out = compile(
        &[
            ("a.qc", "#include \"b.qh\"\n__LINE__\n"),
            ("b.qh", "#pragma line(20)\n__LINE__\n"),
        ],
        "a.qc",
    );
    assert_eq!(tokens(&out), "20 2");
    let out = compile(&[("a.qc", "#include \"a.qc\"\n")], "a.qc");
    assert_eq!(codes(&out), vec![Code::E0112]);
}

// spec FTEF-14.70: `#pragma once` marks the compiled file, not the file it is in
#[test]
fn pragma_once_marks_the_compiled_file() {
    let out = compile(&[("a.qc", "#pragma once\n#include \"a.qc\"\nx\n")], "a.qc");
    assert_eq!((tokens(&out), codes(&out)), ("x".to_owned(), vec![]));
    let out = compile(
        &[
            ("a.qc", "#include \"b.qh\"\n#include \"b.qh\"\n"),
            ("b.qh", "#pragma once\nx\n"),
        ],
        "a.qc",
    );
    assert_eq!(tokens(&out), "x x");
}

// spec FTEF-14.41: an `#includelist`'s words run to the word `#endlist`, wherever it stands
#[test]
fn include_lists_end_at_the_word() {
    assert_eq!(fte("#includelist\na.qc b.qc #endlist\ny\n").0, "y");
    assert_eq!(fte("#includelist\na.qc ;, (#endlist\ny\n").0, "y");
    assert_eq!(fte("#includelist\r\na.qc\r\n#endlist\r\ny\n").0, "y");
    assert_eq!(fte("#includelist\n#package\n").1, vec![Code::E0406]);
}

// spec FTEF-14.42: a macro name of 64 bytes or more is E0121 and defines nothing; 63 bytes is
// defined; `#define` reads its own word first
#[test]
fn macro_names_are_at_most_63_bytes() {
    let name = |n: usize| "n".repeat(n);
    assert_eq!(
        fte(&format!("#define {} 5\n{}\n", name(63), name(63))).0,
        "5"
    );
    let long = format!("#define {} 5\n{}\n", name(64), name(64));
    assert_eq!(fte(&long), (name(64), vec![Code::E0121]));
    let (_, out) = one_fte(&format!("#append {} 5\n", name(70)));
    assert_eq!(
        out.diagnostics[0].message,
        "Compiler constant name length is too long or short"
    );
    assert_eq!(fte("#defined N 5\nN\n").0, "5");
    assert_eq!(fte("#defineN 6 5\nN 6\n").0, "N 6");
    assert_eq!(
        fte("#define(N) 5\nx\n"),
        ("x".to_owned(), vec![Code::E0114])
    );
}

// spec FTEF-14.43: after `#pragma line N` the next line is `N`, for `__LINE__` and `__LINESTR__`
// and the lines of uses after it, to the end of the file or chunk
#[test]
fn pragma_line_numbers_the_lines() {
    let (_, out) = one_fte("#pragma line(10)\n__LINE__\n\n__LINESTR__\n");
    assert_eq!(tokens(&out), "10 \"12\"");
    assert_eq!(fte("#pragma line 0x10\n__LINE__\n").0, "16");
    assert_eq!(fte("#pragma LINE -1\n__LINE__\n").0, "-1");
    assert_eq!(fte("#pragma line\n__LINE__\n").0, "0");
    assert_eq!(fte("#pragma line 7\n#pragma line 3\n__LINE__\n").0, "3");
    assert_eq!(fte("#pragma linex 7\n__LINE__\n").0, "2");
    assert_eq!(fte("#ifdef Q\n#pragma line 7\n#endif\n__LINE__\n").0, "4");
    assert_eq!(
        fte("#define M(x) __LINE__ x\n#pragma line 9\nM(\n)\n").0,
        "10"
    );
    assert_eq!(
        fte("#define E(n) n\n#pragma line 9\nE(# __LINE__)\n").0,
        "# 9"
    );
    assert_eq!(fte("#pragma line 9").0, "");
}

// spec FTEF-14.70: `DONT_COMPILE_THIS_FILE` passes over the rest of the text, the directives
// at its lines' starts still run; `includedir` and the other pragmas change the build
#[test]
fn dont_compile_this_file_runs_only_directives() {
    let (toks, errs) = fte("x\n#pragma DONT_COMPILE_THIS_FILE\nzz zz\n#error y\n\"a\n#warning z\n");
    assert_eq!((toks.as_str(), errs), ("x", vec![Code::E0118, Code::W0103]));
    let (toks, errs) = fte("#pragma dont_compile_this_file\n#define N 5\nN\n");
    assert_eq!((toks.as_str(), errs), ("", vec![]));
    let (toks, errs) = fte("#pragma DONT_COMPILE_THIS_FILE\n/* \n#error x */ zz\n");
    assert_eq!((toks.as_str(), errs), ("", vec![]));
    for pragma in [
        "COPYRIGHT \"x\"",
        "compress 0",
        "forcecrc 5",
        "framerate 10",
        "nosave 1",
        "defaultstatic 0",
        "wrasm 0",
        "optimise 0",
        "TARGET ID",
        "PROGS_DAT progs.dat",
        "opcode enable x",
        "flag enable x",
        "keyword x",
        "sourcefile a",
        "unknown",
        "",
        "(5) x",
    ] {
        // FTEF-12.11: some give FTEQCC's Q120 warnings, none an error.
        let (toks, errs) = fte(&format!("#pragma {pragma}\ny\n"));
        assert_eq!(toks, "y", "{pragma}");
        assert!(
            errs.iter().all(|code| *code == Code::W0113),
            "{pragma}: {errs:?}"
        );
    }
}

// spec FTEF-14.45: a directive on a line of its own after a use's `(` or `,` runs there and is
// no part of the argument; one that ends the text ends the arguments
#[test]
fn directives_run_among_arguments() {
    let (toks, errs) = fte("#define M(x) x\nM(\n#define Y 5\nY)\n");
    assert_eq!((toks.as_str(), errs), ("5", vec![]));
    let (toks, errs) = fte("#define M(x, y) y\nM(1,\n#ifdef Q\nzz\n#endif\n2)\n");
    assert_eq!((toks.as_str(), errs), ("2", vec![]));
    let (toks, errs) = fte("#define M(x) x\nM(// c\n#undef M\n3)\nM(4)\n");
    assert_eq!((toks.as_str(), errs), ("3 M ( 4 )", vec![]));
    let (toks, _) = fte("#define M(x) x\nM(\n#pragma line(40)\n__LINE__)\n");
    assert_eq!(toks, "40");
    assert_eq!(fte("#define M(x) x\nM(\n#eof\n1)\n").1, vec![Code::E0117]);
    assert_eq!(
        fte("#define M(x) x\nM(\n#package\n1)\n").1,
        vec![Code::E0117]
    );
    assert_eq!(
        fte("#define M(x) x\nM(\n#error e\n1)\n").1,
        vec![Code::E0118]
    );
    // A `#` that starts no directive starts the argument.
    assert_eq!(fte("#define M(x) x\nM(\n#5)\n").0, "# 5");
    // A refused use whose arguments ran a directive goes on after them, so none runs twice.
    let refused = "#define A 1\n#define M(x) x\nM(\n#append A 2\n";
    assert_eq!(
        fte(&format!("{refused}A\n")),
        ("M".to_owned(), vec![Code::E0117])
    );
    assert_eq!(
        fte(&format!("{refused}1, 2, 3) A\n")),
        ("M 1 2".to_owned(), vec![Code::E0116])
    );
    assert_eq!(
        fte("#define M(x) x\nM(\n#error boom\n").1,
        vec![Code::E0118, Code::E0117]
    );
    let (toks, errs) = fte("#define M(x) x\n#define L M(\nL\n#ifdef Q\n#endif\n");
    assert_eq!((toks.as_str(), errs), ("M (", vec![Code::E0117]));
    // An `#include` there is not followed.
    let out = compile(
        &[
            ("a.qc", "#define M(x) x\nM(\n#include \"b.qh\"\n1)\n"),
            ("b.qh", "x"),
        ],
        "a.qc",
    );
    assert_eq!(
        (tokens(&out), codes(&out)),
        ("1".to_owned(), vec![Code::E0121])
    );
}

// spec FTEF-14.46: a `#` that starts a line of an expansion is a directive there; a body with
// such a line is F303, an error unless `#pragma warning` makes it a warning or nothing
// spec QCD-8.15, QCD-7.31: F303 is E0122 as an error and W0109 as a warning, at the `#`
#[test]
fn directives_run_in_expansions() {
    let body = "#define D \\\n#define Y 5 \\\nY\nD Y\n";
    let (toks, errs) = fte(body);
    assert_eq!((toks.as_str(), errs), ("5 5", vec![Code::E0122]));
    let (_, out) = one_fte(body);
    assert_eq!(
        out.diagnostics[0].message,
        "preprocessor directive within preprocessor macro D"
    );
    assert_eq!(out.diagnostics[0].span.span, qc_lexer::Span::new(12, 13));
    let (toks, errs) = fte(&format!("#pragma warning enable F303\n{body}"));
    assert_eq!((toks.as_str(), errs), ("5 5", vec![Code::W0109]));
    let (toks, errs) = fte(&format!("#pragma warning disable F303\n{body}"));
    assert_eq!((toks.as_str(), errs), ("5 5", vec![]));
    let off = "#pragma warning off F303\n";
    let (toks, errs) = fte(&format!(
        "{off}#define D \\\n#ifdef Q \\\nzz \\\n#endif \\\n1\nD\n"
    ));
    assert_eq!((toks.as_str(), errs), ("1", vec![]));
    // A `#pragma line` in a chunk numbers the chunk's lines, which a use after it there has; the
    // chunk's own `__LINE__` is the line of its use (FTEF-14.27).
    let (toks, _) = fte(&format!(
        "{off}#define L __LINE__\n#define D \\\n#pragma line(30) \\\nL __LINE__\nD __LINE__\n"
    ));
    assert_eq!(toks, "30 6 6");
    let (toks, _) = fte(&format!(
        "{off}#define D \\\n#pragma file(x) \\\n__FILE__\nD __FILE__\n"
    ));
    assert_eq!(toks, "\"x\" \"a.qc\"");
    let (toks, _) = fte(&format!("{off}#define D \\\n#eof \\\nzz\nD 2\n"));
    assert_eq!(toks, "2");
    let (toks, _) = fte(&format!("{off}#define D \\\n#package \\\nzz\nD 2\n"));
    assert_eq!(toks, "2");
    // An argument's line break is the expansion's, and a `#` after it a directive there.
    let (toks, errs) = fte("#define M(x) x\nM(1\n#define Y 6\n) Y\n");
    assert_eq!((toks.as_str(), errs), ("1 6", vec![]));
    // A `#` at the expansion's start is none.
    let (toks, errs) = fte("#define N 5\n#define D #N\nD\n");
    assert_eq!((toks.as_str(), errs), ("5", vec![]));
    // An `#include` there is not followed.
    let (toks, errs) = fte(&format!("{off}#define D \\\n#include \"b.qh\" \\\n1\nD\n"));
    assert_eq!((toks.as_str(), errs), ("1", vec![Code::E0121]));
}

// spec FTEF-14.47: directives run inside the readings they started nest at most 64 deep
#[test]
fn nested_directive_readings_are_bounded() {
    let mut text = String::from("#define M(x) x\n");
    for _ in 0..70 {
        text.push_str("M(\n#includelist\n");
    }
    let (_, out) = one_fte(&text);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.message == "directives nested too deeply")
    );
    let shallow = "#define M(x) x\nM(\n#includelist\n#endlist\n1)\n";
    assert_eq!(fte(shallow), ("1".to_owned(), vec![]));
}

// spec FTEF-14.33: an `#if` expression's errors are E0121 with FTEQCC's text, and the side is
// skipped; one nested too deep is only false
#[test]
fn if_expression_errors() {
    // FTEQCC's compile stops at the error, so the rest of the line is no second error.
    for (cond, message) in [
        ("(1", "unclosed bracket condition"),
        ("(1 x", "unclosed bracket condition"),
        ("defined(X", "unclosed defined condition"),
        ("defined(X x", "unclosed defined condition"),
        ("", "unexpected end-of-line"),
        ("1 ||", "unexpected end-of-line"),
    ] {
        let (_, out) = one_fte(&format!("#if {cond}\nx\n#endif\ny\n"));
        assert_eq!(tokens(&out), "y", "{cond:?}");
        assert_eq!(codes(&out), vec![Code::E0121], "{cond:?}");
        assert_eq!(out.diagnostics[0].message, message, "{cond:?}");
    }
    // A `\` is a word like another, `atoi` 0, so the line below is the false side.
    assert_eq!(fte("#if \\\n1\nx\n#endif\ny\n"), ("y".to_owned(), vec![]));
}

// spec FTEF-14.49: the module's include directories, FTEQCC's `-I`, are tried after the compiled
// file, before a `#pragma includedir` one, each with a `/` after it, and are there again for the
// second pass; the vanilla dialect keeps none
#[test]
fn command_line_include_dirs_come_first_and_stay() {
    let files = [
        ("src/a.qc", "#include \"x/b.qh\"\n"),
        ("src/x/b.qh", "#include \"c.qh\"\n"),
        ("src/x/c.qh", "found\n"),
    ];
    let with = |dirs: &[&str], files: &[(&str, &str)]| {
        let mut loader = MemoryLoader::new();
        for (path, text) in files {
            loader.insert(*path, text.as_bytes());
        }
        let dirs: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
        Preprocessor::new(loader)
            .with_dialect(qc_lexer::Dialect::Fteqcc)
            .with_include_dirs(&dirs)
    };
    let mut pp = with(&["src/x"], &files);
    let out = pp.preprocess_file(Path::new("src/a.qc")).unwrap();
    assert_eq!((tokens(&out), codes(&out)), ("found".to_owned(), vec![]));
    let mut pp = with(&[], &files);
    let out = pp.preprocess_file(Path::new("src/a.qc")).unwrap();
    assert_eq!(codes(&out), vec![Code::E0111]);
    let ordered = [
        ("src/a.qc", "#pragma includedir y\n#include \"c.qh\"\n"),
        ("inc/c.qh", "inc\n"),
        ("src/y/c.qh", "pragma\n"),
    ];
    let mut pp = with(&["inc"], &ordered);
    let out = pp.preprocess_file(Path::new("src/a.qc")).unwrap();
    assert_eq!(tokens(&out), "inc");
    assert_eq!(pp.fte.include_dirs, [b"inc/".to_vec(), b"src/y/".to_vec()]);
    pp.restart();
    assert_eq!(pp.fte.include_dirs, [b"inc/".to_vec()]);
    let out = pp.preprocess_file(Path::new("src/a.qc")).unwrap();
    assert_eq!(tokens(&out), "inc");
    let mut pp = with(&["inc/"], &ordered);
    pp.preprocess_file(Path::new("src/a.qc")).unwrap();
    assert_eq!(pp.fte.include_dirs[0], b"inc/".to_vec());
    let vanilla = Preprocessor::new(MemoryLoader::new()).with_include_dirs(&[PathBuf::from("inc")]);
    assert!(vanilla.fte.include_dirs.is_empty());
}

// spec FTEF-14.48: `FTEQCC` and `__FTEQCC__` are defined, empty, before the first file and again
// for the second pass, which also forgets the include directories, the `once` marks and F303's
// setting, and keeps the count of open sides; the vanilla dialect defines neither
#[test]
fn fteqcc_is_defined_and_the_second_pass_starts_afresh() {
    assert_eq!(
        fte("#ifdef FTEQCC\nx\n#endif\n#ifdef __FTEQCC__\ny\n#endif\n").0,
        "x y"
    );
    assert_eq!(fte("FTEQCC __FTEQCC__ z\n#if FTEQCC\nw\n#endif\n").0, "z");
    assert_eq!(fte("#undef FTEQCC\n#ifdef FTEQCC\nx\n#endif\n").0, "");
    let (_, out) = super::one("#ifdef FTEQCC\nx\n#endif\n");
    assert_eq!(tokens(&out), "");
    let mut loader = MemoryLoader::new();
    loader.insert(
        "a.qc",
        "#if 1\n#pragma includedir d\n#pragma once\n#pragma warning off F303\n#undef FTEQCC\n",
    );
    loader.insert("b.qc", "#endif\n");
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert_eq!(pp.fte.include_dirs, [b"d/".to_vec()]);
    assert_eq!(pp.fte.once, [b"a.qc".to_vec()]);
    assert_eq!(
        pp.fte.warnings.action(FteWarning::EvilPreprocessor),
        WarningAction::Ignore
    );
    assert!(!pp.is_defined("FTEQCC"));
    pp.restart();
    assert!(pp.fte.include_dirs.is_empty() && pp.fte.once.is_empty());
    assert_eq!(pp.fte.warnings, WarningSettings::default());
    assert!(pp.is_defined("FTEQCC") && pp.is_defined("__FTEQCC__"));
    // The first pass's `#if 1` is still open, and the second pass's `#endif` closes it.
    let out = pp.preprocess_file(Path::new("b.qc")).unwrap();
    assert_eq!(codes(&out), vec![]);
    // The vanilla dialect's restart defines nothing.
    let mut vanilla = Preprocessor::new(MemoryLoader::new());
    vanilla.restart();
    assert!(!vanilla.is_defined("FTEQCC"));
}

// spec FTEF-14.39: `#append` adds to a body only with the same parameters
// spec FTEF-14.58: another list is F315's "arguments differ in macro append" or "different
// number of macro arguments in macro append", and then the macro's other body F315's "Alternate
// precompiler definition"
#[test]
fn append_needs_the_same_parameters() {
    assert_eq!(fte("#define F(a) a\n#append F(a) 2\nF(1)\n").0, "1 2");
    let (toks, errs) = fte("#define F(a) a\n#append F(b) 2\nF(1)\n");
    assert_eq!((toks.as_str(), errs), ("2", vec![Code::W0115, Code::W0115]));
    let (toks, errs) = fte("#define F(a) a\n#append F 2\nF\n");
    assert_eq!((toks.as_str(), errs), ("2", vec![Code::W0115, Code::W0115]));
    let (_, out) = one_fte("#define F(a) a\n#append F(b) 2\n#append F 3\n");
    let messages: Vec<&str> = out.diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "arguments differ in macro append",
            "Alternate precompiler definition of F (a -> 2)",
            "different number of macro arguments in macro append",
            "Alternate precompiler definition of F (2 -> 3)",
        ]
    );
}

// spec FTEF-14.53: a `#` on a line a `\` joins to a line of code is read as code, in a file and in
// a macro's expansion; after a `//` comment's `\`, or a line ending after the join, it is a
// directive
#[test]
fn joined_lines_hold_no_directive() {
    assert_eq!(
        fte("float g; \\\n#define N 1\nN\n").0,
        "float g ; # define N 1 N"
    );
    assert_eq!(fte("float g; \\\r\n#undef N\n").0, "float g ; # undef N");
    assert_eq!(fte("#define N 1\nN \\\n  #undef N\nN\n").0, "1 # undef 1 1");
    assert_eq!(
        fte("#define M(x) x\nM(5 \\\n#define Y 6\n) Y\n").0,
        "5 # define Y 6 Y"
    );
    assert_eq!(fte("float g; // c \\\n#define N 1\nN\n").0, "float g ; 1");
    assert_eq!(fte("float g; \\\n\n#define N 1\nN\n").0, "float g ; 1");
    let files = [("t.qc", "x \\\n#include \"b.qh\"\n"), ("b.qh", "b\n")];
    assert_eq!(tokens(&compile(&files, "t.qc")), "x # include \"b.qh\"");
}

// spec PRJ-5.15: under GMQCC neither `FTEQCC` nor `__FTEQCC__` is defined, before the first file
// or for the second pass, and directives are read as under FTEQCC
#[test]
fn gmqcc_defines_no_fteqcc_macros() {
    let gmqcc = qc_lexer::Dialect::Gmqcc;
    let files = [(
        "t.qc",
        "#ifdef FTEQCC\nx\n#endif\n#ifdef __FTEQCC__\ny\n#endif\n#defined Q z\nQ\n",
    )];
    assert_eq!(tokens(&compile_in(&files, "t.qc", gmqcc)), "z");
    let mut pp = Preprocessor::new(MemoryLoader::new()).with_dialect(gmqcc);
    assert!(!pp.is_defined("FTEQCC") && !pp.is_defined("__FTEQCC__"));
    pp.restart();
    assert!(!pp.is_defined("FTEQCC") && !pp.is_defined("__FTEQCC__"));
}
