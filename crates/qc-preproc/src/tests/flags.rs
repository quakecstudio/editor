// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's compiler flags as the preprocessor reads them (FTEF-12.12, FTEF-12.13): each case was
//! compiled with the pinned FTEQCC (`fteqcc -Tid`), which read the same tokens.

use std::path::Path;

use qc_lexer::Dialect;

use super::{codes, one_fte, tokens};
use crate::fte_tables::TARGETS;
use crate::{
    Code, CommandLine, CompilerFlags, FileOutput, FteFlag, Keyword, KeywordEdit, Keywords,
    MemoryLoader, Preprocessor, Priorities, TargetFormat,
};

/// The compiler flags in force after each of `out`'s flag marks, each at the token it holds
/// from.
fn flag_states(out: &FileOutput) -> Vec<(usize, CompilerFlags)> {
    let mut now = CompilerFlags::default();
    out.flags
        .iter()
        .map(|&(at, edit)| {
            now = edit.apply(now);
            (at, now)
        })
        .collect()
}

/// The keywords in force after each of `out`'s keyword marks, each at the token it holds from.
fn keyword_states(out: &FileOutput) -> Vec<(usize, Keywords)> {
    let mut now = Keywords::NONE;
    out.keywords
        .iter()
        .map(|&(at, edit)| {
            now = edit.apply(now);
            (at, now)
        })
        .collect()
}

/// Preprocesses `a.qc` under the FTEQCC dialect with the command line `args`.
fn one_with(text: &str, args: &[&str]) -> FileOutput {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", text.as_bytes());
    let mut pp = Preprocessor::new(loader)
        .with_dialect(qc_lexer::Dialect::Fteqcc)
        .with_command_line(CommandLine::new(args));
    pp.preprocess_file(Path::new("a.qc")).unwrap()
}

// spec FTEF-20.1: the keywords are marked by token: the whole state at token 0, then each
// `#pragma keyword` or `#pragma flag` that names a keyword, `kce` or `caseinsens`, setting only
// those; a pragma that names none adds no mark, and the vanilla dialect has none
#[test]
fn keyword_changes_are_marked() {
    let (_, out) = one_fte(concat!(
        "a\n",
        "#pragma keyword disable for\n",
        "b\n",
        "#pragma flag enable qccx\n",
        "c\n",
        "#pragma flag disable kce\n",
        "d\n",
    ));
    let off = Keywords::default().with(Keyword::For, false);
    assert_eq!(
        keyword_states(&out),
        [
            (0, Keywords::default()),
            (1, off),
            (3, off.with_coexist(false)),
        ]
    );
    let defined = Keywords::default().with(Keyword::Switch, false);
    let named = out.keywords.get(1).map(|&(_, edit)| edit.apply(defined));
    assert_eq!(named, Some(defined.with(Keyword::For, false)));
    let (_, out) = super::one("a\n#pragma keyword disable for\nb\n");
    assert!(out.keywords.is_empty());
    let out = one_with("a\n", &["-Kno-for", "-Fcaseinsens"]);
    assert_eq!(
        out.keywords,
        [(0, KeywordEdit::whole(off.with_case_insensitive(true)))]
    );
}

// spec FTEF-20.4: `#pragma TARGET` marks whether the target is Hexen II's, for a definition
// named `float`, only when that changes
#[test]
fn a_target_pragma_marks_the_hexen2_target() {
    let (_, out) = one_fte(concat!(
        "a\n",
        "#pragma TARGET HEXEN2\n",
        "b\n",
        "#pragma TARGET UHEXEN2\n",
        "c\n",
        "#pragma TARGET FTE\n",
        "d\n",
        "#pragma TARGET NOPE\n",
        "e\n",
    ));
    let states = keyword_states(&out);
    let hexen2: Vec<(usize, bool)> = states
        .iter()
        .map(|&(at, k)| (at, k.hexen2_target()))
        .collect();
    assert_eq!(hexen2, [(0, false), (1, true), (3, false)]);
    let out = one_with("a\n", &["-Thexen2"]);
    assert!(keyword_states(&out).iter().all(|(_, k)| k.hexen2_target()));
}

// spec FTEF-20.3: a Hexen II target turns on `thinktime`, `until` and `loop` after the command
// line, as the second pass starts again
#[test]
fn a_hexen2_target_turns_its_keywords_on() {
    for args in [&["-Thexen2"][..], &["-Tuhexen2"], &["-Tfteh2"], &["-Th2"]] {
        let out = one_with("a\n", args);
        let (_, keywords) = keyword_states(&out)[0];
        for keyword in [Keyword::Thinktime, Keyword::Until, Keyword::Loop] {
            assert!(keywords.has(keyword), "{args:?} {keyword:?}");
        }
    }
    for args in [&["-Tfte"][..], &["-h2"], &[], &["-Thexen2", "-Tid"]] {
        let out = one_with("a\n", args);
        assert!(!keyword_states(&out)[0].1.has(Keyword::Loop), "{args:?}");
    }
    let out = one_with("#pragma TARGET HEXEN2\na\n", &[]);
    assert!(
        keyword_states(&out)
            .iter()
            .all(|(_, k)| !k.has(Keyword::Loop))
    );
}

// spec FTEF-12.12: a `#pragma flag` changes how the tokens after it are lexed, and a later one
// changes them back
#[test]
fn a_flag_pragma_lexes_the_rest_again() {
    let (_, out) = one_fte(concat!(
        "\"a\":\"b\";\n",
        "#pragma flag enable qccx\n",
        "\"a\":\"b\";\n",
        "#pragma flag disable qccx\n",
        "\"a\":\"b\";\n",
    ));
    assert_eq!(tokens(&out), r#""a" : "b" ; "a" "\x00" "b" ; "a" : "b" ;"#);
    // The lexical errors before the pragma stay, once each, and those after are the new
    // lexing's.
    let (_, out) = one_fte("\"a\n#pragma flag enable qccx\n\"b\n");
    assert_eq!(codes(&out), [Code::E0102, Code::E0102]);
    let (_, out) = one_fte("\"a\n#pragma flag enable qccx\nb\n");
    assert_eq!(codes(&out), [Code::E0102]);
    // A pragma in an expansion (FTEF-14.46) lexes again from the end of the use, where an error
    // of the string that touches it is the new lexing's alone.
    let (_, out) = one_fte(concat!(
        "#pragma warning disable F303\n",
        "#define D \\\n#pragma flag enable qccx \\\nx\n",
        "D\"a\n",
    ));
    assert_eq!(codes(&out), [Code::E0102]);
}

// spec FTEF-12.12: a `#pragma flag` in an expansion (FTEF-14.46) lexes the rest of the
// expansion again with the new flags
#[test]
fn a_flag_pragma_in_an_expansion_lexes_the_rest_of_it_again() {
    let (_, out) = one_fte(concat!(
        "#pragma warning disable F303\n",
        "#define D \\\n#pragma flag enable qccx \\\n\"a\":\"b\"\n",
        "D\n",
    ));
    assert_eq!(tokens(&out), r#""a" "\x00" "b""#);
}

// spec FTEF-12.11: `QCC_main` sets the flags once, before the first pass, so a `#pragma flag`
// holds into the second autoprototyping pass
#[test]
fn a_flag_holds_into_the_second_pass() {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", b"#pragma flag enable qccx\n\"a\":\"b\";\n");
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    pp.preprocess_file(Path::new("a.qc")).unwrap();
    pp.restart();
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert_eq!(tokens(&out), r#""a" "\x00" "b" ;"#);
    assert!(
        flag_states(&out)
            .iter()
            .all(|(_, flags)| flags.has(FteFlag::Qccx))
    );
}

// spec FTEF-14.72: the command line's `-W` and `-T` options are read again as the second pass
// starts, so they hold in it as in the first
#[test]
fn the_command_line_holds_into_the_second_pass() {
    assert_eq!(codes(&one_with("/* /* */\n", &[])), [Code::W0112]);
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", b"/* /* */\n");
    let mut pp = Preprocessor::new(loader)
        .with_dialect(Dialect::Fteqcc)
        .with_command_line(CommandLine::new(&["-Wno-F325", "-Thexen2"]));
    pp.preprocess_file(Path::new("a.qc")).unwrap();
    pp.restart();
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert!(codes(&out).is_empty());
    assert!(keyword_states(&out).iter().all(|(_, k)| k.hexen2_target()));
}

// spec FTEF-12.12: the preprocessor marks each change of the lexer's flags by token, starting
// with the defaults at token 0
// spec FTEF-21.1: a pragma that names no flag the lexer, the parser or the semantic model reads
// adds no mark, and one that does marks only what it names
#[test]
fn lexer_flag_changes_are_marked() {
    let (_, out) = one_fte(concat!(
        "a\n",
        "#pragma flag enable hashonly\n",
        "b\n",
        "#pragma flag enable ssp utf8\n",
        "c\n",
    ));
    let both = CompilerFlags::default()
        .with(FteFlag::Dblstarexp, true)
        .with(FteFlag::Utf8strings, true);
    assert_eq!(
        flag_states(&out),
        [(0, CompilerFlags::default()), (2, both)]
    );
    assert_eq!(out.flags.len(), 2);
}

// spec FTEF-12.12: a flag's lexical warning is reported with the warning settings, F313 an error
// spec QCD-8.17: E0125 is an FTEQCC warning made an error
#[test]
fn flag_warnings_follow_the_warning_settings() {
    let (_, out) = one_fte("#pragma flag enable qccx\nfloat x = 'a';\n");
    assert_eq!(codes(&out), [Code::E0125]);
    let (_, out) = one_fte(concat!(
        "#pragma flag enable qccx\n",
        "#pragma warning disable F313\n",
        "float x = 'a';\n",
    ));
    assert!(codes(&out).is_empty());
    let (_, out) = one_fte("#pragma flag enable ssp\nx = 2**3;\n");
    assert_eq!(codes(&out), [Code::W0118]);
    assert_eq!(tokens(&out), "x = 2 *^ 3 ;");
}

// spec FTEF-12.12: a macro's text is lexed when it is used, with the flags of that moment
#[test]
fn a_macro_is_lexed_with_the_flags_where_it_is_used() {
    let (_, out) = one_fte(concat!(
        "#define P 2**3\n",
        "P;\n",
        "#pragma flag enable ssp\n",
        "P;\n",
    ));
    assert_eq!(tokens(&out), "2 * * 3 ; 2 *^ 3 ;");
}

// spec FTEF-12.12: under `macroinstrings` a `#NAME` in a string reads the macros defined where
// the string is, the text after a `#define` or `#undef` lexed again
#[test]
fn macro_names_in_strings_read_the_macros_of_the_moment() {
    let (_, out) = one_fte(concat!(
        "#define FOO bar\n",
        "\"#FOO\"\n",
        "#pragma flag enable macroinstrings\n",
        "\"#FOO\"\n",
        "#undef FOO\n",
        "\"#FOO\"\n",
        "#define FOO baz\n",
        "\"#FOO\"\n",
    ));
    assert_eq!(tokens(&out), r##""#FOO" "bar" "#FOO" "baz""##);
    // A string a `#NAME` wrote to is spelled as its bytes, for a reader that lexes the text
    // again with no macros; the others keep their source text.
    let spellings: Vec<Option<&str>> = out.tokens.iter().map(|t| t.spelling.as_deref()).collect();
    assert_eq!(
        spellings,
        [None, Some("\"bar\""), Some("\"#FOO\""), Some("\"baz\"")]
    );
    let (_, out) = one_fte("#pragma flag enable macroinstrings\n#define M \"a#M\"\nM\n");
    assert_eq!(out.tokens[0].spelling.as_deref(), Some("\"a\\\"a#M\\\"\""));
}

// spec FTEF-12.13: under `hashonly` a name is a macro's or a built-in constant's only after a `#`
#[test]
fn hashonly_expands_only_after_a_hash() {
    let (_, out) = one_fte(concat!(
        "#define FOO 5\n",
        "FOO __LINE__\n",
        "#pragma flag enable hashonly\n",
        "FOO #FOO __LINE__\n",
    ));
    assert_eq!(tokens(&out), "5 2 FOO 5 __LINE__");
}

// spec FTEF-12.13: under `nofileline` `#pragma file` and `#pragma line` change nothing
#[test]
fn nofileline_ignores_file_and_line_pragmas() {
    let (_, out) = one_fte(concat!(
        "#pragma flag enable nofileline\n",
        "#pragma line 100\n",
        "#pragma file(x.qc)\n",
        "__LINE__ __FILE__\n",
    ));
    assert_eq!(tokens(&out), r#"4 "a.qc""#);
    let (_, out) = one_fte("#pragma line 100\n#pragma file(x.qc)\n__LINE__ __FILE__\n");
    assert_eq!(tokens(&out), r#"101 "x.qc""#);
}

// spec FTEF-12.17: the flags that belong to the compiler or its output change no token, and
// `acc`, which a pragma cannot set, leaves a `|` in a string as it is
// spec FTEF-19.24: `assumedouble` changes no token, and is marked for the semantic model
// spec FTEF-21.4: `cpriority` changes no token, and is marked for the parser with the priorities
// it sets
#[test]
fn other_flags_change_no_token() {
    let text = "x = 1.5 \"a|b\" 'a';\n";
    let (_, plain) = one_fte(text);
    let (_, out) = one_fte(&format!(
        "#pragma flag enable cpriority msvcstyle debugmacros\n{text}"
    ));
    assert_eq!(tokens(&out), tokens(&plain));
    let c = CompilerFlags::default()
        .with(FteFlag::Cpriority, true)
        .with_priorities(Priorities::C);
    assert_eq!(flag_states(&out), [(0, CompilerFlags::default()), (0, c)]);
    let (_, out) = one_fte(&format!(
        "#pragma flag enable msvcstyle debugmacros\n{text}"
    ));
    assert_eq!(flag_states(&out), [(0, CompilerFlags::default())]);
    let (_, out) = one_fte(&format!("#pragma flag enable assumedouble\n{text}"));
    assert_eq!(tokens(&out), tokens(&plain));
    let double = CompilerFlags::default().with(FteFlag::AssumeDouble, true);
    assert_eq!(
        flag_states(&out),
        [(0, CompilerFlags::default()), (0, double)]
    );
    assert!(codes(&out).is_empty());
    let (_, out) = one_fte(&format!("#pragma flag enable acc caseinsens\n{text}"));
    assert_eq!(tokens(&out), r#"x = 1.5 "a|b" 97 ;"#);
    assert_eq!(codes(&out), [Code::W0113, Code::W0113]);
}

// spec FTEF-14.73: under `-Fcaseinsens` the macro table is searched in any case, by a use, by
// `#ifdef`, `#ifndef`, `#if defined()` and `#if NAME`, and a definition in another case replaces
// the macro; without it the names are exact
#[test]
fn macros_are_found_in_any_case_under_caseinsens() {
    let any = ["-Fcaseinsens"];
    let found = |define: &str, use_: &str, args: &[&str]| {
        let out = one_with(&format!("{define}\n{use_}\n"), args);
        tokens(&out)
    };
    assert_eq!(found("#define Foo 5", "foo FOO Foo", &any), "5 5 5");
    assert_eq!(found("#define Foo 5", "foo FOO Foo", &[]), "foo FOO 5");
    assert_eq!(found("#define foo(a) a", "FOO(1)", &any), "1");
    assert_eq!(found("#define foo(a) a", "FOO(1)", &[]), "FOO ( 1 )");
    let test = |condition: &str, args: &[&str]| {
        found(
            "#define Foo 5",
            &format!("{condition}\nyes\n#else\nno\n#endif"),
            args,
        )
    };
    for condition in [
        "#ifdef foo",
        "#if defined(fOO)",
        "#if foo == 5",
        "#if !defined(bar)",
    ] {
        assert_eq!(test(condition, &any), "yes", "{condition}");
    }
    assert_eq!(test("#ifndef FOO", &any), "no");
    assert_eq!(test("#ifdef foo", &[]), "no");
    assert_eq!(found("#define foo 5\n#define FOO 6", "foo", &any), "6");
    assert_eq!(found("#define foo 5\n#undef foo", "foo", &any), "foo");
}

// spec FTEF-14.73: `#undef` finds the macro in any case but removes it only when the name is the
// spelling it was defined with, and says nothing of a macro it found
#[test]
fn undef_removes_the_spelling_it_names() {
    let any = ["-Fcaseinsens"];
    let out = one_with("#define foo 5\n#undef FOO\nfoo\n", &any);
    assert_eq!(tokens(&out), "5");
    assert_eq!(codes(&out), []);
    let out = one_with("#undef FOO\n", &any);
    assert_eq!(codes(&out), [Code::W0114]);
    let out = one_with("#define foo 5\n#undef FOO\n", &[]);
    assert_eq!(codes(&out), [Code::W0114]);
}

// spec FTEF-14.73: the macros FTEQCC defines itself are added before the search is switched, so
// under `-Fcaseinsens` none is found, in any case; a macro whose body names another finds it
// in any case
#[test]
fn the_built_in_macros_are_not_found_in_any_case() {
    let any = ["-Fcaseinsens"];
    for name in ["FTEQCC", "fteqcc", "__FTEQCC__", "__fteqcc__"] {
        let out = one_with(&format!("#ifdef {name}\nyes\n#else\nno\n#endif\n"), &any);
        assert_eq!(tokens(&out), "no", "{name}");
    }
    let out = one_with("#ifdef FTEQCC\nyes\n#else\nno\n#endif\n", &[]);
    assert_eq!(tokens(&out), "yes");
    let out = one_with("#ifdef fteqcc\nyes\n#else\nno\n#endif\n", &[]);
    assert_eq!(tokens(&out), "no");
    let out = one_with("#define Foo 5\n#define bar FOO\nbar\n", &any);
    assert_eq!(tokens(&out), "5");
}

/// Every output format `targets[]` names, each with the `flag_undefwordsize`
/// `QCC_OPCodeSetTarget` sets for it and whether `QCC_PR_FinishCompilation` lets a function be
/// `extern` under it: the match is exhaustive, so a new format must be given both.
fn every_target() -> impl Iterator<Item = (TargetFormat, bool, bool)> {
    TARGETS.iter().map(|&(_, format, _)| {
        let (undefined_word_size, supports_extern) = match format {
            TargetFormat::Standard
            | TargetFormat::Qss
            | TargetFormat::Hexen2
            | TargetFormat::Uhexen2
            | TargetFormat::Kk7
            | TargetFormat::Qtest => (false, false),
            TargetFormat::Fte | TargetFormat::Fteh2 | TargetFormat::FteDebug => (false, true),
            TargetFormat::Darkplaces => (true, false),
        };
        (format, undefined_word_size, supports_extern)
    })
}

// spec FTEF-22.4: `QCC_OPCodeSetTarget` sets `flag_undefwordsize` to true for `QCF_DARKPLACES`
// only and to false for every other format
#[test]
fn only_the_darkplaces_target_has_an_undefined_word_size() {
    for (format, undefined_word_size, _) in every_target() {
        assert_eq!(
            format.undefined_word_size(),
            undefined_word_size,
            "{format:?}"
        );
    }
}

// spec FTEF-22.4: each `-TNAME` and each `#pragma TARGET` that names a target runs
// `QCC_OPCodeSetTarget`, in order, so `-Tdp -Tfte` and `-Fundefwordsize -Tid` leave
// `undefwordsize` off and `-Tdp` and `#pragma target dp` turn it on at the token after
#[test]
fn a_darkplaces_target_turns_undefwordsize_on() {
    for (args, on) in [
        (&["-Tdp"][..], true),
        (&["-Tdarkplaces"], true),
        (&["-Tdp_1"], true),
        (&["-Fundefwordsize", "-Tdp"], true),
        (&["-Tdp", "-Tfte"], false),
        (&["-Fundefwordsize", "-Tid"], false),
        (&["-Tfte"], false),
        (&[], false),
    ] {
        let out = one_with("a\n", args);
        let (_, flags) = flag_states(&out)[0];
        assert_eq!(flags.has(FteFlag::Undefwordsize), on, "{args:?}");
    }
    let out = one_with("a\n#pragma target dp\nb\n#pragma target id\nc\n", &[]);
    let on: Vec<bool> = flag_states(&out)
        .iter()
        .map(|(_, flags)| flags.has(FteFlag::Undefwordsize))
        .collect();
    assert_eq!(on, [false, true, false]);
    assert_eq!(flag_states(&out)[1].0, 1);
}

// spec FTEF-22.5: an `extern` function is the error unless the target is `QCF_FTE`,
// `QCF_FTEDEBUG` or `QCF_FTEH2`
#[test]
fn only_the_fte_targets_support_extern() {
    for (format, _, supports_extern) in every_target() {
        assert_eq!(format.supports_extern(), supports_extern, "{format:?}");
    }
}

// spec FTEF-25.1: the target holds from the file's start, and a `#pragma TARGET` makes every
// opcode valid or not again, undoing a `#pragma opcode` before it
// spec FTEF-25.3: a `#pragma opcode` holds from the token after it
#[test]
fn opcode_targets_hold_from_the_token_after_their_pragma() {
    use crate::Op;
    use crate::OpcodeTarget;
    let (_, out) = one_fte(
        "float a;\n#pragma opcode enable ADD_I\nfloat b;\n#pragma target fte\nfloat c;\n\
         #pragma target id\nfloat d;\n",
    );
    let marks: Vec<(usize, bool, TargetFormat)> = out
        .targets
        .iter()
        .map(|(at, target)| (*at, target.valid(Op::AddI), target.format()))
        .collect();
    assert_eq!(
        marks,
        [
            (0, false, TargetFormat::Standard),
            (3, true, TargetFormat::Standard),
            (6, true, TargetFormat::Fte),
            (9, false, TargetFormat::Standard),
        ]
    );
    assert_eq!(OpcodeTarget::default().format(), TargetFormat::Standard);
}

// spec FTEF-25.1: the command line's last `-T` option sets the target the file starts with
#[test]
fn the_command_lines_target_starts_the_file() {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", b"float a;\n");
    let mut pp = Preprocessor::new(loader)
        .with_dialect(Dialect::Fteqcc)
        .with_command_line(CommandLine::new(&["-Tid", "-Tfte_5744"]));
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    let first = out
        .targets
        .first()
        .map(|(_, target)| (target.format(), target.revision()));
    assert_eq!(first, Some((TargetFormat::Fte, 5744)));
}

// spec FTEF-14.43: the places FTEQCC's messages give hold from the token after a `#pragma file`
// or `#pragma line`, each the file's name and how far its lines are from the source's
#[test]
fn message_places_follow_pragma_file_and_line() {
    let (_, out) = one_fte("float a;\n#pragma file foo.qc\nfloat b;\n#pragma line 40\nfloat c;\n");
    let marks: Vec<(usize, String, i64)> = out
        .places
        .iter()
        .map(|(at, name, delta)| (*at, String::from_utf8_lossy(name).into_owned(), *delta))
        .collect();
    assert_eq!(
        marks,
        [
            (0, "a.qc".to_owned(), 0),
            (3, "foo.qc".to_owned(), 0),
            (6, "foo.qc".to_owned(), 35),
        ]
    );
}
