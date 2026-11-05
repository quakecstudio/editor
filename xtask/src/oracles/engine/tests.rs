// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use qc_trace::{Changes, GlobalChange, Record, Text};

use super::{describe, summary};

fn frame(frame: u32) -> Record {
    Record::Frame {
        frame,
        changes: Changes::default(),
    }
}

// spec TRACE-4.11, TRACE-4.12: an engine trace's records are frame 0, then frames numbered
// from 1 in order, and strings, with one error record last; anything else is not one
#[test]
fn an_engine_trace_is_frames_and_strings() {
    let string = Record::String {
        value: 7,
        text: Text::new(b"e1m1".to_vec()).expect("no NUL"),
    };
    assert_eq!(
        summary(&[frame(0), frame(1), string.clone(), frame(2)]),
        Ok(2)
    );
    assert_eq!(summary(&[]), Ok(0));
    assert_eq!(summary(&[frame(0)]), Ok(0));
    assert!(summary(&[frame(1)]).is_err(), "frame 0 missing");
    assert!(summary(&[frame(0), frame(2)]).is_err(), "frame 1 missing");
    assert!(
        summary(&[frame(0), frame(1), frame(1)]).is_err(),
        "a repeat"
    );
    let exit = Record::Exit {
        changes: Changes::default(),
    };
    assert!(summary(&[frame(0), exit]).is_err());
    let error = Record::Error {
        function: 3,
        statement: 40,
        message: Text::new(b"NULL function".to_vec()).expect("no NUL"),
        changes: Changes::default(),
    };
    assert_eq!(
        summary(&[frame(0), frame(1), error.clone()]),
        Ok(1),
        "a run error ends the trace"
    );
    assert!(
        summary(&[error, frame(1)]).is_err(),
        "an error is the last record"
    );
}

fn header() -> qc_trace::Header {
    qc_trace::Header::new(1, 888, 108, 195, 10).expect("a valid header")
}

fn string(value: u32, text: &[u8]) -> Record {
    Record::String {
        value,
        text: Text::new(text.to_vec()).expect("no NUL"),
    }
}

fn global(slot: u32, bits: u32) -> Changes {
    Changes::new(vec![qc_trace::GlobalChange { slot, bits }], Vec::new()).expect("one change")
}

// spec TRACE-7.3: two builds that give their engine strings other values give the same
// canonical form, the values numbered by first appearance wherever they are; other values,
// and a trace whose strings name other texts, still differ
#[test]
fn the_canonical_form_numbers_engine_strings() {
    let trace = |a: u32, b: u32, text: &[u8]| qc_trace::Trace {
        header: header(),
        records: vec![
            string(a, b"e1m1"),
            string(b, text),
            Record::Frame {
                frame: 1,
                changes: global(3, b),
            },
        ],
    };
    let one = super::canonical(&trace(0x0400_0000, 0x0500_0000, b"start")).unwrap();
    let two = super::canonical(&trace(0x0700_0010, 0x0100_0000, b"start")).unwrap();
    assert_eq!(one, two);
    let Record::Frame { changes, .. } = &one.records[2] else {
        panic!("a frame");
    };
    assert_eq!(changes.globals()[0].bits, 0xffff_0001);
    let other = super::canonical(&trace(0x0400_0000, 0x0500_0000, b"end")).unwrap();
    assert_ne!(one, other);
    assert_eq!(super::first_difference(&one, &other), 1);
    assert_eq!(super::first_difference(&one, &one), 3);
}

#[test]
fn a_difference_names_the_slots_that_differ() {
    let with = |bits: &[(u32, u32)]| Record::Frame {
        frame: 1,
        changes: Changes::new(
            bits.iter()
                .map(|&(slot, bits)| GlobalChange { slot, bits })
                .collect(),
            Vec::new(),
        )
        .unwrap(),
    };
    let ours = with(&[(28, 0x384), (31, 1)]);
    let recorded = with(&[(28, 0x8214), (31, 1), (40, 2)]);
    assert_eq!(
        describe(Some(&ours), Some(&recorded)),
        "global 28: 00000384 recorded 00008214, global 40: - recorded 00000002"
    );
    assert!(describe(Some(&ours), None).ends_with("recorded no record"));
}
