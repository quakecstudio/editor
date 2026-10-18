// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the one diagnostic vocabulary.

#![expect(
    clippy::string_slice,
    reason = "test code: slicing a known-ASCII code string by byte is sound here"
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

// spec QCD-1.2: a code is never reused for a different diagnostic, so no two codes share a
// string and a withdrawn code stays unassigned, which Code::ALL being this exact set holds
#[test]
fn no_code_string_is_shared() {
    let mut sorted = Code::ALL.map(Code::as_str).to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), Code::ALL.len());
}

// spec QCD-1.2: the withdrawn codes E0207 (QCD-3.7), E0403 (QCD-5.3), E0404 (QCD-5.4),
// E0405 (QCD-5.5) and E0501..=E0509 warnings stay unassigned: no variant carries them as a
// not-yet-used string, and the codes below are absent from the live set
#[test]
fn reserved_codes_are_absent() {
    let live: Vec<&str> = Code::ALL.map(Code::as_str).to_vec();
    for reserved in ["E0207", "E0403", "E0404", "E0405"] {
        assert!(!live.contains(&reserved), "{reserved} must stay unassigned");
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

#[test]
fn span_helpers() {
    let span = Span::new(2, 5);
    assert_eq!(span.range(), 2..5);
    assert!(span.contains(Span::new(2, 5)));
    assert!(span.contains(Span::new(3, 4)));
    assert!(!span.contains(Span::new(1, 4)));
    assert!(!span.contains(Span::new(4, 6)));
}
