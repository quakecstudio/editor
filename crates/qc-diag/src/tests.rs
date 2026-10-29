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

/// One statement of the catalogue, from `docs/spec/requirements.tsv`: its ID, whether it is
/// active, and its text.
struct CatalogueRow {
    id: String,
    active: bool,
    text: String,
}

/// The statements of `language-diagnostics.md`, as `requirements.tsv` lists them.
fn catalogue() -> Vec<CatalogueRow> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/spec/requirements.tsv"
    );
    let Ok(index) = std::fs::read_to_string(path) else {
        panic!("{path} is readable");
    };
    let mut rows = Vec::new();
    for line in index.lines().skip(1) {
        let mut columns = line.splitn(5, '\t');
        let (Some(id), Some(spec), Some(_), Some(status), Some(text)) = (
            columns.next(),
            columns.next(),
            columns.next(),
            columns.next(),
            columns.next(),
        ) else {
            panic!("a row of five columns: {line}");
        };
        if spec == "language-diagnostics.md" {
            rows.push(CatalogueRow {
                id: id.to_owned(),
                active: status == "active",
                text: text.to_owned(),
            });
        }
    }
    rows
}

/// Whether `text` is a diagnostic code: `E` or `W` and four digits (QCD-1.1).
fn is_code(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some('E' | 'W')) && text.len() == 5 && chars.all(|c| c.is_ascii_digit())
}

/// The code an active statement assigns: its text starts with the code in backticks.
fn assigned_code(text: &str) -> Option<&str> {
    let (code, _) = text.strip_prefix('`')?.split_once('`')?;
    is_code(code).then_some(code)
}

/// The first code in backticks anywhere in `text`: the one a withdrawn statement gave.
fn first_code(text: &str) -> Option<&str> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .find(|word| is_code(word))
}

/// The statements a withdrawn statement names after "Replaced by" or "replaced by".
fn replacements(text: &str) -> Vec<&str> {
    let Some((_, rest)) = text.split_once("eplaced by ") else {
        return Vec::new();
    };
    // The list ends at the sentence's end: a `.` that is not inside an ID such as `QCD-8.13`.
    let list = rest.split_once(". ").map_or(rest, |(list, _)| list);
    list.split([',', ' '])
        .map(|word| word.trim_end_matches('.'))
        .filter(|word| word.starts_with("QCD-"))
        .collect()
}

// spec QCD-1.2: a code is never reused for a different diagnostic: no two active statements
// of the catalogue assign one code, every code this crate names is one the catalogue assigns,
// and the code of a withdrawn statement is assigned again only by a statement that replaced it
#[test]
fn every_code_is_assigned_once_by_the_catalogue() {
    let rows = catalogue();
    let codes: Vec<(&str, &str)> = rows
        .iter()
        .filter(|row| row.active)
        .filter_map(|row| assigned_code(&row.text).map(|code| (code, row.id.as_str())))
        .collect();
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
            codes.iter().any(|(assigned, _)| *assigned == code.as_str()),
            "{} is not assigned by any active statement",
            code.as_str()
        );
    }
    let mut withdrawn_codes = 0_usize;
    for row in rows.iter().filter(|row| !row.active) {
        let Some(code) = first_code(&row.text) else {
            continue;
        };
        withdrawn_codes += 1;
        let replaced_by = replacements(&row.text);
        for (assigned, statement) in &codes {
            assert!(
                assigned != &code || replaced_by.contains(statement),
                "{code} of the withdrawn {} is assigned again by {statement}, which did not \
                 replace it ({replaced_by:?})",
                row.id
            );
        }
    }
    assert!(
        withdrawn_codes > 0,
        "the catalogue has withdrawn codes to check"
    );
}

// No spec statement: the readers above, on the shapes the catalogue's statements take.
#[test]
fn the_catalogue_readers_read_codes_and_replacements() {
    assert_eq!(
        assigned_code("`E0120` \"macro expansion too large\""),
        Some("E0120")
    );
    assert_eq!(assigned_code("Under a profile, `E0120`"), None);
    assert_eq!(assigned_code("`ab` text"), None);
    assert_eq!(
        first_code("**Withdrawn**: it gave `W0312` for a `label`"),
        Some("W0312")
    );
    assert_eq!(first_code("no code `x`"), None);
    assert_eq!(
        replacements("it said so. Replaced by QCD-8.13 and QCD-8.14."),
        ["QCD-8.13", "QCD-8.14"]
    );
    assert_eq!(
        replacements("gone; replaced by QCD-7.30, the new one."),
        ["QCD-7.30"]
    );
    assert!(replacements("no replacement").is_empty());
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

#[test]
fn a_span_covers_its_bytes_and_an_empty_one_its_start() {
    let span = Span::new(3, 6);
    assert!(!span.covers(2));
    assert!(span.covers(3) && span.covers(5));
    assert!(!span.covers(6));
    let empty = Span::new(4, 4);
    assert!(empty.covers(4));
    assert!(!empty.covers(3) && !empty.covers(5));
}

#[test]
fn spans_join_and_measure() {
    assert_eq!(Span::new(2, 5).to(Span::new(9, 12)), Span::new(2, 12));
    assert_eq!(Span::new(3, 6).len(), 3);
    assert!(!Span::new(3, 6).is_empty());
    assert_eq!(Span::new(4, 4).len(), 0);
    assert!(Span::new(4, 4).is_empty());
    // An inverted span holds nothing.
    assert_eq!(Span::new(6, 3).len(), 0);
    assert!(Span::new(6, 3).is_empty());
}
