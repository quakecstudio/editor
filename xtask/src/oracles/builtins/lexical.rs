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

/// Lines before [`BIND`] under FTEQCC's compiler flags, each set by a `#pragma flag` (FTEF-12.12,
/// FTEF-12.13), with the line that reads the same bytes under the default flags beside it:
/// `qccx`'s `:` between string parts, character constants and `%N` immediates, an error unless
/// `#pragma warning` makes F313 none; `qcfuncs` off and its octal escapes; `ILP32`'s 32-bit `l`,
/// made an error with F312; `ssp`'s `**`, made an error with F321; and `hashonly`, under which
/// a name is no macro's.
const FLAG_LINES: &[&str] = &[
    "#pragma flag enable qccx\nstring s = \"a\":\"b\";\n",
    "string s = \"a\":\"b\";\n",
    "#pragma flag enable qccx\nfloat x = 'a';\n",
    "#pragma flag enable qccx\n#pragma warning disable F313\nfloat x = 'a';\n",
    "#pragma flag enable qccx\nfloat x = %5;\n",
    "#pragma flag enable qccx\n#pragma warning disable F313\nfloat x = %5;\n",
    "#pragma flag enable qccx\n#pragma flag disable qccx\nfloat x = 'a';\n",
    "#pragma flag disable qcfuncs\nstring s = \"\\8\";\n",
    "#pragma flag disable qcfuncs\nstring s = \"\\101\\7\";\n",
    "string s = \"\\8\";\n",
    "#pragma warning error F312\n#pragma flag enable ILP32\nfloat x = 0x100000000l;\n",
    "#pragma warning error F312\nfloat x = 0x100000000l;\n",
    "#pragma flag enable ssp\n#pragma warning error F321\nfloat x = 2**3;\n",
    "#define F f\n#pragma flag enable hashonly\nfloat F;\n",
    "#define F f\nfloat F;\n",
    "#pragma flag enable utf8\nstring s = \"a\u{e9}b\";\n",
];

/// Byte sequences under a flag that no `&str` holds, as [`BYTE_LINES`] are written: `utf8`'s
/// plain strings, read as UTF-8 (FTEF-12.12).
const FLAG_BYTE_LINES: &[&[u8]] = &[b"#pragma flag enable utf8\nstring s = \"a\xffb\";\n"];

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
    let mut cases: Vec<Case> = FLAG_LINES
        .iter()
        .map(|line| Case::new(Dialect::Fteqcc, format!("{line}{BIND}")))
        .collect();
    for line in FLAG_BYTE_LINES {
        let line: String = line.iter().copied().map(char::from).collect();
        let mut case = Case::new(Dialect::Fteqcc, format!("{line}{BIND}"));
        case.encoding = Encoding::Latin1;
        cases.push(case);
    }
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
