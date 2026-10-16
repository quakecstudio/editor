// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the WAV reader (WAV-1 to WAV-3): as given, and as a `RIFF`/`WAVE`
//! file whose chunks the bytes describe, so the fuzzer reaches every chunk the reader looks
//! for. Fails on a panic, and on sample bytes outside the file.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qs_wav::{get_wavinfo, s_load_sound};

/// The chunk ids the reader looks for, and one it skips.
const IDS: [[u8; 4]; 5] = [*b"fmt ", *b"cue ", *b"LIST", *b"data", *b"junk"];

fn check(data: &[u8]) {
    let info = get_wavinfo(data);
    if let Ok(sound) = s_load_sound(data) {
        assert_eq!(info, Ok(sound.info));
        assert!(sound.info.dataofs + sound.data.len() <= data.len());
        for index in [0, 1, usize::MAX] {
            let _sample = sound.sample(index);
        }
    }
}

/// A file of chunks: each is a selector byte (the id, and in its top bit whether the stored
/// length is negated), a length byte, then up to that many bytes of data.
fn chunks(mut data: &[u8]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
    while let [selector, len, rest @ ..] = data {
        let take = usize::from(*len).min(rest.len());
        let id = IDS[usize::from(*selector & 0x7F) % IDS.len()];
        let stored = if *selector & 0x80 == 0 {
            i32::from(*len)
        } else {
            -i32::from(*len)
        };
        bytes.extend_from_slice(&id);
        bytes.extend_from_slice(&stored.to_le_bytes());
        bytes.extend_from_slice(&rest[..take]);
        data = &rest[take..];
    }
    bytes
}

fuzz_target!(|data: &[u8]| {
    check(data);
    check(&chunks(data));
});
