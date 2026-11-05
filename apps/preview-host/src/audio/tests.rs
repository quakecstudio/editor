// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use qs_sound::Output;

use super::{Queue, fill, seconds_of_samples};

fn queue(most: usize) -> Queue {
    Queue {
        samples: Arc::new(Mutex::new(VecDeque::new())),
        most,
    }
}

// The queue holds at most its limit: the frames past it are dropped, so a stall does not
// build up delay
#[test]
fn the_queue_drops_what_does_not_fit() {
    let mut q = queue(4);
    q.write(&[1, 2, 3]);
    q.write(&[4, 5, 6]);
    assert_eq!(*q.samples.lock().unwrap(), [1, 2, 3, 4]);
}

// The device's frames take the left and right samples in order, silence on its other
// channels and once the queue runs dry
#[test]
fn the_device_takes_stereo_frames_and_silence() {
    let mut q = queue(16);
    q.write(&[1, 2, 3, 4, 5]);
    let mut data = [9_i16; 9];
    fill(&q, &mut data, 3, |s| s);
    assert_eq!(data, [1, 2, 0, 3, 4, 0, 5, 0, 0]);
    let mut more = [9.0_f32; 2];
    fill(&q, &mut more, 2, |s| f32::from(s) / 32768.0);
    assert_eq!(more, [0.0, 0.0]);
}

// A quarter second at 48000 frames a second is 24000 stereo samples
#[test]
fn a_quarter_second_of_samples() {
    assert_eq!(seconds_of_samples(0.25, 48_000), 24_000);
}
