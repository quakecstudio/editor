// SPDX-License-Identifier: GPL-3.0-or-later

//! The lexical family of the builtin differential (P2-116): the bytes FTEQCC's lexer reads
//! otherwise than id's, against what each profile makes of them (`docs/spec/dialect-fteqcc.md`
//! FTEF-13.15 to FTEF-13.21). Each program binds `f` after a line that only a lexer reading it as
//! the compiler does accepts or refuses, so the outcome says how it was read.

use qc_syntax::Dialect;

use super::{Case, Encoding};

/// What binds `f` once the line before it has been read.
const BIND: &str = "void() f = #5;\n";

/// Lines before [`BIND`], each read under both dialects: white space and control bytes
/// (FTEF-13.15), names of bytes of 0x80 or more (FTEF-13.16), numbers with letters after them
/// (FTEF-13.17), comments that end at a lone CR or run on past `/*/` (FTEF-13.18), numbers past
/// 64 bits (FTEF-13.19), strings and names past each compiler's token buffer (FTEF-13.20), and
/// strings whose UTF-8 input is not UTF-8 (FTEF-13.21).
const LINES: &[&str] = &[
    "float\x0cx;\n",
    "float\x01x;\n",
    "float\x7fx;\n",
    "float\x0bx;\n",
    "float x;\x0b\x0bfloat y;\n",
    "float \u{e9};\n",
    "float x\u{e9}y;\n",
    "float \u{200b}x;\n",
    "float x\u{e9}\u{e9};\n",
    "float x = 5abc;\n",
    "float x = 5fx;\n",
    "float x = 1.5e3;\n",
    "float x = 08;\n",
    "float x = 0x10g;\n",
    "float x = 5_;\n",
    "float x = 017;\n",
    "float x = 0x100000001;\n",
    "vector v = '1x 2 3';\n",
    "/*/ float x; */\n",
    "/* a /* b */\n",
    "/**/ float x;\n",
    "// c\rfloat y = ;\n",
    "float x; // c\rfloat y = ;\n",
    "float x; // c\n  // e\rfloat y = ;\n",
    "float x; // c\n// e\rfloat y = ;\n",
    "float x; // c\n \x0b// e\rfloat y = ;\n",
    "float x; // c\n\n\x0b// e\rfloat y = ;\n",
    "float x = \\\r\n// c\r5;\n",
    "string s = u8\"a\u{e9}b\";\n",
    "string s = Q\"\\b\u{e9}\";\n",
    "string s = \"a\\qb\";\n",
];

/// Byte sequences no `&str` holds, each a line before [`BIND`] (FTEF-13.16, FTEF-13.21), written
/// as Latin-1 so a case's text keeps one char per byte.
const BYTE_LINES: &[&[u8]] = &[
    b"float x\xc3;\n",
    b"float \xa0x;\n",
    b"float \x80x;\n",
    b"float x\xff\n;\n",
    b"string s = u8\"a\xffb\";\n",
    b"string s = \"a\xffb\";\n",
];

/// Builtin numbers past 64 bits, which FTEQCC's `long long` wraps (FTEF-13.19).
const NUMBERS: &[&str] = &[
    "18446744073709551621",
    "-18446744073709551621",
    "0x10000000000000005",
    "36893488147419103237",
];

/// Lengths about each compiler's token buffer: id's `pr_token[2048]` and FTEQCC's
/// `pr_token[8192]` (QC-27.4, FTEF-13.20).
const LENGTHS: [usize; 6] = [2047, 2048, 3000, 8191, 8192, 9000];

/// The lexical cases, under each dialect.
pub(super) fn cases() -> Vec<Case> {
    let mut sources: Vec<String> = LINES.iter().map(|line| format!("{line}{BIND}")).collect();
    for number in NUMBERS {
        sources.push(format!("void() f = #{number};\n"));
    }
    for length in LENGTHS {
        sources.push(format!("string s = \"{}\";\n{BIND}", "s".repeat(length)));
        let half = length.checked_div(2).unwrap_or(0);
        sources.push(format!(
            "string s = \"{}\" \"{}\";\n{BIND}",
            "s".repeat(half),
            "s".repeat(length.saturating_sub(half))
        ));
    }
    let mut cases = Vec::new();
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        for source in &sources {
            cases.push(Case::new(dialect, source.clone()));
        }
        for line in BYTE_LINES {
            let line: String = line.iter().copied().map(char::from).collect();
            let mut case = Case::new(dialect, format!("{line}{BIND}"));
            case.encoding = Encoding::Latin1;
            cases.push(case);
        }
    }
    cases
}
