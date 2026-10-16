// SPDX-License-Identifier: GPL-3.0-or-later

//! The number vectors of a differential scenario, `docs/spec/vectors/scenario/numbers.txt`,
//! checked from the Rust side (P1-37).
//!
//! `cargo xtask oracles` runs each vector through the VM oracle, which reports only whether
//! it refuses the scenario. This module reads the vectors with the syntax of DIFF-2.7 and
//! DIFF-2.8 checked first and Rust's own `i64`, `f64` and `f32` parsing after it, as the
//! P1-13 runner reads a scenario, and checks both the verdict and the value of every vector.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code: a failed expect or panic is a failed test with a message"
)]

use std::fs;
use std::path::Path;

const VECTORS: &str = "docs/spec/vectors/scenario/numbers.txt";

/// The verdict a scenario reader gives one text: refused, or the value read, as the vector
/// file writes it (decimal for an integer, the bits in hexadecimal for a number).
fn read(directive: &str, text: &str) -> Option<String> {
    match directive {
        "frames" => {
            if !is_integer(text) {
                return None;
            }
            let value: i128 = text.parse().ok()?;
            (0..=1_000_000).contains(&value).then(|| value.to_string())
        }
        "frametime" => {
            if !is_number(text) {
                return None;
            }
            let value: f64 = text.parse().ok()?;
            (value > 0.0 && value <= 1.0).then(|| format!("{:016x}", value.to_bits()))
        }
        "model" | "v_angle" => {
            if !is_number(text) {
                return None;
            }
            let value: f32 = text.parse().ok()?;
            value
                .is_finite()
                .then(|| format!("{:08x}", value.to_bits()))
        }
        other => panic!("{VECTORS}: unknown directive `{other}`"),
    }
}

/// DIFF-2.7: an optional `-`, then one or more digits.
fn is_integer(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// DIFF-2.8: an optional `-`; digits with at most one `.`, at least one digit; and an optional
/// exponent of `e` or `E`, an optional sign and one or more digits.
fn is_number(text: &str) -> bool {
    let text = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (
            text.get(..at).unwrap_or(""),
            text.get(at..).map(|e| e.get(1..)),
        ),
        None => (text, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits_only = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok =
        digits_only(whole) && digits_only(fraction) && !(whole.is_empty() && fraction.is_empty());
    let exponent_ok = match exponent {
        None => true,
        Some(None) => false,
        Some(Some(e)) => {
            let e = e.strip_prefix(['+', '-']).unwrap_or(e);
            !e.is_empty() && digits_only(e)
        }
    };
    mantissa_ok && exponent_ok
}

fn vectors() -> Vec<(usize, String, String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(VECTORS);
    let text = fs::read_to_string(&path).expect("the number vectors are readable");
    let mut vectors = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let body = raw.split_once('#').map_or(raw, |(before, _)| before);
        let words: Vec<&str> = body.split_whitespace().collect();
        match words.as_slice() {
            [] => {}
            [directive, text, result] => vectors.push((
                index.saturating_add(1),
                (*directive).to_owned(),
                (*text).to_owned(),
                (*result).to_owned(),
            )),
            _ => panic!(
                "{VECTORS}:{}: expected <directive> <text> <result>",
                index.saturating_add(1)
            ),
        }
    }
    vectors
}

// spec DIFF-2.7: an integer is an optional `-` and digits, read in base 10, in range or refused
// spec DIFF-2.8: a number has the decimal syntax, is rounded once to the nearest double or f32,
// is finite, and frametime is above 0 and at most 1
// spec DIFF-2.9: a value that is not an integer or a number, or is out of range, is refused
#[test]
fn every_vector_reads_as_its_result_says() {
    let vectors = vectors();
    assert!(
        vectors.len() > 50,
        "{VECTORS} has {} vectors",
        vectors.len()
    );
    for (line, directive, text, result) in &vectors {
        let want = (result != "refused").then(|| result.clone());
        assert_eq!(
            read(directive, text),
            want,
            "{VECTORS}:{line}: {directive} {text}"
        );
    }
    for directive in ["frames", "frametime", "model", "v_angle"] {
        for refused in [false, true] {
            assert!(
                vectors
                    .iter()
                    .any(|(_, d, _, r)| d == directive && (r == "refused") == refused),
                "{VECTORS} has no {} vector for {directive}",
                if refused { "refused" } else { "accepted" }
            );
        }
    }
}

// spec DIFF-2.8: Rust's parsing alone accepts texts that are not numbers, so the syntax is
// checked before the value is read
#[test]
fn the_syntax_is_narrower_than_rust_parsing() {
    for text in ["+0.1", "inf", "-Infinity", "NaN"] {
        assert!(text.parse::<f64>().is_ok(), "{text}");
        assert!(!is_number(text), "{text}");
    }
    assert!("+1".parse::<i64>().is_ok());
    assert!(!is_integer("+1"));
}

// spec DIFF-2.8: each part of the number syntax, at and around its edges
#[test]
fn the_number_syntax_has_each_part() {
    for text in [
        "0", "-0", ".5", "5.", "-.5", "1.5", "1e5", "1E5", "1e+5", "1e-5", ".5e1", "5.e1",
    ] {
        assert!(is_number(text), "{text}");
    }
    for text in [
        "", "-", ".", "-.", "e1", ".e1", "1e", "1e+", "1e-", "1e1.5", "1..2", "1.2.3", "1e1e1",
        "--1", "-+1", "+1", " 1", "1 ", "0x10", "1_0", "1f",
    ] {
        assert!(!is_number(text), "{text}");
    }
}

// spec DIFF-2.7: each part of the integer syntax, at and around its edges
#[test]
fn the_integer_syntax_has_each_part() {
    for text in ["0", "-0", "007", "0000000000000000001", "-12"] {
        assert!(is_integer(text), "{text}");
    }
    for text in ["", "-", "+1", "1.", "1e2", "--1", "1-", " 1", "0x1"] {
        assert!(!is_integer(text), "{text}");
    }
}
