// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the trace comparator and statement coverage (DIFF-9 to DIFF-11).
//!
//! Each test builds trace values directly with the `qc-trace` data model, so the behaviour
//! under test is the comparison, not a scenario run; the host-loop tests
//! (`crate::host::tests`) run whole scenarios. The spec-citation ratchet (`crates/qs-tests`)
//! holds DIFF-9 and DIFF-11 to these tests.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code: a failed expect or panic is a failed test"
)]

use qc_trace::{Changes, FieldChange, GlobalChange, Header, Record, Step, Text, Trace};

use super::{Comparison, Divergence, Slot, compare, coverage};

/// A header with the 32-bit edict layout (EDICT-1.3): `edict_size` 516, `field_offset` 96,
/// `entityfields` 105, `numstrings` 1000, matching the DIFF-9 equivalence vectors.
fn header_32() -> Header {
    Header::new(5927, 516, 96, 105, 1000).expect("a valid header")
}

/// A header with the 64-bit edict layout (EDICT-1.3): `edict_size` 528, `field_offset` 108,
/// the same `entityfields` and `numstrings`, so two traces may differ only in layout (DIFF-9.1).
fn header_64() -> Header {
    Header::new(5927, 528, 108, 105, 1000).expect("a valid header")
}

/// A change set holding the given global slots and field slots (TRACE-3).
fn changes(globals: &[(u32, u32)], fields: &[(u32, u32, u32)]) -> Changes {
    let globals = globals
        .iter()
        .map(|&(slot, bits)| GlobalChange { slot, bits })
        .collect();
    let fields = fields
        .iter()
        .map(|&(edict, field, bits)| FieldChange { edict, field, bits })
        .collect();
    Changes::new(globals, fields).expect("a valid change set")
}

/// A step record for `function` at `statement` with `opcode` and the given global changes
/// (TRACE-4.2).
fn step(function: i32, statement: i32, opcode: u16, globals: &[(u32, u32)]) -> Record {
    Record::Step(Step {
        function,
        statement,
        opcode,
        builtin: None,
        changes: changes(globals, &[]),
    })
}

/// No initial globals: every global defaults to 0 for these tests, which set the slots they
/// compare (DIFF-9.3).
const NO_GLOBALS: &[u32] = &[];

// spec DIFF-9.1: two traces with the same header and the same change-carrying records, each
// pair agreeing, compare equal.
// spec DIFF-9.2: corresponding step records with the same function, statement, opcode and
// builtin agree.
#[test]
fn two_agreeing_traces_compare_equal() {
    let records = vec![
        step(1, 1, 10, &[(30, 0x3f80_0000)]),
        step(1, 2, 11, &[(31, 0x4000_0000)]),
        Record::Exit {
            changes: changes(&[], &[]),
        },
    ];
    let a = Trace {
        header: header_32(),
        records: records.clone(),
    };
    let b = Trace {
        header: header_32(),
        records,
    };
    assert_eq!(compare(&a, &b, NO_GLOBALS), Comparison::Equal);
}

// spec DIFF-9.5: a seeded opcode bug is reported at the first differing record, with its
// statement; comparison stops there.
// spec DIFF-9.2: step records that disagree on their opcode are a divergence.
#[test]
fn a_seeded_opcode_bug_is_reported_with_its_statement() {
    // Trace A is correct; trace B has a wrong opcode at statement 2, a deliberate opcode bug.
    let a = Trace {
        header: header_32(),
        records: vec![
            step(1, 1, 10, &[]),
            step(1, 2, 11, &[]),
            step(1, 3, 12, &[]),
        ],
    };
    let b = Trace {
        header: header_32(),
        records: vec![
            step(1, 1, 10, &[]),
            step(1, 2, 99, &[]), // the bug: opcode 99 instead of 11
            step(1, 3, 12, &[]),
        ],
    };
    let Comparison::Failure(divergence) = compare(&a, &b, NO_GLOBALS) else {
        panic!("a divergence is reported");
    };
    assert_eq!(
        divergence,
        Divergence::Record {
            index: 1,
            statement_a: 2,
            statement_b: 2,
            what: "opcode",
        }
    );
    // DIFF-9.5: the divergence reports the first differing record's statement.
    assert_eq!(divergence.statement(), 2);
}

// spec DIFF-9.3: a divergence in a slot's value is reported, at the first record whose change
// set shows it.
#[test]
fn a_value_divergence_is_reported_with_its_statement() {
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(30, 0x3f80_0000)])],
    };
    let b = Trace {
        header: header_32(),
        // Same record shape, but global slot 30 holds a different, unrelated value.
        records: vec![step(1, 1, 10, &[(30, 0x4080_0000)])],
    };
    let Comparison::Failure(Divergence::Value {
        index,
        statement_a,
        slot,
        a: va,
        b: vb,
        ..
    }) = compare(&a, &b, NO_GLOBALS)
    else {
        panic!("a value divergence is reported");
    };
    assert_eq!(index, 0);
    assert_eq!(statement_a, 1);
    assert_eq!(slot, Slot::Global(30));
    assert_eq!(va, 0x3f80_0000);
    assert_eq!(vb, 0x4080_0000);
}

// spec DIFF-9.3: a global unchanged by either trace keeps its progs file default, so two traces
// that never write it agree even when the default is non-zero (PROGS-12.1).
#[test]
fn an_unwritten_global_reads_its_progs_default() {
    // Slot 0 is written to its default value in A only; both read slot 0 as 7, so they agree.
    let initial = &[7_u32, 0, 0];
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(0, 7)])],
    };
    let b = Trace {
        header: header_32(),
        // B never lists slot 0, so its value is the progs default 7 (DIFF-9.3).
        records: vec![step(1, 1, 10, &[(1, 0)])],
    };
    // Both change sets list a slot; slot 0 is 7 in both, slot 1 is 0 in both.
    assert_eq!(compare(&a, &b, initial), Comparison::Equal);
}

// spec DIFF-9.4: two engine strings are equivalent exactly when their latest texts are the same
// bytes, even though their numbers differ between runners (DIFF-3.8, EDICT-13.3).
#[test]
fn engine_strings_compare_by_text_not_number() {
    // A's string value is 2000 with text "player"; B's is 70000 with the same text. Both are
    // outside the string table (numstrings 1000), so each is an engine string of its trace.
    let text = Text::new(b"player".to_vec()).expect("text");
    let a = Trace {
        header: header_32(),
        records: vec![
            Record::String {
                value: 2000,
                text: text.clone(),
            },
            step(1, 1, 10, &[(40, 2000)]),
        ],
    };
    let b = Trace {
        header: header_64(),
        records: vec![
            Record::String { value: 70000, text },
            step(1, 1, 10, &[(40, 70000)]),
        ],
    };
    assert_eq!(compare(&a, &b, NO_GLOBALS), Comparison::Equal);
}

// spec DIFF-9.4: two engine strings whose latest texts differ are not equivalent.
#[test]
fn engine_strings_with_different_text_diverge() {
    let a = Trace {
        header: header_32(),
        records: vec![
            Record::String {
                value: 2000,
                text: Text::new(b"1".to_vec()).expect("text"),
            },
            step(1, 1, 10, &[(40, 2000)]),
        ],
    };
    let b = Trace {
        header: header_32(),
        records: vec![
            Record::String {
                value: 2000,
                text: Text::new(b"2".to_vec()).expect("text"),
            },
            step(1, 1, 10, &[(40, 2000)]),
        ],
    };
    assert!(matches!(
        compare(&a, &b, NO_GLOBALS),
        Comparison::Failure(Divergence::Value { .. })
    ));
}

// spec DIFF-9.4: two NaNs are equivalent whatever their payload (VM-4.3); -0 and +0 are not.
#[test]
fn nans_are_equivalent_but_signed_zeroes_are_not() {
    // 0x7fc00000 and 0xffc00000 are both NaNs: equivalent.
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 0x7fc0_0000)])],
    };
    let b = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 0xffc0_0000)])],
    };
    assert_eq!(compare(&a, &b, NO_GLOBALS), Comparison::Equal);

    // 0x80000000 (-0) against 0x00000000 (+0): not equivalent.
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 0x8000_0000)])],
    };
    let b = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 0x0000_0000)])],
    };
    assert!(matches!(
        compare(&a, &b, NO_GLOBALS),
        Comparison::Failure(Divergence::Value { .. })
    ));
}

// spec DIFF-9.4: an entity value naming the same edict is equivalent through each trace's own
// edict_size, even when the two layouts differ (TRACE-7.1).
#[test]
fn entity_values_compare_by_edict_number_through_each_header() {
    // Edict 2: 2 * 516 = 1032 in the 32-bit layout, 2 * 528 = 1056 in the 64-bit layout.
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 1032)])],
    };
    let b = Trace {
        header: header_64(),
        records: vec![step(1, 1, 10, &[(40, 1056)])],
    };
    assert_eq!(compare(&a, &b, NO_GLOBALS), Comparison::Equal);

    // Edict 3 (1548) against edict 2 (1056): different edicts, a divergence.
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 1548)])],
    };
    let b = Trace {
        header: header_64(),
        records: vec![step(1, 1, 10, &[(40, 1056)])],
    };
    assert!(matches!(
        compare(&a, &b, NO_GLOBALS),
        Comparison::Failure(Divergence::Value { .. })
    ));
}

// spec DIFF-9.4: a pointer value naming the same field slot of the same edict is equivalent
// through each trace's header (TRACE-7.2).
#[test]
fn pointer_values_compare_by_field_target_through_each_header() {
    // nextthink is field slot 46 of edict 2.
    // 32-bit: 2*516 + 96 + 4*46 = 1032 + 96 + 184 = 1312.
    // 64-bit: 2*528 + 108 + 4*46 = 1056 + 108 + 184 = 1348.
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 1312)])],
    };
    let b = Trace {
        header: header_64(),
        records: vec![step(1, 1, 10, &[(40, 1348)])],
    };
    assert_eq!(compare(&a, &b, NO_GLOBALS), Comparison::Equal);
}

// spec DIFF-9.5: when one trace has fewer change-carrying records, the first record the shorter
// one lacks is the divergence.
#[test]
fn a_shorter_trace_diverges_at_the_missing_record() {
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[]), step(1, 2, 11, &[])],
    };
    let b = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[])],
    };
    let Comparison::Failure(Divergence::Length {
        index,
        shorter_is_a,
    }) = compare(&a, &b, NO_GLOBALS)
    else {
        panic!("a length divergence is reported");
    };
    assert_eq!(index, 1);
    assert!(!shorter_is_a, "trace B is the shorter one");
}

// spec DIFF-9.1: headers that disagree on a field that must match (here numstrings) are a
// divergence; edict_size and field_offset may still differ.
#[test]
fn headers_that_disagree_on_numstrings_diverge() {
    let a = Trace {
        header: Header::new(5927, 516, 96, 105, 1000).expect("header"),
        records: vec![step(1, 1, 10, &[])],
    };
    let b = Trace {
        header: Header::new(5927, 516, 96, 105, 999).expect("header"),
        records: vec![step(1, 1, 10, &[])],
    };
    assert_eq!(
        compare(&a, &b, NO_GLOBALS),
        Comparison::Failure(Divergence::Header {
            field: "numstrings",
        })
    );
}

// spec DIFF-9.2: records with different tags (here a step against an exit) diverge.
#[test]
fn records_with_different_tags_diverge() {
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[])],
    };
    let b = Trace {
        header: header_32(),
        records: vec![Record::Exit {
            changes: changes(&[], &[]),
        }],
    };
    assert!(matches!(
        compare(&a, &b, NO_GLOBALS),
        Comparison::Failure(Divergence::Record { what: "tag", .. })
    ));
}

// spec DIFF-9.2: error records with different messages diverge, byte for byte.
#[test]
fn error_records_with_different_messages_diverge() {
    let error = |message: &str| Record::Error {
        function: 1,
        statement: 5,
        message: Text::new(message.as_bytes().to_vec()).expect("text"),
        changes: changes(&[], &[]),
    };
    let a = Trace {
        header: header_32(),
        records: vec![error("Bad opcode 70")],
    };
    let b = Trace {
        header: header_32(),
        records: vec![error("NULL function")],
    };
    assert!(matches!(
        compare(&a, &b, NO_GLOBALS),
        Comparison::Failure(Divergence::Record {
            what: "message",
            statement_a: 5,
            ..
        })
    ));
}

// spec DIFF-9.6: a divergence left after DIFF-10.2's readings is a failure (DIFF-10.3); the NaN
// and layout readings never turn a real value difference into equality.
// spec DIFF-10.2: NaN payloads and the edict layout are compared through, not treated as
// divergences, so they are not what makes this pair diverge.
#[test]
fn a_plain_value_difference_is_a_failure() {
    let a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 5)])],
    };
    let b = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[(40, 6)])],
    };
    assert!(matches!(
        compare(&a, &b, NO_GLOBALS),
        Comparison::Failure(Divergence::Value { .. })
    ));
}

// spec DIFF-11.1: a statement is executed when a step record has its index.
#[test]
fn executed_statements_are_the_step_indices() {
    let trace = Trace {
        header: header_32(),
        records: vec![
            step(1, 1, 10, &[]),
            step(1, 3, 11, &[]),
            step(1, 3, 12, &[]), // statement 3 executed twice, counted once
            Record::Exit {
                changes: changes(&[], &[]),
            },
        ],
    };
    let executed = coverage::executed(&trace, trace.records.len());
    assert_eq!(executed.into_iter().collect::<Vec<_>>(), vec![1, 3]);
}

// spec DIFF-11.2: coverage is the distinct statements executed over a set of traces divided by
// numstatements - 1, counting only the records before each trace's first divergence.
// spec DIFF-11.3: the report counts statements 1 to numstatements - 1.
#[test]
fn coverage_is_distinct_statements_over_total() {
    // A progs with numstatements 5: statements 1 to 4 (DIFF-11.1).
    let trace_a = Trace {
        header: header_32(),
        records: vec![step(1, 1, 10, &[]), step(1, 2, 11, &[])],
    };
    let trace_b = Trace {
        header: header_32(),
        records: vec![step(1, 2, 11, &[]), step(1, 4, 12, &[])],
    };
    // Distinct statements across both: 1, 2, 4 -> 3 of 4.
    let fraction = coverage::fraction(5, [(&trace_a, 2), (&trace_b, 2)]);
    assert!((fraction - 0.75).abs() < f64::EPSILON);

    // DIFF-11.2: only the records before the first divergence count. Here trace_b's second
    // step (statement 4) is excluded by passing records_before = 1, leaving 1 and 2 -> 2 of 4.
    let fraction = coverage::fraction(5, [(&trace_a, 2), (&trace_b, 1)]);
    assert!((fraction - 0.5).abs() < f64::EPSILON);
}

// spec DIFF-11.2: a progs with no statements to cover has coverage 0, not a divide by zero.
#[test]
fn coverage_of_a_progs_with_no_statements_is_zero() {
    let trace = Trace {
        header: header_32(),
        records: Vec::new(),
    };
    assert_eq!(coverage::fraction(1, [(&trace, 0)]), 0.0);
    assert_eq!(coverage::fraction(0, [(&trace, 0)]), 0.0);
}
