// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2016      Spike
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/pr_ext.c, PF_sprintf_internal, PF_sprintf

//! vkQuake's `sprintf` builtin: its directive reader and the C `printf` conversions it hands
//! each directive to (BUILTIN-18.8).
//!
//! The directive reader follows `PF_sprintf_internal`: an argument position `N$`, flags, a
//! width and precision that may come from arguments, length letters that choose between a
//! float and an integer argument, and FTE's `%S` and `%v`. Each conversion is then written as
//! glibc's `printf` writes it, into vkQuake's 1024-byte temporary string.

use qc_types::PROGS_VECTOR_SLOTS;

use crate::BuiltinError;

/// vkQuake's `STRINGTEMP_LENGTH`, the buffer `sprintf` writes into, its terminator included.
const STRINGTEMP_LENGTH: usize = 1024;

/// The longest precision written out. A longer one only adds digits past the buffer, or
/// zeros that `%g` strips, since a double's exact decimal has fewer than 1100 digits after
/// its point.
const MAX_PRECISION: usize = 2000;
/// `%p`'s width when none is given, eight hexadecimal digits (BUILTIN-18.8).
const POINTER_WIDTH: i64 = 8;
/// `strtol`'s base.
const DECIMAL: i64 = 10;
/// A wide argument's high word's shift.
const WORD_BITS: u32 = 32;
/// `uint64_t`'s top bit, set back after the value less 2^63 is converted.
const TOP_BIT: u64 = 1 << 63;
/// A floating conversion's precision when none is given (BUILTIN-18.8).
const DEFAULT_PRECISION: usize = 6;
/// `%g` writes `%f` while the exponent is from -4 to below the precision (BUILTIN-18.8).
const GENERAL_LOWEST_EXPONENT: i64 = -4;
/// What `quotedbuf` keeps for the closing quote and its terminator past the text.
const QUOTED_ROOM: usize = 4;

/// The arguments `sprintf` reads, `OFS_PARM0 + 3 * n` and the slots after it.
pub(crate) trait Args {
    /// The call's argument count, `qcvm->argc`.
    fn count(&self) -> usize;
    /// The bits of slot `offset` of argument `arg`, 0 for a slot the progs does not have.
    fn word(&self, arg: usize, offset: usize) -> u32;
    /// Argument `arg` as a string, `G_STRING`.
    ///
    /// # Errors
    ///
    /// The mock's error for a value that is no string (BUILTIN-2.8).
    fn string(&mut self, arg: usize) -> Result<Vec<u8>, BuiltinError>;
}

/// The flags of a directive, `PRINTF_*`.
#[derive(Clone, Copy, Default)]
struct Flags {
    alternate: bool,
    zero_pad: bool,
    left: bool,
    space: bool,
    plus: bool,
}

/// Whether a conversion may pad with zeros under the `0` flag: not a character or a string, not
/// an integer with a precision, not an infinity or a NaN.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ZeroPad {
    /// Zeros between the sign and the digits.
    Allowed,
    /// Spaces.
    Never,
}

/// Whether a numeric argument is read as a float or an integer (`isfloat`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Float,
    Integer,
}

/// One directive, read up to its conversion letter.
struct Directive {
    flags: Flags,
    width: i64,
    precision: Option<usize>,
    kind: Kind,
    wide: bool,
    arg: usize,
    conversion: u8,
}

/// The output buffer, which keeps what fits before its terminator, as `q_snprintf` and the
/// verbatim copy do.
struct Out {
    bytes: Vec<u8>,
}

impl Out {
    /// Whether a directive is written: `if (o < end - 1)`.
    fn has_room(&self) -> bool {
        self.bytes.len() < STRINGTEMP_LENGTH.saturating_sub(1)
    }

    /// Appends what `q_snprintf (o, end - o, ...)` then `o += strlen (o)` keep of `text`: as
    /// much as fits before the terminator, up to its first NUL.
    fn write(&mut self, text: &[u8]) {
        let room = STRINGTEMP_LENGTH
            .saturating_sub(1)
            .saturating_sub(self.bytes.len());
        let kept = text.get(..room.min(text.len())).unwrap_or(text);
        let end = kept.iter().position(|&b| b == 0).unwrap_or(kept.len());
        self.bytes
            .extend_from_slice(kept.get(..end).unwrap_or(kept));
    }
}

/// `sprintf(format, ...)`'s text, `PF_sprintf_internal (format, firstarg, ...)` (BUILTIN-18.8).
///
/// # Errors
///
/// The string error of a `%s` or `%S` argument that is no string.
pub(crate) fn sprintf(
    format: &[u8],
    first: usize,
    args: &mut dyn Args,
) -> Result<Vec<u8>, BuiltinError> {
    let mut out = Out { bytes: Vec::new() };
    let mut reader = Reader {
        format,
        at: 0,
        first,
        next_arg: first,
    };
    loop {
        match reader.peek() {
            0 => break,
            b'%' => {
                reader.at = reader.at.saturating_add(1);
                if reader.peek() == b'%' {
                    if out.has_room() {
                        out.bytes.push(b'%');
                    }
                    reader.at = reader.at.saturating_add(1);
                    continue;
                }
                let Some(directive) = reader.directive(args) else {
                    break;
                };
                if out.has_room() && !directive.convert(&reader, args, &mut out)? {
                    break;
                }
                reader.at = reader.at.saturating_add(1);
            }
            byte => {
                if out.has_room() {
                    out.bytes.push(byte);
                }
                reader.at = reader.at.saturating_add(1);
            }
        }
    }
    Ok(out.bytes)
}

/// Where the reader is in the format, and the next argument a directive takes.
struct Reader<'f> {
    format: &'f [u8],
    at: usize,
    first: usize,
    next_arg: usize,
}

impl Reader<'_> {
    /// The byte at the reader, 0 past the end, as the C string's terminator.
    fn peek(&self) -> u8 {
        self.format.get(self.at).copied().unwrap_or(0)
    }

    /// The decimal number at the reader, `strtol (s, &err, 10)` of digits stored in an `int`,
    /// and moves past it.
    fn number(&mut self) -> i64 {
        i64::from(c_int(self.long()))
    }

    /// `strtol (s, &err, 10)` of the digits at the reader, saturating at `LONG_MAX`.
    fn long(&mut self) -> i64 {
        let mut value = 0_i64;
        while self.peek().is_ascii_digit() {
            value = value
                .saturating_mul(DECIMAL)
                .saturating_add(i64::from(self.peek().saturating_sub(b'0')));
            self.at = self.at.saturating_add(1);
        }
        value
    }

    /// The argument a `*` takes: `N$` after it, else the next one.
    fn star_arg(&mut self) -> Option<usize> {
        if self.peek().is_ascii_digit() {
            let n = self.number();
            if self.peek() != b'$' {
                return None;
            }
            self.at = self.at.saturating_add(1);
            Some(arg_position(n, self.first))
        } else {
            let arg = self.next_arg;
            self.next_arg = self.next_arg.saturating_add(1);
            Some(arg)
        }
    }

    /// The directive after a `%`, up to its conversion letter, which the reader is left at;
    /// `None` for an invalid one, which ends the output (`goto finished`).
    fn directive(&mut self, args: &mut dyn Args) -> Option<Directive> {
        let mut flags = Flags::default();
        let mut width = -1_i64;
        let mut arg = None;
        if self.peek().is_ascii_digit() {
            let leading_zero = self.peek() == b'0';
            let n = self.number();
            if self.peek() == b'$' {
                arg = Some(arg_position(n, self.first));
                self.at = self.at.saturating_add(1);
            } else {
                if leading_zero {
                    flags.zero_pad = true;
                }
                width = if leading_zero && n == 0 { -1 } else { n };
            }
        }
        if width < 0 {
            loop {
                match self.peek() {
                    b'#' => flags.alternate = true,
                    b'0' => flags.zero_pad = true,
                    b'-' => flags.left = true,
                    b' ' => flags.space = true,
                    b'+' => flags.plus = true,
                    _ => break,
                }
                self.at = self.at.saturating_add(1);
            }
            if self.peek() == b'*' {
                self.at = self.at.saturating_add(1);
                let from = self.star_arg()?;
                let value = float_to_i32(float_arg(&*args, self.first, from));
                if value < 0 {
                    flags.left = true;
                }
                // `width = -width`, which leaves INT_MIN negative, as x86 does.
                width = i64::from(if value < 0 {
                    value.wrapping_neg()
                } else {
                    value
                });
            } else if self.peek().is_ascii_digit() {
                width = self.number();
            }
        }
        let mut precision = None;
        if self.peek() == b'.' {
            self.at = self.at.saturating_add(1);
            if self.peek() == b'*' {
                self.at = self.at.saturating_add(1);
                let from = self.star_arg()?;
                let value = float_to_i32(float_arg(&*args, self.first, from));
                precision = usize::try_from(value).ok();
            } else if self.peek().is_ascii_digit() {
                precision = usize::try_from(self.number()).ok();
            } else {
                return None;
            }
        }
        let mut kind = None;
        let mut wide = false;
        loop {
            match self.peek() {
                b'h' => kind = Some(Kind::Float),
                b'l' | b'L' => kind = Some(Kind::Integer),
                b'q' => wide = true,
                b'j' | b'z' | b't' => {}
                _ => break,
            }
            self.at = self.at.saturating_add(1);
        }
        let conversion = self.peek();
        if matches!(conversion, b'p' | b'P') {
            flags.zero_pad = true;
            if width < 0 {
                width = POINTER_WIDTH;
            }
            kind = kind.or(Some(Kind::Integer));
        } else if conversion == b'i' {
            kind = kind.or(Some(Kind::Integer));
        }
        let arg = arg.unwrap_or_else(|| {
            let next = self.next_arg;
            self.next_arg = self.next_arg.saturating_add(1);
            next
        });
        Some(Directive {
            flags,
            width: width.max(0),
            precision: precision.map(|p| p.min(MAX_PRECISION)),
            kind: kind.unwrap_or(Kind::Float),
            wide,
            arg,
            conversion,
        })
    }
}

/// A C `long` stored in an `int`: its low 32 bits, as x86-64 truncates it.
fn c_int(value: i64) -> i32 {
    let [a, b, c, d, ..] = value.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}

/// The argument `N$` names: `N + (firstarg - 1)`.
fn arg_position(n: i64, first: usize) -> usize {
    usize::try_from(n)
        .unwrap_or(0)
        .saturating_add(first)
        .saturating_sub(1)
}

/// Whether argument `arg` was passed: `(a) >= firstarg && (a) < qcvm->argc`.
fn present(args: &dyn Args, first: usize, arg: usize) -> bool {
    arg >= first && arg < args.count()
}

/// `GETARG_FLOAT`.
fn float_arg(args: &dyn Args, first: usize, arg: usize) -> f32 {
    if present(args, first, arg) {
        f32::from_bits(args.word(arg, 0))
    } else {
        0.0
    }
}

/// The 64 bits of argument `arg`'s first two slots, `G_DOUBLE` and `G_INT64`.
fn wide_bits(args: &dyn Args, first: usize, arg: usize) -> u64 {
    if present(args, first, arg) {
        u64::from(args.word(arg, 0)) | (u64::from(args.word(arg, 1)) << WORD_BITS)
    } else {
        0
    }
}

/// `GETARG_INT`.
fn int_arg(args: &dyn Args, first: usize, arg: usize) -> i32 {
    if present(args, first, arg) {
        i32::from_ne_bytes(args.word(arg, 0).to_ne_bytes())
    } else {
        0
    }
}

/// 2^63, where a 64-bit integer's range ends.
const TWO_63: f64 = f64::from_bits(0x43E0_0000_0000_0000);

/// x86-64's `cvttss2si` and `cvttsd2si` to 64 bits, which a C cast to `int64_t` compiles to:
/// truncation, and `INT64_MIN` for NaN and a value out of range.
fn to_i64(value: f64) -> i64 {
    if (-TWO_63..TWO_63).contains(&value) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "BUILTIN-18.8: inside the range the cast truncates as cvttsd2si does"
        )]
        let truncated = value.trunc() as i64;
        truncated
    } else {
        i64::MIN
    }
}

/// A C cast of a double to `uint64_t` on x86-64: below 2^63 `cvttsd2si`, from 2^63 the value
/// less 2^63 converted and its top bit set.
fn to_u64(value: f64) -> u64 {
    if value < TWO_63 {
        u64::from_ne_bytes(to_i64(value).to_ne_bytes())
    } else {
        u64::from_ne_bytes(to_i64(value - TWO_63).to_ne_bytes()) ^ TOP_BIT
    }
}

/// A C cast of a float to `int` on x86-64, `cvttss2si`: `INT32_MIN` out of range.
fn float_to_i32(value: f32) -> i32 {
    i32::try_from(to_i64(f64::from(value))).unwrap_or(i32::MIN)
}

impl Directive {
    /// `GETARG_SNUMERIC (int64_t, a)`.
    fn signed_arg(&self, args: &dyn Args, first: usize) -> i64 {
        match (self.wide, self.kind) {
            (true, Kind::Float) => to_i64(f64::from_bits(wide_bits(args, first, self.arg))),
            (true, Kind::Integer) => {
                i64::from_ne_bytes(wide_bits(args, first, self.arg).to_ne_bytes())
            }
            (false, Kind::Float) => to_i64(f64::from(float_arg(args, first, self.arg))),
            (false, Kind::Integer) => i64::from(int_arg(args, first, self.arg)),
        }
    }

    /// `GETARG_UNUMERIC (uint64_t, a)`.
    fn unsigned_arg(&self, args: &dyn Args, first: usize) -> u64 {
        match (self.wide, self.kind) {
            (true, Kind::Float) => to_u64(f64::from_bits(wide_bits(args, first, self.arg))),
            (true, Kind::Integer) => wide_bits(args, first, self.arg),
            (false, Kind::Float) => to_u64(f64::from(float_arg(args, first, self.arg))),
            (false, Kind::Integer) => u64::from(u32::from_ne_bytes(
                int_arg(args, first, self.arg).to_ne_bytes(),
            )),
        }
    }

    /// `GETARG_SNUMERIC (double, a)`.
    fn double_arg(&self, args: &dyn Args, first: usize) -> f64 {
        match (self.wide, self.kind) {
            (true, Kind::Float) => f64::from_bits(wide_bits(args, first, self.arg)),
            #[expect(
                clippy::cast_precision_loss,
                reason = "BUILTIN-18.8: C's (double) of an int64_t rounds to nearest as this does"
            )]
            (true, Kind::Integer) => {
                i64::from_ne_bytes(wide_bits(args, first, self.arg).to_ne_bytes()) as f64
            }
            (false, Kind::Float) => f64::from(float_arg(args, first, self.arg)),
            (false, Kind::Integer) => f64::from(int_arg(args, first, self.arg)),
        }
    }

    /// Writes the directive; `false` for a conversion letter vkQuake does not know, which ends
    /// the output.
    fn convert(
        &self,
        reader: &Reader<'_>,
        args: &mut dyn Args,
        out: &mut Out,
    ) -> Result<bool, BuiltinError> {
        let first = reader.first;
        let text = match self.conversion {
            b'd' | b'i' => {
                let value = self.signed_arg(&*args, first);
                let sign = if value < 0 {
                    "-"
                } else if self.flags.plus {
                    "+"
                } else if self.flags.space {
                    " "
                } else {
                    ""
                };
                self.integer(sign, value.unsigned_abs().to_string(), "")
            }
            b'o' | b'u' | b'x' | b'X' | b'p' | b'P' => {
                let value = self.unsigned_arg(&*args, first);
                let (digits, prefix) = match self.conversion {
                    b'o' => (format!("{value:o}"), ""),
                    b'u' => (value.to_string(), ""),
                    b'x' | b'p' => (format!("{value:x}"), if value == 0 { "" } else { "0x" }),
                    _ => (format!("{value:X}"), if value == 0 { "" } else { "0X" }),
                };
                let prefix = if self.flags.alternate { prefix } else { "" };
                self.integer("", digits, prefix)
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
                self.floating(self.conversion, self.double_arg(&*args, first))
            }
            b'v' | b'V' => {
                let conversion = if self.conversion == b'v' { b'g' } else { b'G' };
                let mut parts = Vec::new();
                for offset in 0..PROGS_VECTOR_SLOTS {
                    let value = if present(&*args, first, self.arg) {
                        let word = args.word(self.arg, offset);
                        match self.kind {
                            Kind::Float => f64::from(f32::from_bits(word)),
                            Kind::Integer => f64::from(i32::from_ne_bytes(word.to_ne_bytes())),
                        }
                    } else {
                        0.0
                    };
                    parts.push(self.floating(conversion, value));
                }
                parts.join(&b' ')
            }
            b'c' => {
                let value = match self.kind {
                    Kind::Float => to_i64(f64::from(float_arg(&*args, first, self.arg))),
                    Kind::Integer => i64::from(int_arg(&*args, first, self.arg)),
                };
                let byte = value.to_le_bytes().first().copied().unwrap_or(0);
                self.flags
                    .pad(self.width, Vec::new(), vec![byte], ZeroPad::Never)
            }
            b's' => {
                let text = self.string_arg(args, first)?;
                let kept = cut(&text, self.precision);
                self.flags.pad(self.width, Vec::new(), kept, ZeroPad::Never)
            }
            b'S' => {
                let text = self.string_arg(args, first)?;
                self.flags.pad(
                    self.width,
                    Vec::new(),
                    cut(&quoted(&text), self.precision),
                    ZeroPad::Never,
                )
            }
            _ => return Ok(false),
        };
        out.write(&text);
        Ok(true)
    }

    /// `GETARG_STRING`: the directive's argument's text, empty when it was not passed.
    fn string_arg(&self, args: &mut dyn Args, first: usize) -> Result<Vec<u8>, BuiltinError> {
        if present(&*args, first, self.arg) {
            args.string(self.arg)
        } else {
            Ok(Vec::new())
        }
    }

    /// An integer conversion: `digits` of the magnitude, at least `precision` of them and none for
    /// a zero with precision 0, after `sign` and `prefix`, padded to the width.
    fn integer(&self, sign: &str, digits: String, prefix: &str) -> Vec<u8> {
        // Every base writes a zero as `0`.
        let zero = digits == "0";
        let mut digits = digits.into_bytes();
        if let Some(precision) = self.precision {
            if precision == 0 && zero {
                digits.clear();
            }
            if digits.len() < precision {
                let mut padded = vec![b'0'; precision.saturating_sub(digits.len())];
                padded.append(&mut digits);
                digits = padded;
            }
        }
        if self.conversion == b'o' && self.flags.alternate && digits.first() != Some(&b'0') {
            digits.insert(0, b'0');
        }
        let mut head = sign.as_bytes().to_vec();
        head.extend_from_slice(prefix.as_bytes());
        let zeros = if self.precision.is_none() {
            ZeroPad::Allowed
        } else {
            ZeroPad::Never
        };
        self.flags.pad(self.width, head, digits, zeros)
    }

    /// A floating conversion `e`, `E`, `f`, `F`, `g` or `G` of `value`, as glibc writes it.
    fn floating(&self, conversion: u8, value: f64) -> Vec<u8> {
        let upper = conversion.is_ascii_uppercase();
        let sign = if value.is_sign_negative() {
            "-"
        } else if self.flags.plus {
            "+"
        } else if self.flags.space {
            " "
        } else {
            ""
        };
        let magnitude = value.abs();
        let body = if magnitude.is_nan() {
            String::from(if upper { "NAN" } else { "nan" })
        } else if magnitude.is_infinite() {
            String::from(if upper { "INF" } else { "inf" })
        } else {
            let precision = self.precision.unwrap_or(DEFAULT_PRECISION);
            let text = match conversion.to_ascii_lowercase() {
                b'f' => self.flags.fixed(magnitude, precision),
                b'e' => self.flags.exponent(magnitude, precision),
                _ => self.flags.general(magnitude, precision),
            };
            if upper {
                text.to_ascii_uppercase()
            } else {
                text
            }
        };
        self.flags.pad(
            self.width,
            sign.as_bytes().to_vec(),
            body.into_bytes(),
            if value.is_finite() {
                ZeroPad::Allowed
            } else {
                ZeroPad::Never
            },
        )
    }
}

impl Flags {
    /// `head` and `body` padded to `width`: on the right with `-`, with zeros between them when
    /// the conversion and the `0` flag allow, else with spaces before.
    fn pad(&self, width: i64, head: Vec<u8>, body: Vec<u8>, zeros: ZeroPad) -> Vec<u8> {
        let len = head.len().saturating_add(body.len());
        let width = usize::try_from(width).unwrap_or(0).min(STRINGTEMP_LENGTH);
        let fill = width.saturating_sub(len);
        let mut out = Vec::with_capacity(len.saturating_add(fill));
        if self.left {
            out.extend(head);
            out.extend(body);
            out.resize(out.len().saturating_add(fill), b' ');
        } else if self.zero_pad && zeros == ZeroPad::Allowed {
            out.extend(head);
            out.resize(out.len().saturating_add(fill), b'0');
            out.extend(body);
        } else {
            out.resize(fill, b' ');
            out.extend(head);
            out.extend(body);
        }
        out
    }
    /// `%f`: the value rounded to `precision` places, a point kept with `#`.
    fn fixed(&self, value: f64, precision: usize) -> String {
        let mut text = format!("{value:.precision$}");
        if precision == 0 && self.alternate {
            text.push('.');
        }
        text
    }
    /// `%e`: one digit, `precision` places and an exponent of at least two digits with its sign.
    fn exponent(&self, value: f64, precision: usize) -> String {
        let (mantissa, exp) = split_exponent(value, precision);
        let mut text = mantissa;
        if precision == 0 && self.alternate {
            text.push('.');
        }
        let sign = if exp < 0 { '-' } else { '+' };
        text.push('e');
        text.push(sign);
        text.push_str(&format!("{:02}", exp.unsigned_abs()));
        text
    }
    /// `%g`: `%e` or `%f` by the exponent `%e` would show, `P` significant digits, and trailing
    /// zeros and a trailing point removed unless `#`.
    fn general(&self, value: f64, precision: usize) -> String {
        let p = precision.max(1);
        let (_, x) = split_exponent(value, p.saturating_sub(1));
        let p_i = i64::try_from(p).unwrap_or(i64::MAX);
        let x = i64::from(x);
        let text = if p_i > x && x >= GENERAL_LOWEST_EXPONENT {
            let places = usize::try_from(p_i.saturating_sub(1).saturating_sub(x)).unwrap_or(0);
            self.fixed(value, places)
        } else {
            self.exponent(value, p.saturating_sub(1))
        };
        if self.alternate {
            return text;
        }
        let (number, tail) = match text.find('e') {
            Some(at) => text.split_at(at),
            None => (text.as_str(), ""),
        };
        let number = if number.contains('.') {
            number.trim_end_matches('0').trim_end_matches('.')
        } else {
            number
        };
        format!("{number}{tail}")
    }
}

/// `%S`'s argument quoted for tokenizing: in double quotes, a leading `\` too and `\n`, `\r`
/// and `\"` escaped when it has one of those, or would not fit `quotedbuf`.
fn quoted(text: &[u8]) -> Vec<u8> {
    const QUOTED_LEN: usize = 65536;
    let mut buf = vec![b'\\', b'"'];
    let mut warn = false;
    let mut leftover = false;
    for &byte in text {
        if buf.len() >= QUOTED_LEN.saturating_sub(QUOTED_ROOM) {
            leftover = true;
            break;
        }
        match byte {
            b'\n' => {
                buf.extend_from_slice(b"\\n");
                warn = true;
            }
            b'\r' => {
                buf.extend_from_slice(b"\\r");
                warn = true;
            }
            b'"' => {
                buf.extend_from_slice(b"\\\"");
                warn = true;
            }
            _ => buf.push(byte),
        }
    }
    buf.push(b'"');
    if warn || leftover {
        buf
    } else {
        buf.split_off(1)
    }
}

/// `text` cut to `precision` bytes, as `%.Ns` reads at most N.
fn cut(text: &[u8], precision: Option<usize>) -> Vec<u8> {
    let end = precision.map_or(text.len(), |p| p.min(text.len()));
    text.get(..end).unwrap_or(text).to_vec()
}

/// Rust's `{:.precision$e}` of `value` as its mantissa and its exponent.
fn split_exponent(value: f64, precision: usize) -> (String, i32) {
    let text = format!("{value:.precision$e}");
    match text.split_once('e') {
        Some((mantissa, exp)) => (mantissa.to_owned(), exp.parse().unwrap_or(0)),
        None => (text, 0),
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use super::{Args, sprintf};
    use crate::{BuiltinError, RuntimeError};

    /// Arguments 1 and up, each a float, an integer, a vector or a string.
    #[derive(Clone, Copy)]
    enum Arg {
        F(f32),
        I(i32),
        V([f32; 3]),
        S(&'static str),
        Bad,
    }

    struct Fixed(Vec<Arg>);

    impl Args for Fixed {
        fn count(&self) -> usize {
            self.0.len() + 1
        }

        fn word(&self, arg: usize, offset: usize) -> u32 {
            match self.0[arg - 1] {
                Arg::F(f) if offset == 0 => f.to_bits(),
                Arg::I(i) if offset == 0 => i.cast_unsigned(),
                Arg::V(v) => v[offset].to_bits(),
                Arg::F(_) | Arg::I(_) | Arg::S(_) | Arg::Bad => 0,
            }
        }

        fn string(&mut self, arg: usize) -> Result<Vec<u8>, BuiltinError> {
            match self.0[arg - 1] {
                Arg::S(s) => Ok(s.as_bytes().to_vec()),
                Arg::F(_) | Arg::I(_) | Arg::V(_) | Arg::Bad => {
                    Err(BuiltinError::from(RuntimeError::InvalidString))
                }
            }
        }
    }

    fn print(format: &str, args: &[Arg]) -> String {
        let text = sprintf(format.as_bytes(), 1, &mut Fixed(args.to_vec())).unwrap();
        String::from_utf8(text).unwrap()
    }

    // spec BUILTIN-18.8: each conversion is written as C's printf writes it (the expected texts
    // are a C program's output for the format vkQuake builds), a numeric argument a float unless
    // `l`, `i` or `p` makes it an integer
    #[test]
    fn conversions_are_written_as_c_writes_them() {
        use Arg::{F, I, S, V};
        for (format, args, expected) in [
            ("%5.2f", vec![F(3.25)], " 3.25"),
            ("%-5d|", vec![F(42.0)], "42   |"),
            ("%05d", vec![F(-42.0)], "-0042"),
            ("%x", vec![F(255.0)], "ff"),
            ("%#x", vec![F(255.0)], "0xff"),
            ("%#o", vec![F(8.0)], "010"),
            ("%e", vec![F(12345.678)], "1.234568e+04"),
            ("%g", vec![F(0.0001)], "0.0001"),
            ("%g", vec![F(0.00001)], "1e-05"),
            ("%g", vec![F(1.25e8)], "1.25e+08"),
            ("%.3s", vec![S("abcdef")], "abc"),
            ("%c", vec![F(65.0)], "A"),
            ("%+d", vec![F(5.0)], "+5"),
            ("% d", vec![F(5.0)], " 5"),
            ("%.0f", vec![F(2.5)], "2"),
            ("%.0f", vec![F(3.5)], "4"),
            ("%v", vec![V([1.0, 2.5, -3.0])], "1 2.5 -3"),
            ("%f", vec![F(f32::NAN)], "nan"),
            ("%f", vec![F(-f32::NAN)], "-nan"),
            ("%f", vec![F(f32::NEG_INFINITY)], "-inf"),
            ("%08f", vec![F(f32::INFINITY)], "     inf"),
            ("%.0d", vec![F(0.0)], ""),
            ("%6.3d", vec![F(7.0)], "   007"),
            ("%.0e", vec![F(1.5)], "2e+00"),
            ("%#.0e", vec![F(1.5)], "2.e+00"),
            ("%#.3g", vec![F(1.0)], "1.00"),
            ("%.3g", vec![F(1.0)], "1"),
            ("%G", vec![F(1e-10)], "1E-10"),
            ("%E", vec![F(0.0)], "0.000000E+00"),
            ("%g", vec![F(100_000.0)], "100000"),
            ("%g", vec![F(1_000_000.0)], "1e+06"),
            ("%.1f", vec![F(0.25)], "0.2"),
            ("%lu", vec![I(-1)], "4294967295"),
            ("%#10lX", vec![I(48879)], "    0XBEEF"),
            ("%-12.2e|", vec![F(-0.0)], "-0.00e+00   |"),
            ("%i", vec![I(-7)], "-7"),
            ("%d", vec![F(3.9)], "3"),
            ("%p", vec![I(255)], "000000ff"),
        ] {
            assert_eq!(print(format, &args), expected, "{format}");
        }
    }

    // spec BUILTIN-18.8: `N$` names an argument, `*` takes the width or precision from one, a
    // negative width is left-justified, and a missing argument is 0 or empty
    #[test]
    fn arguments_are_taken_by_position_and_star() {
        use Arg::{F, S};
        assert_eq!(print("%2$s %1$s", &[S("a"), S("b")]), "b a");
        assert_eq!(print("%*d|", &[F(4.0), F(7.0)]), "   7|");
        assert_eq!(print("%*d|", &[F(-4.0), F(7.0)]), "7   |");
        assert_eq!(print("%.*f", &[F(1.0), F(2.25)]), "2.2");
        assert_eq!(print("%*1$d|", &[F(3.0)]), "  3|");
        assert_eq!(print("%d %s|", &[]), "0 |");
        assert_eq!(print("%0d", &[F(5.0)]), "5");
        assert_eq!(print("%3$d", &[F(1.0), F(2.0), F(3.0)]), "3");
    }

    // spec BUILTIN-18.8: `%%` is a `%`; an invalid directive, an unknown letter and a
    // trailing `%` end the text there
    #[test]
    fn a_bad_directive_ends_the_text() {
        use Arg::F;
        assert_eq!(print("100%%", &[]), "100%");
        assert_eq!(print("a%.fb", &[F(1.0)]), "a");
        assert_eq!(print("a%Qb", &[F(1.0)]), "a");
        assert_eq!(print("a%Ib", &[F(1.0)]), "a");
        assert_eq!(print("a%", &[]), "a");
        assert_eq!(print("a%*5d", &[F(1.0)]), "a");
    }

    // spec BUILTIN-18.8: %S quotes its string, escaping a line break or quote; %c of 0 writes
    // only the padding before it; the text stops short of 1024 bytes
    #[test]
    fn quoting_nul_and_the_buffer() {
        use Arg::{F, S};
        assert_eq!(print("%S", &[S("ab")]), "\"ab\"");
        assert_eq!(print("%S", &[S("a\"b\n")]), "\\\"a\\\"b\\n\"");
        assert_eq!(print("[%3c]", &[F(0.0)]), "[  ]");
        let long = "x".repeat(1100);
        let long: &'static str = Box::leak(long.into_boxed_str());
        assert_eq!(print("%s", &[S(long)]).len(), 1023);
        assert_eq!(print("%2000d", &[F(1.0)]).len(), 1023);
    }

    // spec BUILTIN-18.8: the reader's corners: a `0` width is no width, so the flags still
    // follow it; a `0` among the flags zero-pads; `h`, `q` and the C length letters choose the
    // argument's kind without ending the directive; a wide integer is two argument slots; a
    // `*` width of 0 leaves `%p` at no width; a precision of 0 clears only a zero; `%v` is
    // three `%g` and `%V` three `%G`; and `%S` escapes a carriage return
    #[test]
    fn the_reader_s_corners() {
        use Arg::{F, I, S, V};
        let wide = V([f32::from_bits(1), f32::from_bits(2), 0.0]);
        for (format, args, expected) in [
            ("%0-3d|", vec![F(5.0)], "5  |"),
            ("% 05d", vec![F(5.0)], " 0005"),
            ("%hi", vec![F(3.0)], "3"),
            ("%zd%jd%td", vec![F(1.0), F(2.0), F(3.0)], "123"),
            ("%qld", vec![wide], "8589934593"),
            ("%qlx", vec![wide], "200000001"),
            ("%*p|", vec![F(0.0), I(255)], "ff|"),
            ("%*p|", vec![F(-3.0), I(255)], "ff |"),
            ("%.0d", vec![F(5.0)], "5"),
            ("%.0u", vec![F(5.0)], "5"),
            ("%.0x", vec![F(0.0)], ""),
            ("%#x", vec![F(0.0)], "0"),
            ("%u", vec![F(1e19)], "9999999980506447872"),
            (
                "%v",
                vec![V([1e-10, f32::NAN, f32::INFINITY])],
                "1e-10 nan inf",
            ),
            (
                "%V",
                vec![V([1e-10, f32::NAN, f32::INFINITY])],
                "1E-10 NAN INF",
            ),
            ("%S", vec![S("a\rb")], "\\\"a\\rb\""),
        ] {
            assert_eq!(print(format, &args), expected, "{format}");
        }
    }

    // spec BUILTIN-18.8: a `%%` or a plain byte at the buffer's end is dropped like a
    // conversion: the text never passes 1023 bytes
    #[test]
    fn the_buffer_s_end_drops_every_kind_of_byte() {
        let full = "x".repeat(1023);
        let over = "x".repeat(1030);
        assert_eq!(print(&format!("{full}%%"), &[]).len(), 1023);
        assert_eq!(print(&format!("{full}y"), &[]).len(), 1023);
        assert_eq!(print(&over, &[]).len(), 1023);
        assert!(print(&format!("{full}%%"), &[]).ends_with('x'));
    }

    // spec BUILTIN-18.8: a string argument that is no string is the mock's string error
    #[test]
    fn a_bad_string_is_an_error() {
        let error = sprintf(b"%s", 1, &mut Fixed(vec![Arg::Bad])).unwrap_err();
        assert_eq!(error, BuiltinError::from(RuntimeError::InvalidString));
    }
}
