// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_LexString, QCC_PR_LexEscapedCodepoint

//! FTEQCC's string literals, as its `QCC_PR_LexString` and `QCC_PR_LexEscapedCodepoint` read
//! them (`docs/spec/dialect-fteqcc.md`, FTEF-13.7 to FTEF-13.11): the prefixes `R`, `Q`, `u8`,
//! `u`, `U` and, after a string, `L`; raw strings; the escapes; and the bytes each kind of
//! string writes. Ported from FTEQCC's behaviour under ADR-0017's clean-port policy, written
//! fresh in this lexer, and checked against the pinned FTEQCC by `cargo xtask oracles builtins`.

use crate::{Code, Dialect, Lexed, Lexer, Span, TokenKind, is_fteqcc_whitespace};

/// The bytes of `text`, one part of an FTEQCC string literal as the lexer made its token, an
/// `L"..."` after another part included (FTEF-13.7); `None` when `text` is not one such part.
#[must_use]
pub fn fteqcc_string_bytes(text: &[u8]) -> Option<Vec<u8>> {
    let mut lexer = Lexer {
        text,
        pos: 0,
        dialect: Dialect::Fteqcc,
        flags: crate::LexFlags::default(),
        marks: &[],
        macros: &|_| None,
        out: Lexed::default(),
        joined: 0,
    };
    let prefix = lexer.fte_string_prefix(true)?;
    let bytes = lexer.fte_string_contents(prefix);
    (lexer.pos == text.len()).then_some(bytes)
}

/// The bytes that end a `#NAME` in a string under `macroinstrings`, besides white space
/// (FTEF-12.12): `QCC_PR_LexString`'s list.
const STRING_MACRO_STOPS: &[u8] = b")(+-*/\\|&=^~[]\"{};:,.#";

/// The most times this profile looks up a macro's text as a name for a `#NAME` in a string: a
/// deliberate divergence, as `QCC_PR_CheckCompConstString` recurses with no bound and so never
/// returns for a macro whose text names itself (FTEF-12.12).
const FTEQCC_MAX_STRING_MACRO_LOOKUPS: usize = 64;

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

/// The most bytes an FTEQCC string holds, its parts joined: `pr_token[8192]` less its NUL
/// (FTEF-13.20).
pub const FTEQCC_MAX_STRING: usize = crate::FTEQCC_MAX_TOKEN - 1;

/// FTEQCC's error for a longer one: `QCC_Error(ERR_INVALIDSTRINGIMMEDIATE, "String length
/// exceeds %u", (unsigned)sizeof(pr_token)-1);` (FTEF-13.20).
pub const STRING_TOO_LONG: &str = "String length exceeds 8191";

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
            // FTEF-12.12: `stringtype = flag_utf8strings?2:0;`.
            (b'"', _, _) if self.flags.has(crate::LexFlag::Utf8Strings) => {
                prefix(0, Output::Utf8, false)
            }
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
    ///
    /// `joins` says the part follows another, which FTEQCC reads into the same token, so the
    /// bytes of both count towards its 8191 (FTEF-13.20): the error is at the first part that
    /// takes the total past them.
    pub(crate) fn fte_string(&mut self, prefix: Prefix, joins: bool) {
        let start = self.pos;
        let before = if joins { self.joined } else { 0 };
        // The parts before this one, which a macro's text in it counts on from (FTEF-12.12).
        self.joined = before;
        let contents = self.fte_string_contents(prefix);
        self.joined = before.saturating_add(contents.len());
        if self.joined > FTEQCC_MAX_STRING && before <= FTEQCC_MAX_STRING {
            self.error(Code::E0502, STRING_TOO_LONG, Span::new(start, self.pos));
        }
        self.push(TokenKind::String(contents), start);
    }

    /// The bytes FTEQCC writes for the string literal part at its prefix (FTEF-13.7 to
    /// FTEF-13.11), read up to its end.
    fn fte_string_contents(&mut self, prefix: Prefix) -> Vec<u8> {
        let start = self.pos;
        // FTEF-13.22: `QCC_PR_ParseWarning(WARN_NOTUTF8, "char32_t/char16_t/wchar_t strings are
        // not supported, treating as u8 prefix (as utf-8)");` for `U`, `u` and `L`.
        if prefix.output == Output::Utf8 && prefix.len == 1 {
            self.error(
                Code::W0119,
                "char32_t/char16_t/wchar_t strings are not supported, treating as u8 prefix (as utf-8)",
                Span::new(start, start.saturating_add(1)),
            );
        }
        self.bump(prefix.len.saturating_add(1));
        if prefix.raw {
            self.raw_string(start)
        } else {
            self.cooked_string(start, prefix.output)
        }
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
                b'#' if self.flags.has(crate::LexFlag::MacroInStrings) => {
                    self.string_macro(&mut contents, output);
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
                            self.write_code(&mut contents, 0xe01c | bronze, output, escape);
                        }
                        // `\u` is "a utf-8 code regardless of output encoding", `\x` "an
                        // explicit byte regardless of encoding".
                        Some(b'u' | b'U') => {
                            let code = self.fte_escape(start, escape);
                            self.utf8_code(&mut contents, code, escape);
                        }
                        Some(b'x' | b'X') => {
                            let code = self.fte_escape(start, escape);
                            contents.push(low_byte(code));
                        }
                        Some(_) => {
                            let code = self.fte_escape(start, escape);
                            self.write_code(&mut contents, code, output, escape);
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

    /// `#NAME` in a string under `macroinstrings`, at its `#` (FTEF-12.12): the name runs to
    /// white space or one of FTEQCC's stop bytes, and is looked up as
    /// `QCC_PR_CheckCompConstString` does, a macro's text looked up again as a name for as long as
    /// it names one. The text found is written as it is, its trailing spaces and tabs dropped,
    /// with "Macro NAME expansion in string"; a name of no macro is "Unable to expand string macro
    /// NAME" unless it is empty, and the `#` is written as a byte with no mask.
    fn string_macro(&mut self, contents: &mut Vec<u8>, output: Output) {
        let hash = self.pos;
        let name_start = hash.saturating_add(1);
        let rest = self.text.get(name_start..).unwrap_or_default();
        let len = rest
            .iter()
            .position(|&b| is_fteqcc_whitespace(b) || STRING_MACRO_STOPS.contains(&b))
            .unwrap_or(rest.len());
        let name = rest.get(..len).unwrap_or_default().to_vec();
        let name_end = name_start.saturating_add(len);
        let mut value = None;
        for _ in 0..FTEQCC_MAX_STRING_MACRO_LOOKUPS {
            match (self.macros)(value.as_deref().unwrap_or(&name)) {
                Some(text) => value = Some(text),
                None => break,
            }
        }
        let shown = String::from_utf8_lossy(&name).into_owned();
        let Some(mut value) = value else {
            if !name.is_empty() {
                self.error(
                    Code::W0122,
                    format!("Unable to expand string macro {shown}"),
                    Span::new(hash, name_end),
                );
            }
            self.bump(1);
            self.write_code(contents, u32::from(b'#'), output, hash);
            return;
        };
        self.error(
            Code::W0122,
            format!("Macro {shown} expansion in string"),
            Span::new(hash, name_end),
        );
        let total = self.joined.saturating_add(contents.len());
        if total.saturating_add(value.len()) >= FTEQCC_MAX_STRING {
            self.error(Code::E0502, STRING_TOO_LONG, Span::new(hash, name_end));
        } else {
            while value.last().is_some_and(|&b| b == b' ' || b == b'\t') {
                value.pop();
            }
            contents.extend_from_slice(&value);
        }
        self.pos = name_end;
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
            // FTEF-13.21: `if (stringtype) QCC_PR_ParseWarning(ERR_BADCHARACTERCODE, "Input
            // string is not valid utf-8");`, an error, as the code is an `ERR_` one.
            if output != Output::Quake {
                let at = self.pos;
                self.error(
                    Code::E0103,
                    "Input string is not valid utf-8",
                    Span::new(at, at.saturating_add(1)),
                );
            }
            // `if (c >= ' ') c |= texttype;`, then the byte as it is (`forcebyte`).
            self.bump(1);
            contents.push(if b >= b' ' { b | low_byte(bronze) } else { b });
            return;
        };
        let at = self.pos;
        if bronze == 0 || code < u32::from(b' ') {
            self.bump(len);
            self.write_code(contents, code, output, at);
        } else if code < 0x80 {
            self.bump(len);
            // `cp|0xe080`: below 0x80 the code shares no bit with it, so the sum is the same.
            self.write_code(contents, BRONZE.wrapping_add(code), output, at);
        } else {
            // "Unable to mask non-ascii chars. Attempting to mask bytes", an error (FTEF-13.21):
            // the first byte goes as it is, and the rest of the sequence is read again, one
            // byte at a time.
            let at = self.pos;
            self.error(
                Code::E0103,
                "Unable to mask non-ascii chars. Attempting to mask bytes",
                Span::new(at, at.saturating_add(len)),
            );
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
            // "yellow numbers" under `flag_qcfuncs`, on by default; C's octal escape without it
            // (FTEF-12.12).
            b'0'..=b'9' if self.flags.has(crate::LexFlag::QcFuncs) => {
                0xe012_u32.wrapping_add(u32::from(c.wrapping_sub(b'0')))
            }
            b'0'..=b'9' => self.fte_octal(c, escape),
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
            // A `\` before CR is the byte after the CR, which should be a LF: `if (c != '\n')
            // QCC_PR_ParseWarning(WARN_HANGINGSLASHR, "Hanging \\\\\r");` (FTEF-13.25).
            b'\r' => match self.peek(0) {
                Some(next) => {
                    self.bump(1);
                    if next != b'\n' {
                        self.error(Code::W0123, "Hanging \\\\\r", Span::new(escape, self.pos));
                    }
                    u32::from(next)
                }
                None => {
                    self.error(Code::E0101, "EOF inside quote", Span::new(start, self.pos));
                    0
                }
            },
            _ => {
                // `QCC_PR_ParseError (ERR_INVALIDSTRINGIMMEDIATE, "Unknown escape char %c", c);`
                self.error(
                    Code::E0103,
                    format!("Unknown escape char {}", char::from(c)),
                    Span::new(escape, self.pos),
                );
                u32::from(c)
            }
        }
    }

    /// C's octal escape whose first digit `first` is read, at `escape`, as FTEQCC reads it without
    /// `qcfuncs` (FTEF-12.12): `8` or `9` first is "Bad character code \\%c", and up to two more
    /// octal digits follow.
    fn fte_octal(&mut self, first: u8, escape: usize) -> u32 {
        if first >= b'8' {
            self.error(
                Code::E0103,
                format!("Bad character code \\{}", char::from(first)),
                Span::new(escape, self.pos),
            );
            return 0;
        }
        let mut code = u32::from(first.wrapping_sub(b'0'));
        for _ in 0..2 {
            match self.peek(0) {
                Some(d @ b'0'..=b'7') => {
                    code = code
                        .wrapping_mul(8)
                        .wrapping_add(u32::from(d.wrapping_sub(b'0')));
                    self.bump(1);
                }
                _ => break,
            }
        }
        code
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

    /// Writes `code`, read at `at`, as `output` says (FTEF-13.8): as UTF-8 (`forceutf8`,
    /// [`Self::utf8_code`], with its F301 warning), or as a Quake character (`forcequake`), its
    /// low byte, a code from 0xE000 to 0xE0FF being Quake's glyph, with FTEQCC's F301 warning
    /// "Cannot convert codepoint %#x to quake's charset" for a code above 0x7F outside them
    /// (FTEF-13.22).
    fn write_code(&mut self, contents: &mut Vec<u8>, code: u32, output: Output, at: usize) {
        match output {
            Output::Utf8 => self.utf8_code(contents, code, at),
            Output::Quake | Output::QuakeFromUtf8 => {
                if code >= 0x80 && !(0xe000..=0xe0ff).contains(&code) {
                    self.error(
                        Code::W0119,
                        format!("Cannot convert codepoint {code:#x} to quake's charset"),
                        Span::new(at, self.pos.max(at.saturating_add(1))),
                    );
                }
                contents.push(low_byte(code));
            }
        }
    }

    /// Writes `code`, read at `at`, as UTF-8 ([`utf8`]), with FTEQCC's F301 warning "Bad unicode
    /// character code - codepoint is above 0x10FFFFu" past U+10FFFF (FTEF-13.22).
    fn utf8_code(&mut self, contents: &mut Vec<u8>, code: u32, at: usize) {
        if code > 0x10_ffff {
            self.error(
                Code::W0119,
                "Bad unicode character code - codepoint is above 0x10FFFFu",
                Span::new(at, self.pos.max(at.saturating_add(1))),
            );
        }
        utf8(contents, code);
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

/// `code` as FTEQCC's `forceutf8` writes it (FTEF-13.8): as many bytes as its size needs, up
/// to 7, with no check that it is a Unicode scalar: a lead byte of as many high bits as there
/// are bytes, then 6 bits in each continuation byte. A code of 0x80000000 or more makes FTEQCC
/// loop for ever (its `t <<= 5` reaches 0); it is written here as 7 bytes.
fn utf8(contents: &mut Vec<u8>, code: u32) {
    let code = u64::from(code);
    let mut count = 1_u32;
    let mut bound = 0x80_u64;
    while code >= bound {
        // FTEQCC's `else t <<= 6;`, for 7 bytes or more, is reached only in its endless loop:
        // a code is below 2^32 and the bound for 7 bytes is 2^36, so this loop ends there.
        bound <<= if count == 1 { 4 } else { 5 };
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
