// SPDX-License-Identifier: GPL-3.0-or-later

//! The rerelease's Ogg Vorbis music files (RRE-3.5), on tones generated for these tests.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division,
    reason = "test code: a failed unwrap, index or read is a failed test"
)]

use super::{OggError, OggStream};
use crate::music::{MusicStream, Read};

const MONO: &[u8] = include_bytes!("tone-mono.ogg");
const STEREO: &[u8] = include_bytes!("tone-stereo.ogg");
const THREE: &[u8] = include_bytes!("tone-3ch.ogg");

/// Every sample of `stream`, read `chunk` at a time.
fn all(stream: &mut OggStream, chunk: usize) -> Vec<i16> {
    let mut out = Vec::new();
    let mut buffer = vec![0_i16; chunk];
    loop {
        match stream.read(&mut buffer) {
            Read::Samples(n) => out.extend_from_slice(&buffer[..n]),
            Read::End => return out,
            Read::Failed(code) => panic!("read failed: {code}"),
        }
    }
}

// spec RRE-3.5: an Ogg Vorbis file streams its rate, its channels and its 16-bit samples
// interleaved, to the end, the same however much a read asks for
#[test]
fn an_ogg_file_streams_its_samples() {
    let mut mono = OggStream::new(MONO.to_vec()).unwrap();
    assert_eq!((mono.rate(), mono.channels()), (22050, 1));
    let samples = all(&mut mono, 4096);
    // A quarter of a second, give or take the encoder's last block; ffmpeg's sine peaks at
    // an eighth of full scale.
    assert!((5000..=6000).contains(&samples.len()), "{}", samples.len());
    assert!(
        samples.iter().any(|&s| s.unsigned_abs() > 2000),
        "a tone, not silence"
    );
    let mut again = OggStream::new(MONO.to_vec()).unwrap();
    assert_eq!(
        all(&mut again, 7),
        samples,
        "small reads give the same samples"
    );

    let mut stereo = OggStream::new(STEREO.to_vec()).unwrap();
    assert_eq!((stereo.rate(), stereo.channels()), (44100, 2));
    let samples = all(&mut stereo, 8192);
    assert_eq!(samples.len() % 2, 0);
    assert!(
        (21000..=24000).contains(&samples.len()),
        "{}",
        samples.len()
    );
}

// spec RRE-3.5: a rewind starts the file again, as a looping track does at its end
#[test]
fn a_rewind_starts_again() {
    let mut stream = OggStream::new(STEREO.to_vec()).unwrap();
    let first = all(&mut stream, 1024);
    assert_eq!(stream.read(&mut [0; 16]), Read::End);
    assert!(stream.rewind());
    assert_eq!(all(&mut stream, 1024), first);
}

// spec RRE-3.5: bytes that are not Ogg Vorbis, and a stream of more than two channels, are
// refused when the file is opened
#[test]
fn what_cannot_be_played_is_refused() {
    assert!(matches!(
        OggStream::new(Vec::new()),
        Err(OggError::Decode(_))
    ));
    assert!(matches!(
        OggStream::new(b"RIFF\0\0\0\0WAVEfmt ".to_vec()),
        Err(OggError::Decode(_))
    ));
    // A file cut short is refused or reads to an end or a failure, never a panic.
    for cut in [8, 64, 512, MONO.len() / 2] {
        if let Ok(mut stream) = OggStream::new(MONO[..cut].to_vec()) {
            let mut buffer = [0_i16; 256];
            for _ in 0..64 {
                if !matches!(stream.read(&mut buffer), Read::Samples(_)) {
                    break;
                }
            }
        }
    }
    let error = OggStream::new(THREE.to_vec())
        .err()
        .expect("three channels");
    assert!(matches!(error, OggError::Channels(3)));
    assert_eq!(
        error.to_string(),
        "3 channels, where the music plays one or two"
    );
}
