// SPDX-License-Identifier: GPL-3.0-or-later

//! Where an FTEQCC directive's text ends for a reader that runs none (`fte_line`, FTEF-14.51).

use qc_lexer::{Dialect, Lexed, Span, TokenKind, lex_with};

use crate::{fte_directive_end, relex_from};

/// The text of the directive at the start of `text`, `None` when there is none.
fn directive(text: &str) -> Option<&str> {
    let hash = text.find('#')?;
    let end = fte_directive_end(text.as_bytes(), hash)?;
    text.get(hash..end)
}

// spec FTEF-14.51: each directive ends where the preprocessor reads on after it with nothing
// run: its line's end as `QCC_PR_SkipToEndOfLine` ends it, after its operand
#[test]
fn each_directive_ends_where_the_preprocessor_reads_on() {
    let cases = [
        ("#\nx", "#"),
        ("#undef X // \\\ny\nz", "#undef X // \\\ny"),
        ("#ifdef X /*\n*/\nz", "#ifdef X /*\n*/"),
        ("#ifndef X\nz", "#ifndef X"),
        ("#if 1 // \\\ny\nz", "#if 1 // \\\ny"),
        ("#else // \\\ny\nz", "#else // \\\ny"),
        ("#elif 0 // \\\ny\nz", "#elif 0 // \\\ny"),
        ("#endif /*\n*/\nz", "#endif /*\n*/"),
        ("#error e \\\ny\nz", "#error e \\\ny"),
        ("#warning w \\\ny\nz", "#warning w \\\ny"),
        ("#message m \\\ny\nz", "#message m \\\ny"),
        ("#copyright c \\\ny\nz", "#copyright c \\\ny"),
        ("#forcecrc 5 \\\ny\nz", "#forcecrc 5 \\\ny"),
        ("#merge \"m\" \\\ny\nz", "#merge \"m\" \\\ny"),
        ("#datafile \"d\" \\\ny\nz", "#datafile \"d\" \\"),
        ("#output \"o\" \\\ny\nz", "#output \"o\" \\\ny"),
        ("#pack 2 \\\ny\nz", "#pack 2 \\\ny"),
        ("#package p \\\ny\nz", "#package p \\\ny"),
        ("#includelist \\\ny\nz", "#includelist \\\ny"),
        ("#include \"b.qh\" \\\ny\nz", "#include \"b.qh\" \\\ny"),
        ("#include b.qh \\\ny\nz", "#include b.qh \\\ny"),
        ("#include \"b.qh\nz", "#include \"b.qh"),
        ("#pragma p \\\ny\nz", "#pragma p \\"),
        ("#define M 1 // \\\ny\nz", "#define M 1 // \\\ny"),
        ("#define M(a) a \\\n a\nz", "#define M(a) a \\\n a"),
        ("#define\nz", "#define"),
        ("#iffy x\nz", "#iffy"),
        ("#if(1)\nz", "#if"),
        ("#eof // \\\ny\nz", "#eof // \\"),
        ("#eof", "#eof"),
        ("#undef X", "#undef X"),
        ("#endif\0\nz", "#endif"),
    ];
    for (text, expected) in cases {
        assert_eq!(directive(text), Some(expected), "{text:?}");
    }
}

// spec FTEF-14.51: a `#` `QCC_PR_NewLine` does not reach, or before a name of no directive,
// starts none; one after a line ending and spaces and tabs, or at the file's start after them,
// does
#[test]
fn only_a_hash_new_line_reaches_starts_a_directive() {
    for text in ["/* */ #define X", "x #define X", "#foo", "#endlist", "x"] {
        assert_eq!(directive(text), None, "{text:?}");
    }
    assert_eq!(fte_directive_end(b"x", 0), None);
    assert_eq!(directive(" \t#endif\nz"), Some("#endif"));
    assert_eq!(directive("x\n\t #endif\nz"), Some("#endif"));
    assert_eq!(directive("x\r#endif\nz"), Some("#endif"));
}

/// The spans of `lexed`'s tokens and the starts of its errors.
fn spans(lexed: &Lexed) -> (Vec<(usize, usize)>, Vec<usize>) {
    let tokens = lexed.tokens.iter().map(|t| (t.span.start, t.span.end));
    let errors = lexed.diagnostics.iter().map(|d| d.span.start);
    (tokens.collect(), errors.collect())
}

/// `text` lexed under FTEQCC, then lexed again from `offset`.
fn relexed(text: &[u8], offset: usize) -> Lexed {
    let mut lexed = lex_with(text, Dialect::Fteqcc);
    relex_from(&mut lexed, text, offset, Dialect::Fteqcc);
    lexed
}

// spec FTEF-14.51: the text from a directive's end is lexed again as the preprocessor reads on
// there: not at all after white space alone, and otherwise only as far as a token the new
// lexing reaches as the first did, the first lexing's errors after it kept
#[test]
fn text_after_a_directive_is_lexed_again_as_the_preprocessor_reads_on() {
    let text = b"#undef X\nfloat y;";
    assert_eq!(
        spans(&relexed(text, 8)),
        spans(&lex_with(text, Dialect::Fteqcc))
    );
    let text = b"#undef X // c\nfloat y; \"s\n";
    let first = lex_with(text, Dialect::Fteqcc);
    assert_eq!(spans(&first).1, [23]);
    assert_eq!(spans(&relexed(text, 13)), spans(&first));
    // A token across the end is lexed again from there, in the middle of a line after a
    // token of its own, and its error is the new lexing's.
    let lexed = relexed(b"#if\"a b\";\n$frame f g\n", 5);
    assert_eq!(
        spans(&lexed),
        (vec![(0, 1), (1, 3), (6, 7), (7, 9), (10, 20)], vec![7])
    );
    let frames = lexed.tokens.iter().find_map(|t| match &t.kind {
        TokenKind::Frames(names) => Some(names.iter().map(|n| n.span).collect::<Vec<_>>()),
        _ => None,
    });
    assert_eq!(frames, Some(vec![Span::new(17, 18), Span::new(19, 20)]));
    // An error of the first lexing that the new one does not make is gone.
    let text = b"#if\"a b\" \"c\n";
    assert_eq!(spans(&lex_with(text, Dialect::Fteqcc)).1, [9]);
    assert_eq!(spans(&relexed(text, 5)).1, Vec::<usize>::new());
    // A token lexed again starts a line where only white space is before it on its line.
    let lexed = relexed(b"#undef X // c\n#warning w\n", 13);
    assert!(
        lexed
            .tokens
            .iter()
            .any(|t| t.line_start && t.span.start == 14)
    );
}
