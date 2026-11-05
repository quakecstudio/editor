// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::expect_used,
    reason = "test code: a failed expect is a failed test"
)]

use qc_trace::{Changes, Record, Text};

use super::summary;

fn frame(frame: u32) -> Record {
    Record::Frame {
        frame,
        changes: Changes::default(),
    }
}

// spec TRACE-4.11: an engine trace's records are frames, numbered from 1 in order, and
// strings; anything else is not one
#[test]
fn an_engine_trace_is_frames_and_strings() {
    let string = Record::String {
        value: 7,
        text: Text::new(b"e1m1".to_vec()).expect("no NUL"),
    };
    assert_eq!(summary(&[frame(1), string.clone(), frame(2)]), Ok(2));
    assert_eq!(summary(&[]), Ok(0));
    assert!(summary(&[frame(2)]).is_err(), "frame 1 missing");
    assert!(summary(&[frame(1), frame(1)]).is_err(), "a repeat");
    let exit = Record::Exit {
        changes: Changes::default(),
    };
    assert!(summary(&[frame(1), exit]).is_err());
}
