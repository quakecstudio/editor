// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the one diagnostic vocabulary.

#![expect(
    clippy::string_slice,
    clippy::panic,
    reason = "test code: slicing a known-ASCII code string by byte is sound here, and a panic \
              is a failed test"
)]

use crate::{Code, Diagnostic, Severity, Span};

// spec QCD-1.1: a code is E or W, four digits, the first two a group from 01 to 05
#[test]
fn code_is_letter_and_four_digits_in_a_known_group() {
    for code in Code::ALL {
        let text = code.as_str();
        let (letter, digits) = text.split_at(1);
        assert!(letter == "E" || letter == "W", "{text}");
        assert_eq!(digits.len(), 4, "{text}");
        assert!(digits.bytes().all(|b| b.is_ascii_digit()), "{text}");
        assert!(
            ["01", "02", "03", "04", "05"].contains(&&digits[..2]),
            "{text}"
        );
    }
}

// spec QCD-1.1: an E code is an error and a W code is a warning
#[test]
fn letter_decides_severity() {
    for code in Code::ALL {
        let letter = &code.as_str()[..1];
        assert_eq!(letter == "E", code.is_error(), "{}", code.as_str());
        let expected = if letter == "E" {
            Severity::Error
        } else {
            Severity::Warning
        };
        assert_eq!(code.severity(), expected, "{}", code.as_str());
    }
}

// spec QCD-1.2: a code is never reused for a different diagnostic, so no two variants share a
// code string
#[test]
fn no_code_string_is_shared() {
    let mut sorted = Code::ALL.map(Code::as_str).to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), Code::ALL.len());
}

#[test]
fn all_lists_every_code_in_order() {
    let listed = Code::ALL.map(Code::as_str).to_vec();
    let mut sorted = listed.clone();
    sorted.sort_unstable();
    assert_eq!(listed, sorted);
}

/// The code each active statement of the catalogue assigns, with the statement: a statement
/// `- **QCD-n.m** \`CODE\` ...`, read from `docs/spec/language-diagnostics.md`.
fn catalogue_codes() -> Vec<(String, String)> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/spec/language-diagnostics.md"
    );
    let Ok(text) = std::fs::read_to_string(path) else {
        panic!("{path} is readable");
    };
    let mut codes = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("- **QCD-") else {
            continue;
        };
        let Some((number, body)) = rest.split_once("** ") else {
            continue;
        };
        let Some(code) = body.strip_prefix('`').and_then(|b| b.split_once('`')) else {
            continue;
        };
        codes.push((code.0.to_owned(), format!("QCD-{number}")));
    }
    codes
}

// spec QCD-1.2: a code is never reused for a different diagnostic: no two active statements
// of the catalogue assign one code, and every code this crate names is one the catalogue
// assigns, so none can stand for a diagnostic the catalogue does not give it
#[test]
fn every_code_is_assigned_once_by_the_catalogue() {
    let codes = catalogue_codes();
    assert!(codes.len() >= Code::ALL.len(), "{} codes read", codes.len());
    for (i, (code, statement)) in codes.iter().enumerate() {
        let later = codes.iter().skip(i + 1).find(|(other, _)| other == code);
        assert!(
            later.is_none(),
            "{code} is assigned by {statement} and {later:?}"
        );
    }
    for code in Code::ALL {
        assert!(
            codes.iter().any(|(assigned, _)| assigned == code.as_str()),
            "{} is not assigned by any active statement",
            code.as_str()
        );
    }
}

// spec QCD-1.3: a diagnostic carries its code, message and primary span
#[test]
fn diagnostic_carries_code_message_and_span() {
    let diag = Diagnostic {
        code: Code::E0301,
        message: "Unknown value \"foo\"".to_owned(),
        span: Span::new(10, 13),
    };
    assert_eq!(diag.code, Code::E0301);
    assert_eq!(diag.message, "Unknown value \"foo\"");
    assert_eq!(diag.span, Span::new(10, 13));
    assert!(diag.is_error());
    assert_eq!(diag.severity(), Severity::Error);

    let warn = Diagnostic {
        code: Code::W0201,
        message: "local x is the global x".to_owned(),
        span: Span::new(0, 1),
    };
    assert!(!warn.is_error());
    assert_eq!(warn.severity(), Severity::Warning);
}

// No spec statement: QCD-1.3 asks only that a diagnostic has a primary span; its range and
// containment are this crate's helpers.
#[test]
fn span_helpers() {
    let span = Span::new(2, 5);
    assert_eq!(span.range(), 2..5);
    assert!(span.contains(Span::new(2, 5)));
    assert!(span.contains(Span::new(3, 4)));
    assert!(!span.contains(Span::new(1, 4)));
    assert!(!span.contains(Span::new(4, 6)));
}
