// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's compiler flags as the preprocessor reads them (FTEF-12.12, FTEF-12.13): each case was
//! compiled with the pinned FTEQCC (`fteqcc -Tid`), which read the same tokens.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::Path;

use qc_lexer::{Dialect, LexFlag, LexFlags};

use super::{codes, one_fte, tokens};
use crate::{Code, MemoryLoader, Preprocessor};

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
        out.lex_flags
            .iter()
            .all(|(_, flags)| flags.has(LexFlag::Qccx))
    );
}

// spec FTEF-12.12: the preprocessor marks each change of the lexer's flags by token, starting
// with the defaults at token 0; a pragma that changes no lexer flag adds no mark
#[test]
fn lexer_flag_changes_are_marked() {
    let (_, out) = one_fte(concat!(
        "a\n",
        "#pragma flag enable hashonly\n",
        "b\n",
        "#pragma flag enable ssp utf8\n",
        "c\n",
    ));
    let both = LexFlags::default()
        .with(LexFlag::Ssp, true)
        .with(LexFlag::Utf8Strings, true);
    assert_eq!(out.lex_flags, [(0, LexFlags::default()), (2, both)]);
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

// spec FTEF-12.14: the flags that belong to the parse, the compiler or its output change no
// token, and `acc`, which a pragma cannot set, leaves a `|` in a string as it is
#[test]
fn other_flags_change_no_token() {
    let text = "x = 1.5 \"a|b\" 'a';\n";
    let (_, plain) = one_fte(text);
    let (_, out) = one_fte(&format!(
        "#pragma flag enable assumedouble cpriority msvcstyle debugmacros\n{text}"
    ));
    assert_eq!(tokens(&out), tokens(&plain));
    assert_eq!(out.lex_flags, [(0, LexFlags::default())]);
    assert!(codes(&out).is_empty());
    let (_, out) = one_fte(&format!("#pragma flag enable acc caseinsens\n{text}"));
    assert_eq!(tokens(&out), r#"x = 1.5 "a|b" 97 ;"#);
    assert_eq!(codes(&out), [Code::W0113, Code::W0113]);
}
