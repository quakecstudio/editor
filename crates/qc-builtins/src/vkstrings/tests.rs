// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    chr2str, float_to_unsigned, modulo, pow, sin, str2chr, strpad, strreplace, uri_escape,
    uri_unescape,
};

// spec BUILTIN-18.31: sin and pow are C's of the floats as doubles, rounded to a float; mod is
// a - n * (int)(a / n) in float, and 0 for a zero divisor
#[test]
fn math_builtins_round_as_c() {
    assert_eq!(sin(0.0), 0.0);
    assert_eq!(sin(core::f32::consts::FRAC_PI_2), 1.0);
    assert_eq!(pow(2.0, 10.0), 1024.0);
    assert_eq!(pow(2.0, 0.5), core::f32::consts::SQRT_2);
    assert_eq!(modulo(7.5, 2.0), 1.5);
    assert_eq!(modulo(-7.5, 2.0), -1.5, "truncation toward zero");
    assert_eq!(modulo(7.0, -2.0), 1.0);
    assert_eq!(modulo(5.0, 0.0), 0.0);
    assert_eq!(modulo(-5.0, -0.0), 0.0);
    // (int) of 1e30 is 0x80000000 (BUILTIN-2.2): 1e30 - 1 * -2147483648.
    assert_eq!(modulo(1e30, 1.0), 1e30 + 2.0_f32.powi(31));
}

// spec BUILTIN-18.32: str2chr reads the byte at an offset, from the end when negative, the
// first byte at 0, and 0 past either end
#[test]
fn str2chr_reads_a_byte() {
    assert_eq!(str2chr(b"abc", 0), f32::from(b'a'));
    assert_eq!(str2chr(b"abc", 2), f32::from(b'c'));
    assert_eq!(str2chr(b"abc", 3), 0.0, "the terminator");
    assert_eq!(str2chr(b"abc", 4), 0.0);
    assert_eq!(str2chr(b"abc", -1), f32::from(b'c'));
    assert_eq!(str2chr(b"abc", -3), f32::from(b'a'), "the start, offset 0");
    assert_eq!(str2chr(b"abc", -4), 0.0);
    assert_eq!(str2chr(b"\xff", 0), 255.0, "unsigned");
    assert_eq!(str2chr(b"", 0), 0.0);
}

// spec BUILTIN-18.32: chr2str makes a byte of each code, the Quake range its low byte, below
// 256 itself, any other `?`; a zero ends the text and so does the 1018-byte limit; a code is
// C's (unsigned int) of its float
#[test]
fn chr2str_makes_bytes() {
    assert_eq!(chr2str(&[72, 105]), b"Hi");
    assert_eq!(chr2str(&[0xe041, 0xe0ff]), [0x41, 0xff]);
    assert_eq!(chr2str(&[0xe100, 256, 0xdfff]), b"???");
    assert_eq!(chr2str(&[65, 0, 66]), b"A");
    assert_eq!(chr2str(&[65; 2000]).len(), 1018);
    assert_eq!(float_to_unsigned(65.9), 65);
    assert_eq!(float_to_unsigned(-1.0), u32::MAX, "wraps through 64 bits");
    assert_eq!(float_to_unsigned(2.0_f32.powi(32)), 0, "2^32's low bits");
    assert_eq!(float_to_unsigned(f32::NAN), 0);
    assert_eq!(float_to_unsigned(1e20), 0, "past an i64");
}

// spec BUILTIN-18.33: strpad pads with spaces to the width, on the left when it is negative,
// leaves a longer text as it is, and keeps within a temporary string
#[test]
fn strpad_pads_left_and_right() {
    assert_eq!(strpad(5, b"ab"), b"ab   ");
    assert_eq!(strpad(-5, b"ab"), b"   ab");
    assert_eq!(strpad(1, b"abc"), b"abc");
    assert_eq!(strpad(-1, b"abc"), b"abc");
    assert_eq!(strpad(0, b""), b"");
    assert_eq!(strpad(5000, b"ab").len(), 1023);
    assert_eq!(strpad(-5000, b"ab").len(), 1023);
    let long = [b'x'; 2000];
    assert_eq!(strpad(0, &long).len(), 1023);
    assert_eq!(strpad(-1, &long).len(), 1023);
    assert_eq!(
        strpad(i32::MIN, b"ab").len(),
        1023,
        "-INT_MIN wraps, then clamps"
    );
}

// spec BUILTIN-18.33: strreplace replaces each match left to right, gives the subject itself
// for an empty search, and stops at the temporary string's limit less the replacement
#[test]
fn strreplace_replaces_each_match() {
    assert_eq!(strreplace(b"a", b"xy", b"banana"), b"bxynxynxy");
    assert_eq!(strreplace(b"aa", b"b", b"aaa"), b"ba");
    assert_eq!(strreplace(b"", b"x", b"abc"), b"abc");
    assert_eq!(strreplace(b"q", b"x", b"abc"), b"abc");
    assert_eq!(strreplace(b"a", b"", b"aba"), b"b");
    let long = [b'a'; 2000];
    assert_eq!(strreplace(b"z", b"", &long).len(), 1022);
    assert_eq!(
        strreplace(b"a", b"bb", &long).len(),
        1020,
        "1018 then one more pair"
    );
    assert_eq!(
        strreplace(b"a", &[b'b'; 1023], b"aa"),
        b"",
        "no room at all"
    );
}

// spec BUILTIN-18.34: uri_escape keeps letters, digits and `.-_` and writes every other byte as
// %XX; uri_unescape reads %XX in either case, keeps a `%` without two digits, and ends at %00
#[test]
fn uri_escape_round_trips() {
    assert_eq!(uri_escape(b"a b/c.d-e_f"), b"a%20b%2Fc.d-e_f");
    assert_eq!(uri_escape(b"\xff"), b"%FF");
    assert_eq!(uri_escape(&[b' '; 600]).len(), 1020);
    assert_eq!(uri_unescape(b"a%20b%2fc"), b"a b/c");
    assert_eq!(uri_unescape(b"%zz%4"), b"%zz%4");
    assert_eq!(uri_unescape(b"a%00b"), b"a");
    assert_eq!(uri_unescape(b"%"), b"%");
    assert_eq!(uri_unescape(&[b'x'; 2000]).len(), 1022);
}
