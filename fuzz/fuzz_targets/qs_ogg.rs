// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the rerelease's music decoder (RRE-3.5): as an Ogg Vorbis file, and
//! as the generated tone's file with the bytes written over part of it, so the fuzzer reaches
//! past the headers into the audio packets. Fails on a panic, and on a read that writes more
//! samples than it was given room for.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qs_sound::{MusicStream, OggStream, Read};

/// The tone `qs-sound`'s tests decode.
const TONE: &[u8] = include_bytes!("../../crates/qs-sound/src/ogg/tone-mono.ogg");

fn play(bytes: Vec<u8>) {
    let Ok(mut stream) = OggStream::new(bytes) else {
        return;
    };
    assert!((1..=2).contains(&stream.channels()));
    let mut buffer = [0_i16; 509];
    for round in 0..2 {
        for _ in 0..256 {
            match stream.read(&mut buffer) {
                Read::Samples(n) => assert!(n <= buffer.len()),
                Read::End | Read::Failed(_) => break,
            }
        }
        if round == 0 && !stream.rewind() {
            return;
        }
    }
}

fuzz_target!(|data: &[u8]| {
    play(data.to_vec());
    // The first two bytes place the rest within the tone's file.
    if let [high, low, rest @ ..] = data {
        let mut tone = TONE.to_vec();
        let at = usize::from(u16::from_be_bytes([*high, *low])) % tone.len();
        for (byte, new) in tone.iter_mut().skip(at).zip(rest) {
            *byte = *new;
        }
        play(tone);
    }
});
