// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's string literals, as its `QCC_PR_LexString` and `QCC_PR_LexEscapedCodepoint` read
//! them (`docs/spec/dialect-fteqcc.md`, FTEF-13.7 to FTEF-13.11): the prefixes `R`, `Q`, `u8`,
//! `u`, `U` and, after a string, `L`; raw strings; the escapes; and the bytes each kind of
//! string writes. Written from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e
//! (`engine/qclib/qcc_pr_lex.c`), which is not a porting upstream, and checked against the
//! pinned FTEQCC by `cargo xtask oracles builtins`.

use crate::{Code, Dialect, Lexed, Lexer, Span, Token, TokenKind};

/// The bytes of `text`, one part of an FTEQCC string literal as the lexer made its token, an
/// `L"..."` after another part included (FTEF-13.7); `None` when `text` is not one such part.
#[must_use]
pub fn fteqcc_string_bytes(text: &[u8]) -> Option<Vec<u8>> {
    let mut lexer = Lexer {
        text,
        pos: 0,
        dialect: Dialect::Fteqcc,
        out: Lexed::default(),
    };
    let prefix = lexer.fte_string_prefix(true)?;
    lexer.fte_string(prefix);
    if lexer.pos != text.len() {
        return None;
    }
    match lexer.out.tokens.pop() {
        Some(Token {
            kind: TokenKind::String(bytes),
            ..
        }) => Some(bytes),
        _ => None,
    }
}

/// What a string's characters are written as: `QCC_PR_LexString`'s `stringtype`
/// (FTEF-13.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Output {
    /// 0, "quake output, input is 8bit": a plain string, the default with no `-Futf8`, and
    /// `R"..."`.
    Quake,
    /// 1, "quake output, input is utf-8": `Q"..."`.
    QuakeFromUtf8,
    /// 2, "utf-8 output, input is utf-8": `u8"..."`, `u"..."`, `U"..."` and `L"..."`.
    Utf8,
}

/// The prefix of a string literal (FTEF-13.7): its length before the `"` and how its
/// characters are written. `raw` for `R`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Prefix {
    len: usize,
    output: Output,
    raw: bool,
}

/// `Bronze` toggled by `\s` and `\b`: `QCC_PR_LexString`'s `texttype ^= 0xe080` (FTEF-13.10).
const BRONZE: u32 = 0xe080;

/// The longest raw string delimiter: `char rawdelim[64]`, filled from index 1 and refused
/// `if (!c || raw >= sizeof(rawdelim)-1)` (FTEF-13.9).
const MAX_RAW_DELIMITER: usize = 62;

impl Lexer<'_> {
    /// The string prefix at the current byte, if a string starts here under FTEQCC
    /// (FTEF-13.7): `QCC_PR_Lex` reads `"`, `R"`, `Q"`, `u"`, `U"` and `u8"` as a string, and
    /// `QCC_PR_LexString` an `L"` too, but only after another part of the same string, which
    /// `joins` says.
    pub(crate) fn fte_string_prefix(&self, joins: bool) -> Option<Prefix> {
        let prefix = |len, output, raw| Some(Prefix { len, output, raw });
        match (self.peek(0)?, self.peek(1), self.peek(2)) {
            (b'"', _, _) => prefix(0, Output::Quake, false),
            (b'R', Some(b'"'), _) => prefix(1, Output::Quake, true),
            (b'Q', Some(b'"'), _) => prefix(1, Output::QuakeFromUtf8, false),
            (b'u' | b'U', Some(b'"'), _) => prefix(1, Output::Utf8, false),
            (b'L', Some(b'"'), _) if joins => prefix(1, Output::Utf8, false),
            (b'u', Some(b'8'), Some(b'"')) => prefix(2, Output::Utf8, false),
            _ => None,
        }
    }

    /// One part of an FTEQCC string literal at its prefix (FTEF-13.7 to FTEF-13.11), pushed as
    /// a string token of the bytes FTEQCC writes for it. Adjacent parts are joined by the
    /// parser (FTEF-16.8).
    pub(crate) fn fte_string(&mut self, prefix: Prefix) {
        let start = self.pos;
        self.bump(prefix.len.saturating_add(1));
        let contents = if prefix.raw {
            self.raw_string(start)
        } else {
            self.cooked_string(start, prefix.output)
        };
        self.check_length(contents.len(), Span::new(start, self.pos));
        self.push(TokenKind::String(contents), start);
    }

    /// The rest of a raw string after its `R"` (FTEF-13.9): a delimiter up to `(`, an optional
    /// `\` and line break, then every byte as it is up to `)`, the delimiter and `"`.
    fn raw_string(&mut self, start: usize) -> Vec<u8> {
        let mut delimiter = Vec::new();
        loop {
            match self.peek(0) {
                Some(b'(') => {
                    self.bump(1);
                    break;
                }
                Some(b) if delimiter.len() < MAX_RAW_DELIMITER => {
                    delimiter.push(b);
                    self.bump(1);
                }
                _ => {
                    self.error(
                        Code::E0101,
                        "EOF while parsing raw string delimiter. Expected: R\"delim(string)delim\"",
                        Span::new(start, self.pos),
                    );
                    return Vec::new();
                }
            }
        }
        for skip in [&b"\\\r\n"[..], b"\\\r", b"\\\n"] {
            if self
                .text
                .get(self.pos..)
                .is_some_and(|rest| rest.starts_with(skip))
            {
                self.bump(skip.len());
                break;
            }
        }
        let mut end = vec![b')'];
        end.extend_from_slice(&delimiter);
        end.push(b'"');
        let mut contents = Vec::new();
        loop {
            let rest = self.text.get(self.pos..).unwrap_or_default();
            if rest.starts_with(&end) {
                self.bump(end.len());
                return contents;
            }
            let Some(&b) = rest.first() else {
                self.error(Code::E0101, "EOF inside quote", Span::new(start, self.pos));
                return contents;
            };
            contents.push(b);
            self.bump(1);
        }
    }

    /// The rest of a string that is not raw, after its `"`, written as `output` says
    /// (FTEF-13.8, FTEF-13.10, FTEF-13.11).
    fn cooked_string(&mut self, start: usize, output: Output) -> Vec<u8> {
        let mut contents = Vec::new();
        let mut bronze = 0_u32;
        loop {
            let Some(b) = self.peek(0) else {
                self.error(Code::E0101, "EOF inside quote", Span::new(start, self.pos));
                return contents;
            };
            match b {
                b'\n' => {
                    self.error(
                        Code::E0102,
                        "newline inside quote",
                        Span::new(start, self.pos),
                    );
                    return contents;
                }
                b'"' => {
                    self.bump(1);
                    return contents;
                }
                b'\\' => {
                    let escape = self.pos;
                    self.bump(1);
                    match self.peek(0) {
                        Some(b's' | b'b') => {
                            self.bump(1);
                            bronze ^= BRONZE;
                        }
                        Some(b'.') => {
                            self.bump(1);
                            write(&mut contents, 0xe01c | bronze, output);
                        }
                        // `\u` is "a utf-8 code regardless of output encoding", `\x` "an
                        // explicit byte regardless of encoding".
                        Some(b'u' | b'U') => {
                            let code = self.fte_escape(start, escape);
                            utf8(&mut contents, code);
                        }
                        Some(b'x' | b'X') => {
                            let code = self.fte_escape(start, escape);
                            contents.push(low_byte(code));
                        }
                        Some(_) => {
                            let code = self.fte_escape(start, escape);
                            write(&mut contents, code, output);
                        }
                        None => {
                            self.error(Code::E0101, "EOF inside quote", Span::new(start, self.pos));
                            return contents;
                        }
                    }
                }
                _ => self.character_in_string(&mut contents, b, bronze, output),
            }
        }
    }

    /// A byte of a string that starts no escape, and the UTF-8 sequence it starts when the
    /// input is UTF-8 (FTEF-13.8, FTEF-13.10).
    fn character_in_string(&mut self, contents: &mut Vec<u8>, b: u8, bronze: u32, output: Output) {
        let sequence = match output {
            Output::Quake => None,
            Output::QuakeFromUtf8 | Output::Utf8 => {
                utf8_check(self.text.get(self.pos..).unwrap_or_default())
            }
        };
        let Some((code, len)) = sequence else {
            // `if (c >= ' ') c |= texttype;`, then the byte as it is (`forcebyte`).
            self.bump(1);
            contents.push(if b >= b' ' { b | low_byte(bronze) } else { b });
            return;
        };
        if bronze == 0 || code < u32::from(b' ') {
            self.bump(len);
            write(contents, code, output);
        } else if code < 0x80 {
            self.bump(len);
            // `cp|0xe080`: below 0x80 the code shares no bit with it, so the sum is the same.
            write(contents, BRONZE.wrapping_add(code), output);
        } else {
            // "Unable to mask non-ascii chars. Attempting to mask bytes": the first byte goes
            // as it is, and the rest of the sequence is read again, one byte at a time.
            self.bump(1);
            contents.push(b);
        }
    }

    /// The code of the escape whose `\` is at `escape`, the byte after it current, as
    /// `QCC_PR_LexEscapedCodepoint` reads it (FTEF-13.11), in a string or a character
    /// constant (FTEF-13.13) starting at `start`. An error is reported at the escape and the
    /// code read so far returned.
    pub(crate) fn fte_escape(&mut self, start: usize, escape: usize) -> u32 {
        let Some(c) = self.peek(0) else {
            self.error(Code::E0101, "EOF inside quote", Span::new(start, self.pos));
            return 0;
        };
        self.bump(1);
        match c {
            b'n' | b'\n' => u32::from(b'\n'),
            b'r' => u32::from(b'\r'),
            b't' => u32::from(b'\t'),
            b'a' => 0x07,
            b'v' => 0x0b,
            b'f' => 0x0c,
            b'#' | b'"' | b'\\' | b'?' | b'\'' => u32::from(c),
            b'[' => 0xe010,
            b']' => 0xe011,
            // A string reads `\.` before it gets here, to give it bronze (FTEF-13.10).
            b'.' => 0xe01c,
            b'<' => 0xe01d,
            b'-' => 0xe01e,
            b'>' => 0xe01f,
            b'(' => 0xe080,
            b'=' => 0xe081,
            b')' => 0xe082,
            b'+' => 0xe083,
            b'0'..=b'9' => 0xe012_u32.wrapping_add(u32::from(c.wrapping_sub(b'0'))),
            b'{' => self.fte_braced(escape),
            b'u' => self.fte_hex(escape, 4),
            b'U' => self.fte_hex(escape, 8),
            b'x' | b'X' => {
                let Some(high) = self.peek(0).and_then(|d| char::from(d).to_digit(16)) else {
                    self.bad_character_code(escape);
                    return 0;
                };
                self.bump(1);
                match self.peek(0).and_then(|d| char::from(d).to_digit(16)) {
                    Some(low) => {
                        self.bump(1);
                        high.wrapping_mul(16).wrapping_add(low)
                    }
                    None => high,
                }
            }
            // A `\` before CR is the byte after the CR, which should be a LF.
            b'\r' => match self.peek(0) {
                Some(next) => {
                    self.bump(1);
                    u32::from(next)
                }
                None => {
                    self.error(Code::E0101, "EOF inside quote", Span::new(start, self.pos));
                    0
                }
            },
            _ => {
                self.error(
                    Code::E0103,
                    "Unknown escape char",
                    Span::new(escape, self.pos),
                );
                u32::from(c)
            }
        }
    }

    /// `\{n}` or `\{xN}`, the `{` read: digits up to the `}`; any other byte is "Bad character
    /// code" (FTEF-13.11).
    fn fte_braced(&mut self, escape: usize) -> u32 {
        let radix = if self.peek(0) == Some(b'x') {
            self.bump(1);
            16
        } else {
            10
        };
        let mut code = 0_u32;
        loop {
            match self.peek(0) {
                Some(b'}') => {
                    self.bump(1);
                    return code;
                }
                Some(d) if char::from(d).is_digit(radix) => {
                    let digit = char::from(d).to_digit(radix).unwrap_or(0);
                    code = code.wrapping_mul(radix).wrapping_add(digit);
                    self.bump(1);
                }
                _ => {
                    self.bad_character_code(escape);
                    return code;
                }
            }
        }
    }

    /// Up to `count` hex digits of the `\u` or `\U` at `escape` (FTEF-13.11). FTEQCC's
    /// `while (nibbles --> 0)` leaves `nibbles` nonzero, and so reports "Unicode character
    /// terminated unexpectedly", unless the digits stop one short of `count`.
    fn fte_hex(&mut self, escape: usize, count: usize) -> u32 {
        let mut code = 0_u32;
        let mut read = 0_usize;
        while read < count {
            let Some(digit) = self.peek(0).and_then(|b| char::from(b).to_digit(16)) else {
                break;
            };
            code = code.wrapping_mul(16).wrapping_add(digit);
            self.bump(1);
            read = read.saturating_add(1);
        }
        if read.saturating_add(1) != count {
            self.error(
                Code::E0103,
                "Unicode character terminated unexpectedly",
                Span::new(escape, self.pos),
            );
        }
        code
    }

    /// FTEQCC's "Bad character code" (`ERR_BADCHARACTERCODE`), reported as an unknown escape
    /// (E0103, FTEF-13.11).
    fn bad_character_code(&mut self, escape: usize) {
        self.error(
            Code::E0103,
            "Bad character code",
            Span::new(escape, self.pos),
        );
    }
}

/// The low byte of a code, as `pr_token[len] = c;` stores it in a `char`.
fn low_byte(code: u32) -> u8 {
    code.to_le_bytes().first().copied().unwrap_or(0)
}

/// Writes `code` as `output` says (FTEF-13.8): as UTF-8 (`forceutf8`), or as a Quake
/// character (`forcequake`): a code from 0xE000 to 0xE0FF is its low byte, Quake's glyph,
/// and any other its low byte too, a code above 0x7F with FTEQCC's warning "Cannot convert
/// codepoint %#x to quake's charset".
fn write(contents: &mut Vec<u8>, code: u32, output: Output) {
    match output {
        Output::Utf8 => utf8(contents, code),
        Output::Quake | Output::QuakeFromUtf8 => contents.push(low_byte(code)),
    }
}

/// `code` as FTEQCC's `forceutf8` writes it (FTEF-13.8): as many bytes as its size needs, up
/// to 7, with no check that it is a Unicode scalar: a lead byte of as many high bits as there
/// are bytes, then 6 bits in each continuation byte. A code of 0x80000000 or more makes FTEQCC
/// loop for ever (its `t <<= 5` reaches 0); it is written here as 7 bytes.
fn utf8(contents: &mut Vec<u8>, code: u32) {
    let code = u64::from(code);
    let mut count = 1_u32;
    let mut bound = 0x80_u64;
    while code >= bound {
        bound <<= match count {
            1 => 4,
            2..=6 => 5,
            _ => 6,
        };
        count = count.saturating_add(1);
    }
    if count == 1 {
        contents.push(low_byte(u32::try_from(code).unwrap_or(0)));
        return;
    }
    let mut shift = count.saturating_mul(6).saturating_sub(6);
    // The lead byte's high bits, `0xffffff00 >> bytecount`, and the code's top bits, which
    // `bound` keeps below them, so adding them sets both.
    let lead = (0xff00_u64 >> count).wrapping_add(code.checked_shr(shift).unwrap_or(0));
    contents.push(lead.to_le_bytes().first().copied().unwrap_or(0));
    while shift > 0 {
        shift = shift.saturating_sub(6);
        let bits = code.checked_shr(shift).unwrap_or(0) & 0x3f;
        contents.push(low_byte(u32::try_from(bits).unwrap_or(0)).wrapping_add(0x80));
    }
}

/// The code and length of the UTF-8 sequence at the start of `bytes`, as FTEQCC's
/// `utf8_check` reads it (FTEF-13.8): two bytes for 0 (modified UTF-8) or 0x80 to 0x7FF, three
/// for 0x800 to 0xFFFF, surrogates included, four for 0x10000 to 0x10FFFF; `None` for any
/// other byte of 0x80 or more.
pub(crate) fn utf8_check(bytes: &[u8]) -> Option<(u32, usize)> {
    let lead = *bytes.first()?;
    let (len, bits, least) = match lead {
        0x00..=0x7f => return Some((u32::from(lead), 1)),
        0xc0..=0xdf => (2, lead & 0x1f, 0x80),
        0xe0..=0xef => (3, lead & 0x0f, 0x800),
        0xf0..=0xf7 => (4, lead & 0x07, 0x1_0000),
        _ => return None,
    };
    let mut code = u32::from(bits);
    for i in 1..len {
        let next = *bytes.get(i)?;
        if !(0x80..0xc0).contains(&next) {
            return None;
        }
        code = code.wrapping_mul(64).wrapping_add(u32::from(next & 0x3f));
    }
    // `if (!uc || uc >= (1u<<7))`, `if (uc >= (1u<<11))`, `if (uc >= (1u<<16)) if (uc <=
    // 0x10ffff)`.
    let fits = code >= least && code <= 0x10_ffff;
    (fits || (len == 2 && code == 0)).then_some((code, len))
}
