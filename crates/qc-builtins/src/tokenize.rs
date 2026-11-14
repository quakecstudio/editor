// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2016      Spike
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/pr_ext.c, tokenizeqc, PF_Tokenize, PF_tokenize_console, PF_tokenizebyseparator, PF_ArgV, PF_argv_start_index, PF_argv_end_index
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/common.c, COM_ParseEx, COM_Parse

//! vkQuake's string tokenizer builtins: the token list `tokenize`, `tokenize_console` and
//! `tokenizebyseparator` fill and `argv`, `argv_start_index` and `argv_end_index` read
//! (BUILTIN-18.23, BUILTIN-18.24).

use crate::STRINGTEMP_TEXT;

/// The most tokens the list holds, `MAXQCTOKENS`.
pub const MAX_QC_TOKENS: usize = 64;

/// The bytes `COM_ParseEx`'s `com_token` holds, its terminating zero among them,
/// `COM_PARSE_MAX_TOKEN_SIZE`.
const COM_PARSE_MAX_TOKEN_SIZE: usize = 4096;

/// The most separators `tokenizebyseparator` reads, `sep[7]`.
pub const MAX_SEPARATORS: usize = 7;
/// A space, the highest byte C's signed `char` reads as white space: `c <= ' '`.
pub(crate) const SPACE: i8 = 32;

/// One token: its text and the byte offsets in the tokenized string it starts and ends at.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    text: Vec<u8>,
    start: usize,
    end: usize,
}

/// The token list, vkQuake's `qctoken` and `qctoken_count` (BUILTIN-18.23): one per engine,
/// kept from the tokenize call that filled it until the next.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    tokens: Vec<Token>,
}

impl Tokens {
    /// `tokenizeqc`: the list emptied, then each word `COM_Parse` reads from `text`, after
    /// the bytes up to `' '` (unsigned) before it, until the text ends, a word cannot be read
    /// or the list is full; returns how many (BUILTIN-18.23).
    pub fn tokenize(&mut self, text: &[u8]) -> usize {
        self.tokens.clear();
        let byte = |at: usize| text.get(at).copied().unwrap_or(0);
        let mut at = 0;
        while self.tokens.len() < MAX_QC_TOKENS {
            while byte(at) != 0 && byte(at) <= b' ' {
                at = at.saturating_add(1);
            }
            if byte(at) == 0 {
                break;
            }
            let start = at;
            let Some((word, next)) = com_parse(text, at) else {
                break;
            };
            self.tokens.push(Token {
                text: word,
                start,
                end: next,
            });
            at = next;
        }
        self.tokens.len()
    }

    /// `PF_tokenizebyseparator`: the list emptied, then `text` split at each of `separators`
    /// as vkQuake splits it, the byte after a separator never starting another; returns how
    /// many (BUILTIN-18.24).
    pub fn split(&mut self, text: &[u8], separators: &[Vec<u8>]) -> usize {
        self.tokens.clear();
        let byte = |at: usize| text.get(at).copied().unwrap_or(0);
        let mut start = 0;
        let mut at = 0;
        if byte(0) == 0 {
            return 0;
        }
        loop {
            let mut found = None;
            if byte(at) == 0 {
                found = Some(at);
            } else {
                // `strncmp (str, sep[s], seplen[s])`: the separator's bytes, where the text
                // ends at its first zero.
                let rest = text.get(at..).unwrap_or_default();
                let rest = rest.split(|&b| b == 0).next().unwrap_or_default();
                if let Some(sep) = separators.iter().find(|sep| rest.starts_with(sep)) {
                    found = Some(at);
                    at = at.saturating_add(sep.len());
                }
            }
            if let Some(end) = found {
                self.tokens.push(Token {
                    text: text.get(start..end).unwrap_or_default().to_vec(),
                    start,
                    end,
                });
                if byte(at) != 0 && self.tokens.len() < MAX_QC_TOKENS {
                    start = at;
                } else {
                    break;
                }
            }
            at = at.saturating_add(1);
        }
        self.tokens.len()
    }

    /// The token `PF_ArgV` and its index builtins name by `index`: a negative one counts from
    /// the end; `None` outside the list.
    fn get(&self, index: TokenIndex) -> Option<&Token> {
        let index = index.get();
        let count = i32::try_from(self.tokens.len()).unwrap_or(i32::MAX);
        let index = if index < 0 {
            index.wrapping_add(count)
        } else {
            index
        };
        usize::try_from(index)
            .ok()
            .and_then(|index| self.tokens.get(index))
    }

    /// `PF_ArgV`'s text of token `index`, cut to the 1023 bytes `q_strlcpy` copies into a
    /// temporary string; `None` outside the list, the null string.
    #[must_use]
    pub fn argv(&self, index: TokenIndex) -> Option<Vec<u8>> {
        self.get(index).map(|token| {
            let text = token.text.split(|&b| b == 0).next().unwrap_or_default();
            text.get(..text.len().min(STRINGTEMP_TEXT))
                .unwrap_or_default()
                .to_vec()
        })
    }

    /// `PF_argv_start_index`: the byte token `index` starts at, or -1.
    #[must_use]
    pub fn start(&self, index: TokenIndex) -> i64 {
        self.get(index)
            .map_or(-1, |token| i64::try_from(token.start).unwrap_or(i64::MAX))
    }

    /// `PF_argv_end_index`: the byte after token `index`, or -1.
    #[must_use]
    pub fn end(&self, index: TokenIndex) -> i64 {
        self.get(index)
            .map_or(-1, |token| i64::try_from(token.end).unwrap_or(i64::MAX))
    }
}

/// A token's index for `argv` and its edges: from 0, or from the end when negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenIndex(i32);

impl TokenIndex {
    /// Index `index`.
    #[must_use]
    pub const fn new(index: i32) -> Self {
        Self(index)
    }

    /// The index.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

/// C's `char`, signed on x86: `c = *data` makes a byte from 0x80 up negative, so it is
/// white space to `c <= ' '` and ends a word at `c > 32`.
pub(crate) const fn signed(byte: u8) -> i8 {
    i8::from_ne_bytes([byte])
}

/// `COM_ParseEx (data, CPE_NOTRUNC)` from byte `at` of `text`, whose first zero or end is the
/// string's end: the word read and the byte after it, or `None` at the end of the text or for a
/// word longer than `com_token` holds (BUILTIN-18.23).
pub(crate) fn com_parse(text: &[u8], mut at: usize) -> Option<(Vec<u8>, usize)> {
    let byte = |at: usize| text.get(at).copied().unwrap_or(0);
    loop {
        // skipwhite: `while ((c = *data) <= ' ')`, the end of the text an end of file.
        while signed(byte(at)) <= SPACE {
            if byte(at) == 0 {
                return None;
            }
            at = at.saturating_add(1);
        }
        let c = byte(at);
        if c == b'/' && byte(at.saturating_add(1)) == b'/' {
            while byte(at) != 0 && byte(at) != b'\n' {
                at = at.saturating_add(1);
            }
            continue;
        }
        if c == b'/' && byte(at.saturating_add(1)) == b'*' {
            at = at.saturating_add(2);
            while byte(at) != 0 && !(byte(at) == b'*' && byte(at.saturating_add(1)) == b'/') {
                at = at.saturating_add(1);
            }
            if byte(at) != 0 {
                at = at.saturating_add(2);
            }
            continue;
        }
        break;
    }
    let full = COM_PARSE_MAX_TOKEN_SIZE.saturating_sub(1);
    let mut word = Vec::new();
    let c = byte(at);
    if c == b'"' {
        at = at.saturating_add(1);
        loop {
            let c = byte(at);
            if c != 0 {
                at = at.saturating_add(1);
            }
            if c == b'"' || c == 0 {
                return Some((word, at));
            }
            if word.len() >= full {
                return None;
            }
            word.push(c);
        }
    }
    if matches!(c, b'{' | b'}' | b'(' | b')' | b'\'' | b':') {
        word.push(c);
        return Some((word, at.saturating_add(1)));
    }
    loop {
        if word.len() >= full {
            return None;
        }
        word.push(byte(at));
        at = at.saturating_add(1);
        let c = byte(at);
        // `':'` is no stop here, "so that ip:port works".
        if matches!(c, b'{' | b'}' | b'(' | b')' | b'\'') || signed(c) <= SPACE {
            break;
        }
    }
    Some((word, at))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::{MAX_QC_TOKENS, TokenIndex, Tokens, com_parse};

    fn words(text: &[u8]) -> Vec<Vec<u8>> {
        let mut tokens = Tokens::default();
        let count = tokens.tokenize(text);
        (0..count)
            .map(|i| {
                tokens
                    .argv(TokenIndex::new(i32::try_from(i).unwrap()))
                    .unwrap()
            })
            .collect()
    }

    // spec BUILTIN-18.23: tokenize splits at white space, keeps a quoted string whole, makes
    // each of `{ } ( ) ' :` a word of its own, skips comments, and a `:` inside a word stays
    #[test]
    fn tokenize_reads_words_as_com_parse_does() {
        assert_eq!(words(b"say hello"), [b"say".to_vec(), b"hello".to_vec()]);
        assert_eq!(
            words(b"  say \"two words\"  "),
            [b"say".to_vec(), b"two words".to_vec()]
        );
        assert_eq!(
            words(b"a(b)c"),
            [
                b"a".to_vec(),
                b"(".to_vec(),
                b"b".to_vec(),
                b")".to_vec(),
                b"c".to_vec()
            ]
        );
        assert_eq!(
            words(b"ip:27500 :x"),
            [b"ip:27500".to_vec(), b":".to_vec(), b"x".to_vec()]
        );
        assert_eq!(words(b"a // b c\nd"), [b"a".to_vec(), b"d".to_vec()]);
        assert_eq!(words(b"a /* b */ c"), [b"a".to_vec(), b"c".to_vec()]);
        assert_eq!(words(b"a /* never closed"), [b"a".to_vec()]);
        // A `/` that starts no comment is a word's byte, and so is a `*` after one; a `/` in a
        // block comment does not close it.
        assert_eq!(words(b"/x x*"), [b"/x".to_vec(), b"x*".to_vec()]);
        assert_eq!(words(b"/* a/ b */ c"), [b"c".to_vec()]);
        assert_eq!(words(b"\"unclosed"), [b"unclosed".to_vec()]);
        assert!(words(b"").is_empty());
        assert!(words(b"   ").is_empty());
        // A comment that ends the text leaves no word: COM_Parse returns NULL.
        assert_eq!(words(b"a //"), [b"a".to_vec()]);
        // A byte from 0x80 up is white space to COM_Parse (signed char) and ends a word.
        assert_eq!(words(b"a\xe9b"), [b"a".to_vec(), b"b".to_vec()]);
        // The text ends at its first zero.
        assert_eq!(words(b"a\0b"), [b"a".to_vec()]);
    }

    // spec BUILTIN-18.23: the start and end of each word, the end the byte COM_Parse stopped
    // at, a closing quote counted; a negative index counts from the end
    #[test]
    fn tokenize_records_where_each_word_is() {
        let mut tokens = Tokens::default();
        assert_eq!(tokens.tokenize(b" ab \"c d\" e"), 3);
        assert_eq!(
            (
                tokens.start(TokenIndex::new(0)),
                tokens.end(TokenIndex::new(0))
            ),
            (1, 3)
        );
        assert_eq!(
            (
                tokens.start(TokenIndex::new(1)),
                tokens.end(TokenIndex::new(1))
            ),
            (4, 9)
        );
        assert_eq!(
            (
                tokens.start(TokenIndex::new(-1)),
                tokens.end(TokenIndex::new(-1))
            ),
            (10, 11)
        );
        assert_eq!(tokens.argv(TokenIndex::new(-3)).unwrap(), b"ab");
        assert_eq!(tokens.argv(TokenIndex::new(3)), None);
        assert_eq!(tokens.argv(TokenIndex::new(-4)), None);
        assert_eq!(
            (
                tokens.start(TokenIndex::new(3)),
                tokens.end(TokenIndex::new(-4))
            ),
            (-1, -1)
        );
    }

    // spec BUILTIN-18.23: the list holds 64 words; the rest are not read
    #[test]
    fn tokenize_keeps_at_most_64_words() {
        let text = "w ".repeat(70);
        let mut tokens = Tokens::default();
        assert_eq!(tokens.tokenize(text.as_bytes()), MAX_QC_TOKENS);
        assert_eq!(tokens.tokenize(b"x"), 1);
    }

    // spec BUILTIN-18.23: a word longer than com_token holds ends the list (CPE_NOTRUNC), a
    // quoted one too; one byte less is read
    #[test]
    fn a_word_too_long_for_com_token_ends_the_list() {
        let long = vec![b'a'; 4096];
        assert_eq!(com_parse(&long, 0), None);
        let fits = vec![b'a'; 4095];
        assert_eq!(com_parse(&fits, 0).unwrap().0.len(), 4095);
        let mut quoted = vec![b'"'];
        quoted.extend(vec![b'a'; 4096]);
        assert_eq!(com_parse(&quoted, 0), None);
        let mut text = b"x ".to_vec();
        text.extend(&long);
        let mut tokens = Tokens::default();
        assert_eq!(tokens.tokenize(&text), 1);
        // argv copies at most 1023 bytes.
        assert_eq!(tokens.tokenize(&fits), 1);
        assert_eq!(tokens.argv(TokenIndex::new(0)).unwrap().len(), 1023);
    }

    // spec BUILTIN-18.24: tokenizebyseparator splits at each separator, the byte after one
    // never starting another, so `a,,b` is `a` and `,b`; a separator that ends the text ends
    // the list; no separator leaves the whole text one token; an empty text none
    #[test]
    fn split_follows_vkquakes_scan() {
        let split = |text: &[u8], seps: &[&[u8]]| {
            let seps: Vec<Vec<u8>> = seps.iter().map(|s| s.to_vec()).collect();
            let mut tokens = Tokens::default();
            let count = tokens.split(text, &seps);
            (0..count)
                .map(|i| {
                    tokens
                        .argv(TokenIndex::new(i32::try_from(i).unwrap()))
                        .unwrap()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            split(b"a,b,c", &[b","]),
            [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
        assert_eq!(split(b"a,,b", &[b","]), [b"a".to_vec(), b",b".to_vec()]);
        assert_eq!(split(b"a,", &[b","]), [b"a".to_vec()]);
        assert_eq!(
            split(b"a::b;c", &[b"::", b";"]),
            [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
        assert_eq!(split(b"abc", &[]), [b"abc".to_vec()]);
        assert!(split(b"", &[b","]).is_empty());
        // A separator past the text's end does not match.
        assert_eq!(split(b"a,", &[b",,"]), [b"a,".to_vec()]);
        let mut tokens = Tokens::default();
        tokens.split(b"ab,cd", &[b",".to_vec()]);
        assert_eq!(
            (
                tokens.start(TokenIndex::new(1)),
                tokens.end(TokenIndex::new(1))
            ),
            (3, 5)
        );
        let many = ",".repeat(200);
        let text = format!("x{many}");
        assert!(tokens.split(text.as_bytes(), &[b",".to_vec()]) <= MAX_QC_TOKENS);
    }

    // spec BUILTIN-18.23: tokenizeqc's own skip reads bytes as unsigned, so a word after a
    // byte from 0x80 up starts at that byte, which COM_Parse then skips; a text of such bytes
    // alone has no word (P2-R1B review)
    #[test]
    fn reviewer_a_high_byte_before_a_word_is_its_start() {
        let mut tokens = Tokens::default();
        assert_eq!(tokens.tokenize(b" \xe9ab"), 1);
        assert_eq!(
            (
                tokens.start(TokenIndex::new(0)),
                tokens.end(TokenIndex::new(0))
            ),
            (1, 4)
        );
        assert_eq!(tokens.argv(TokenIndex::new(0)).unwrap(), b"ab");
        assert_eq!(tokens.tokenize(b"\xe9\xff"), 0);
        // A quoted word's end is past its closing quote; an unclosed one's is the text's end.
        assert_eq!(tokens.tokenize(b"\"a b\"c \"d"), 3);
        assert_eq!(tokens.argv(TokenIndex::new(1)).unwrap(), b"c");
        assert_eq!(
            (
                tokens.start(TokenIndex::new(2)),
                tokens.end(TokenIndex::new(2))
            ),
            (7, 9)
        );
    }

    // spec BUILTIN-18.24: an empty separator matches at every byte, so each byte is a token
    // after an empty first one; the list stops at exactly 64 tokens, the last ending at a
    // separator; the first separator listed that matches is taken, not the longest (P2-R1B
    // review)
    #[test]
    fn reviewer_split_with_an_empty_separator_and_at_the_cap() {
        let mut tokens = Tokens::default();
        assert_eq!(tokens.split(b"abc", &[Vec::new()]), 4);
        let all: Vec<Vec<u8>> = (0..4)
            .map(|i| tokens.argv(TokenIndex::new(i)).unwrap())
            .collect();
        assert_eq!(
            all,
            [b"".to_vec(), b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
        let text = "ab,".repeat(70);
        assert_eq!(
            tokens.split(text.as_bytes(), &[b",".to_vec()]),
            MAX_QC_TOKENS
        );
        assert_eq!(tokens.argv(TokenIndex::new(-1)).unwrap(), b"ab");
        assert_eq!(
            (
                tokens.start(TokenIndex::new(-1)),
                tokens.end(TokenIndex::new(-1))
            ),
            (189, 191)
        );
        assert_eq!(tokens.split(b"a::b", &[b":".to_vec(), b"::".to_vec()]), 2);
        assert_eq!(tokens.argv(TokenIndex::new(1)).unwrap(), b":b");
    }

    // spec BUILTIN-18.23: a comment is passed over by COM_Parse, not by tokenizeqc's own skip,
    // so the word after it starts at the comment; an unclosed block comment ends the list
    // (P2-R1B re-review)
    #[test]
    fn rereview_a_comment_is_part_of_the_next_word() {
        let mut tokens = Tokens::default();
        assert_eq!(tokens.tokenize(b"a // c\nb"), 2);
        assert_eq!(tokens.argv(TokenIndex::new(1)).unwrap(), b"b");
        assert_eq!(
            (
                tokens.start(TokenIndex::new(1)),
                tokens.end(TokenIndex::new(1))
            ),
            (2, 8)
        );
        assert_eq!(tokens.tokenize(b"x /**/y"), 2);
        assert_eq!(
            (
                tokens.start(TokenIndex::new(1)),
                tokens.end(TokenIndex::new(1))
            ),
            (2, 7)
        );
        assert_eq!(tokens.tokenize(b"a /* x"), 1);
        assert_eq!(tokens.tokenize(b"a //"), 1);
    }

    // spec BUILTIN-18.23: a quoted word holds up to 4095 bytes, com_token less its zero; one
    // more is COM_Parse's NULL and ends the list; an unclosed quote ends at the text's end
    // (P2-R1B re-review)
    #[test]
    fn rereview_a_quoted_word_at_the_com_token_limit() {
        let mut tokens = Tokens::default();
        let fits = format!("\"{}\" b", "q".repeat(4095));
        assert_eq!(tokens.tokenize(fits.as_bytes()), 2);
        assert_eq!(
            tokens.argv(TokenIndex::new(0)).unwrap(),
            "q".repeat(1023).as_bytes()
        );
        assert_eq!(
            (
                tokens.start(TokenIndex::new(1)),
                tokens.end(TokenIndex::new(1))
            ),
            (4098, 4099)
        );
        let over = format!("\"{}\" b", "q".repeat(4096));
        assert_eq!(tokens.tokenize(over.as_bytes()), 0);
        assert_eq!(tokens.tokenize(b"a \"bc"), 2);
        assert_eq!(tokens.argv(TokenIndex::new(1)).unwrap(), b"bc");
        assert_eq!(tokens.end(TokenIndex::new(1)), 5);
    }

    // spec BUILTIN-18.24: a separator that is the whole text makes one empty token; one longer
    // than what is left never matches (P2-R1B re-review)
    #[test]
    fn rereview_split_at_a_separator_as_long_as_the_text() {
        let mut tokens = Tokens::default();
        assert_eq!(tokens.split(b"ab", &[b"ab".to_vec()]), 1);
        assert_eq!(tokens.argv(TokenIndex::new(0)).unwrap(), b"");
        assert_eq!(
            (
                tokens.start(TokenIndex::new(0)),
                tokens.end(TokenIndex::new(0))
            ),
            (0, 0)
        );
        assert_eq!(tokens.split(b"a,", &[b",,".to_vec()]), 1);
        assert_eq!(tokens.argv(TokenIndex::new(0)).unwrap(), b"a,");
        assert_eq!(tokens.split(b"", &[b",".to_vec()]), 0);
        assert_eq!(tokens.argv(TokenIndex::new(0)), None);
    }
}
