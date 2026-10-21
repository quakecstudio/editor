// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's word and string readers' bounds (FTEF-14.66) and the comments its white-space reader
//! meets the end of the text in (FTEF-14.67): each case was compiled with the pinned FTEQCC
//! (`fteqcc -Tid`), which gave the same error, at 1024 bytes and not at 1023.

use super::{codes, one_fte};
use crate::Code;

/// The codes and messages `text` gives under FTEQCC.
fn errors(text: &str) -> Vec<(Code, String)> {
    let (_, out) = one_fte(text);
    out.diagnostics
        .into_iter()
        .map(|d| (d.code, d.message))
        .collect()
}

fn too_long() -> Vec<(Code, String)> {
    vec![(Code::E0121, "token exceeds 1023 chars".to_owned())]
}

// spec FTEF-14.66: a directive's name or word of 1024 bytes is "token exceeds 1023 chars", and
// one of 1023 is read
#[test]
fn directive_words_past_1023_bytes_stop_the_compile() {
    let long = "A".repeat(1024);
    for text in [
        format!("#undef {long}\n"),
        format!("#ifdef {long}\n#endif\n"),
        format!("#ifndef {long}\n#endif\n"),
        format!("#if defined {long}\n#endif\n"),
        format!("#if {long}\n#endif\n"),
        format!("#if 0\n#elifdef {long}\n#endif\n"),
        format!("#define {long} 1\n"),
        format!("#includelist\n{long}\n#endlist\n"),
        format!("#if{long}\n"),
        format!("#define{long}\n"),
    ] {
        let shown: String = text.chars().take(12).collect();
        assert_eq!(errors(&text), too_long(), "{shown}");
    }
    let fits = "A".repeat(1023);
    assert_eq!(
        codes(&one_fte(&format!("#undef {fits}\n")).1),
        [Code::W0114]
    );
    assert!(errors(&format!("#ifdef {fits}\n#endif\n")).is_empty());
    // The compile stops: no more of the file is read.
    let (_, out) = one_fte(&format!("#undef {long}\nfloat x;\n"));
    assert!(out.tokens.is_empty());
}

// spec FTEF-14.66: a string of 1024 bytes, or with a line break, after `#output`, `#merge` or
// `#datafile` stops the compile
#[test]
fn directive_strings_past_1023_bytes_or_lines_stop_the_compile() {
    let long = "s".repeat(1024);
    for directive in ["output", "merge", "datafile"] {
        assert_eq!(
            errors(&format!("#{directive} \"{long}\"\n")),
            too_long(),
            "{directive}"
        );
        assert_eq!(
            errors(&format!("#{directive} \"abc\n")),
            [(Code::E0121, "new line inside string".to_owned())],
            "{directive}"
        );
    }
    let fits = "s".repeat(1023);
    assert!(errors(&format!("#output \"{fits}\"\n")).is_empty());
}

// spec FTEF-14.66: a frame macro word of 1024 bytes is "token exceeds 1023 chars" from the lexer
#[test]
fn frame_words_past_1023_bytes() {
    let (_, out) = one_fte(&format!("$frame {}\n", "f".repeat(1024)));
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code == Code::E0502 && d.message == "token exceeds 1023 chars")
    );
    let (_, out) = one_fte(&format!("$frame {}\n", "f".repeat(1023)));
    assert!(!out.diagnostics.iter().any(|d| d.code == Code::E0502));
}

// spec FTEF-14.67: a comment the text ends in, before an `#includelist` word or in a use's
// arguments, is the one error "EOF inside comment"
#[test]
fn comments_the_text_ends_in() {
    let eof = vec![(Code::E0501, "EOF inside comment".to_owned())];
    assert_eq!(errors("#includelist\n/* open\n"), eof);
    assert_eq!(errors("#define M(x) x\nM( /* open\n"), eof);
    assert_eq!(errors("#define M(x, y) x\nM(1, /* open\n"), eof);
    assert_eq!(errors("#pragma DONT_COMPILE_THIS_FILE\nx /* open\n"), eof);
    // A closed one is no error.
    assert!(errors("#includelist\n/* c */\n#endlist\n").is_empty());
}
