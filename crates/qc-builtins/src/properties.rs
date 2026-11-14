// SPDX-License-Identifier: GPL-3.0-or-later

//! Properties of the readers of outside text that `fuzz/fuzz_targets/qc_builtins.rs` fuzzes:
//! the localization file and `PF_VarString` (RRE-1), the tokenizers, `sprintf`'s directive
//! reader and `atof` (BUILTIN-18).

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_in_result,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use proptest::prelude::*;

use crate::BuiltinError;
use crate::atof::atof;
use crate::localization::{Localization, RRE_VARSTRING_LEN};
use crate::printf::{Args, sprintf};
use crate::tokenize::{MAX_QC_TOKENS, MAX_SEPARATORS, TokenIndex, Tokens};

/// The longest text a temporary string holds, `STRINGTEMP_LENGTH` less its terminator.
const TEMP_TEXT: usize = 1023;

/// `sprintf`'s arguments 1 and up: three words and a string each.
struct Generated(Vec<([u32; 3], Vec<u8>)>);

impl Args for Generated {
    fn count(&self) -> usize {
        self.0.len() + 1
    }

    fn word(&self, arg: usize, offset: usize) -> u32 {
        self.0
            .get(arg.wrapping_sub(1))
            .and_then(|(words, _)| words.get(offset))
            .copied()
            .unwrap_or(0)
    }

    fn string(&mut self, arg: usize) -> Result<Vec<u8>, BuiltinError> {
        Ok(self
            .0
            .get(arg.wrapping_sub(1))
            .map(|(_, text)| text.clone())
            .unwrap_or_default())
    }
}

/// Every token inside `text`, in order, at most 64, each `argv` at most 1023 bytes and the
/// same from a negative index; nothing outside the list.
fn check_tokens(tokens: &Tokens, count: usize, text: &[u8]) -> Result<(), TestCaseError> {
    prop_assert!(count <= MAX_QC_TOKENS);
    let count = i32::try_from(count).unwrap();
    let len = i64::try_from(text.len()).unwrap();
    let mut last = 0;
    for index in 0..count {
        let (start, end) = (
            tokens.start(TokenIndex::new(index)),
            tokens.end(TokenIndex::new(index)),
        );
        prop_assert!(
            last <= start && start <= end && end <= len,
            "{start} {end} {len}"
        );
        last = end;
        let word = tokens.argv(TokenIndex::new(index)).unwrap();
        prop_assert!(word.len() <= TEMP_TEXT);
        prop_assert_eq!(tokens.argv(TokenIndex::new(index - count)), Some(word));
    }
    for index in [count, -count - 1] {
        prop_assert_eq!(tokens.argv(TokenIndex::new(index)), None);
        prop_assert_eq!(
            (
                tokens.start(TokenIndex::new(index)),
                tokens.end(TokenIndex::new(index))
            ),
            (-1, -1)
        );
    }
    Ok(())
}

/// Bytes weighted toward the ones the tokenizers stop at, around a run of letters that may
/// make a word longer than `argv` keeps.
fn command_text() -> impl Strategy<Value = Vec<u8>> {
    let byte = prop_oneof![
        prop::sample::select(b" \t\n\"{}()':/*,;ab".to_vec()),
        any::<u8>(),
    ];
    let part = prop::collection::vec(byte, 0..300);
    let word = prop::collection::vec(b'a'..=b'z', 0..1100);
    (part.clone(), word, part).prop_map(|(head, word, tail)| [head, word, tail].concat())
}

/// `value` written exactly in hexadecimal, as C's `%a` would write it: the sign, `0x1.` or a
/// subnormal's `0x0.`, its 13 mantissa digits and its binary exponent, with `lead` zero digits
/// before the first and `trail` after the last.
fn exact_hex(value: f64, lead: usize, trail: usize) -> String {
    let bits = value.to_bits();
    let sign = if bits >> 63 == 1 { "-" } else { "" };
    let field = (bits >> 52) & 0x7ff;
    let mantissa = bits & ((1 << 52) - 1);
    let (whole, exponent) = if field == 0 {
        (0, -1022)
    } else {
        (1, i64::try_from(field).unwrap() - 1023)
    };
    format!(
        "{sign}0x{}{whole}.{mantissa:013x}{}p{exponent}",
        "0".repeat(lead),
        "0".repeat(trail)
    )
}

/// The value of the hexadecimal digits `digits`, the last `fraction` of them after the point,
/// times `2^exponent`, written exactly in decimal, which Rust's `parse` rounds correctly: every
/// such value has a finite decimal expansion.
fn exact_decimal(digits: &[u32], fraction: usize, exponent: i64) -> String {
    // Little-endian decimal digits of the integer the hexadecimal digits spell.
    let mut number = vec![0_u32];
    #[expect(
        clippy::integer_division,
        reason = "test code: base-10 long multiplication carries the quotient"
    )]
    let times = |number: &mut Vec<u32>, by: u32, add: u32| {
        let mut carry = add;
        for digit in number.iter_mut() {
            let wide = *digit * by + carry;
            *digit = wide % 10;
            carry = wide / 10;
        }
        while carry > 0 {
            number.push(carry % 10);
            carry /= 10;
        }
    };
    for &digit in digits {
        times(&mut number, 16, digit);
    }
    let scale = exponent - 4 * i64::try_from(fraction).unwrap();
    let mut power = 0_i64;
    if scale >= 0 {
        for _ in 0..scale {
            times(&mut number, 2, 0);
        }
    } else {
        // m * 2^-k is m * 5^k / 10^k.
        for _ in 0..-scale {
            times(&mut number, 5, 0);
        }
        power = scale;
    }
    let text: String = number
        .iter()
        .rev()
        .map(|d| char::from_digit(*d, 10).unwrap())
        .collect();
    format!("{text}e{power}")
}

/// Hexadecimal digits at or next to a tie between two doubles: a `1` and 13 digits, the 53
/// bits a double keeps, then a digit at or around half of the next, a run of zeros and
/// perhaps a final nonzero digit that breaks the tie.
fn near_tie_digits() -> impl Strategy<Value = Vec<u32>> {
    (
        proptest::collection::vec(0_u32..16, 13),
        prop_oneof![Just(8_u32), Just(7), Just(9), Just(0), Just(15)],
        0_usize..30,
        prop_oneof![Just(0_u32), Just(1)],
    )
        .prop_map(|(head, half, zeros, last)| {
            let mut digits = vec![1];
            digits.extend(head);
            digits.push(half);
            digits.extend(std::iter::repeat_n(0, zeros));
            digits.push(last);
            digits
        })
}

proptest! {
    // spec RRE-1.7: LOC_LoadFile reads the text up to its first zero and skips a UTF-8 byte
    // order mark: `key = "value"` lines read back by key whatever follows the zero, with or
    // without the mark, the first of a key listed twice kept (RRE-1.2)
    #[test]
    fn a_quoted_entry_reads_back(
        entries in prop::collection::vec(("[a-z_]{1,8}", "[ -!#-\\[\\]-~]{0,24}"), 1..12),
        tail in prop::collection::vec(any::<u8>(), 0..64),
        bom: bool,
    ) {
        let mut file = if bom { b"\xEF\xBB\xBF".to_vec() } else { Vec::new() };
        for (key, value) in &entries {
            file.extend_from_slice(format!("{key} = \"{value}\"\n").as_bytes());
        }
        file.push(0);
        file.extend_from_slice(&tail);
        let loc = Localization::loc_load_file(&file);
        for (key, _) in &entries {
            let first = &entries.iter().find(|(k, _)| k == key).unwrap().1;
            let lookup = format!("${key}");
            prop_assert_eq!(loc.loc_get_raw_string(lookup.as_bytes()), Some(first.as_bytes()));
        }
    }

    // spec RRE-1.7: any bytes load, the text read only up to its first zero
    #[test]
    fn any_file_loads_up_to_its_first_zero(
        file in prop::collection::vec(any::<u8>(), 0..400),
        tail in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut cut = file.clone();
        cut.push(0);
        cut.extend_from_slice(&tail);
        let head = &file[..file.iter().position(|&b| b == 0).unwrap_or(file.len())];
        prop_assert_eq!(Localization::loc_load_file(&cut), Localization::loc_load_file(head));
    }

    // spec RRE-1.9: PF_VarString's text fits its 1024-byte buffer with its terminator, and
    // with no file loaded it is the parameters, C strings, joined and cut to 1023 bytes
    #[test]
    fn var_string_fits_its_buffer(
        file in prop::collection::vec(any::<u8>(), 0..200),
        parms in prop::collection::vec(prop::collection::vec(1_u8..=255, 0..400), 0..8),
    ) {
        let text = Localization::loc_load_file(&file).pf_var_string(&parms);
        prop_assert!(text.len() < RRE_VARSTRING_LEN);
        let plain = Localization::default().pf_var_string(&parms);
        let joined = parms.concat();
        prop_assert_eq!(plain, joined[..joined.len().min(TEMP_TEXT)].to_vec());
    }

    // spec BUILTIN-18.23: tokenize keeps at most 64 tokens, each starting and ending inside
    // the text after the one before, argv at most 1023 bytes, a negative index counting from
    // the end, and -1 or the null string outside the list
    #[test]
    fn tokenize_keeps_its_tokens_inside_the_text(text in command_text()) {
        let mut tokens = Tokens::default();
        let count = tokens.tokenize(&text);
        check_tokens(&tokens, count, &text)?;
    }

    // spec BUILTIN-18.24: tokenizebyseparator keeps at most 64 tokens inside the text, each
    // the bytes from its start to its end
    #[test]
    fn split_tokens_are_the_text_between_separators(
        text in command_text(),
        separators in prop::collection::vec(
            prop::collection::vec(prop::sample::select(b" ,;ab".to_vec()), 0..3),
            0..=MAX_SEPARATORS,
        ),
    ) {
        let mut tokens = Tokens::default();
        let count = tokens.split(&text, &separators);
        check_tokens(&tokens, count, &text)?;
        for index in 0..i32::try_from(count).unwrap() {
            let start = usize::try_from(tokens.start(TokenIndex::new(index))).unwrap();
            let end = usize::try_from(tokens.end(TokenIndex::new(index))).unwrap();
            let slice = text[start..end].split(|&b| b == 0).next().unwrap();
            prop_assert_eq!(
                tokens.argv(TokenIndex::new(index)).unwrap(),
                slice[..slice.len().min(TEMP_TEXT)].to_vec()
            );
        }
    }

    // spec BUILTIN-18.8: sprintf's text stops at 1023 bytes whatever the format and its
    // arguments, and a format with no directive is itself
    #[test]
    fn sprintf_stays_in_its_buffer(
        format in prop::collection::vec(
            prop_oneof![
                prop::sample::select(b"%%$*.-+ #0123456789hlLqdiouxXeEfFgGcsSvVpP".to_vec()),
                any::<u8>(),
            ],
            0..200,
        ),
        args in prop::collection::vec(
            (any::<[u32; 3]>(), prop::collection::vec(1_u8..=255, 0..1200)),
            0..8,
        ),
        plain in prop::collection::vec(prop::sample::select(b"ab \n{}".to_vec()), 0..1500),
    ) {
        let text = sprintf(&format, 1, &mut Generated(args)).unwrap();
        prop_assert!(text.len() <= TEMP_TEXT);
        let text = sprintf(&plain, 1, &mut Generated(Vec::new())).unwrap();
        prop_assert_eq!(text, plain[..plain.len().min(TEMP_TEXT)].to_vec());
    }

    // spec BUILTIN-18.25: atof rounds a decimal number correctly, after white space and a
    // sign, and reads only the longest prefix that is a number
    #[test]
    fn atof_reads_a_decimal_number_exactly(
        value in any::<f64>().prop_filter("finite", |v| v.is_finite()),
        space in "[ \t\n\r]{0,3}",
        tail in "[g-z ]{0,4}",
    ) {
        for text in [format!("{value:e}"), format!("{value}"), format!("{value:+e}")] {
            let read = atof(format!("{space}{text}{tail}").as_bytes());
            prop_assert_eq!(read.to_bits(), value.to_bits(), "{}", text);
        }
    }

    // spec BUILTIN-18.25: a hexadecimal number is correctly rounded whatever its digits, so every
    // finite double written exactly in hexadecimal, padded with zero digits on either side,
    // reads back bit for bit
    #[test]
    fn atof_reads_an_exact_hexadecimal_number_back(
        value in any::<f64>().prop_filter("finite", |v| v.is_finite()),
        lead in 0_usize..300,
        trail in 0_usize..300,
    ) {
        let text = exact_hex(value, lead, trail);
        prop_assert_eq!(atof(text.as_bytes()).to_bits(), value.to_bits(), "{}", text);
    }

    // spec BUILTIN-18.25: a hexadecimal number reads as the nearest double to its exact value,
    // ties to even, whatever its digits, point and exponent: the double Rust's correctly
    // rounded decimal reader gives for its exact decimal expansion (P2-R1B re-review)
    #[test]
    fn rereview_atof_rounds_a_hexadecimal_number_as_its_exact_decimal(
        digits in prop_oneof![
            proptest::collection::vec(
                prop_oneof![Just(0_u32), Just(1), Just(8), Just(15), 0_u32..16],
                1..40,
            ),
            near_tie_digits(),
        ],
        point in any::<prop::sample::Index>(),
        exponent in -1250_i64..1100,
    ) {
        let fraction = point.index(digits.len() + 1);
        let split = digits.len() - fraction;
        let hex = |part: &[u32]| -> String {
            part.iter().map(|d| char::from_digit(*d, 16).unwrap()).collect()
        };
        let text = format!("0x{}.{}p{exponent}", hex(&digits[..split]), hex(&digits[split..]));
        let expected: f64 = exact_decimal(&digits, fraction, exponent).parse().unwrap();
        prop_assert_eq!(atof(text.as_bytes()).to_bits(), expected.to_bits(), "{}", text);
    }
}
