// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or match is a failed test"
)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use qs_sound::{MusicStream, Output, Read};

use super::{Audio, CdImage, Device, Queue, TrackFile, seconds_of_samples};

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
    q.fill(&mut data, 3, |s| s);
    assert_eq!(data, [1, 2, 0, 3, 4, 0, 5, 0, 0]);
    let mut more = [9.0_f32; 2];
    q.fill(&mut more, 2, |s| f32::from(s) / 32768.0);
    assert_eq!(more, [0.0, 0.0]);
}

// A quarter second at 48000 frames a second is 24000 stereo samples
#[test]
fn a_quarter_second_of_samples() {
    assert_eq!(seconds_of_samples(0.25, 48_000), 24_000);
}

/// A CD image in a fresh directory: `game.cue` and its `game.bin` of `bin` bytes, removed when
/// the test ends.
struct Image(std::path::PathBuf);

impl Image {
    fn new(name: &str, cue: &str, bin: &[u8]) -> Self {
        let dir =
            std::env::temp_dir().join(format!("quakec-engine-cd-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("game.cue"), cue).unwrap();
        std::fs::write(dir.join("game.bin"), bin).unwrap();
        Self(dir)
    }

    fn open(&self) -> CdImage {
        CdImage::open(&self.0.join("game.cue")).unwrap()
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

// spec SND-6.4: a track is read from its INDEX 01 as 16-bit little-endian stereo, whole frames
// only, to the next track's start, and rewinds to its own start
#[test]
fn a_cd_track_reads_whole_frames_of_its_own() {
    // Track 2 at sector 1, track 3 at sector 2; a sector is 2352 bytes.
    let mut bin = vec![0_u8; 2352 * 2 + 6];
    for (i, byte) in bin.iter_mut().enumerate().skip(2352) {
        *byte = u8::try_from(i % 251).unwrap();
    }
    let cue = "FILE \"game.bin\" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\n\
               TRACK 02 AUDIO\nINDEX 01 00:00:01\nTRACK 03 AUDIO\nINDEX 01 00:00:02\n";
    let image = Image::new("frames", cue, &bin);
    let cd = image.open();
    let mut two = cd.stream(2).unwrap();
    assert_eq!((two.rate(), two.channels()), (44100, 2));
    let mut out = [0_i16; 3];
    // Three samples asked: one whole frame of two read, the half frame left for the next read.
    assert_eq!(two.read(&mut out), Read::Samples(2));
    let first = i16::from_le_bytes([bin[2352], bin[2353]]);
    assert_eq!(out[0], first);
    let mut rest = vec![0_i16; 4096];
    assert_eq!(two.read(&mut rest), Read::Samples(1176 - 2));
    assert_eq!(
        rest[0],
        i16::from_le_bytes([bin[2356], bin[2357]]),
        "in step after the short read"
    );
    assert_eq!(two.read(&mut rest), Read::End, "track 3 starts there");
    assert!(two.rewind());
    assert_eq!(two.read(&mut out), Read::Samples(2));
    assert_eq!(out[0], first);
    // The last track runs to the end of the file, 6 bytes: one whole frame.
    let mut three = cd.stream(3).unwrap();
    assert_eq!(three.read(&mut rest), Read::Samples(2));
    assert_eq!(three.read(&mut rest), Read::End);
    assert!(cd.stream(1).is_none(), "the data track is not music");
    assert!(cd.stream(9).is_none());
}

// spec SND-6.4: a sheet that does not read, or names a file that is not there, gives no music
#[test]
fn a_cd_image_that_does_not_open() {
    let image = Image::new("broken", "TRACK 02 AUDIO\n", b"");
    assert!(CdImage::open(&image.0.join("game.cue")).is_none());
    assert!(CdImage::open(&image.0.join("missing.cue")).is_none());
    let lost = Image::new(
        "lost",
        "FILE \"other.bin\" BINARY\nTRACK 02 AUDIO\nINDEX 01 00:00:00\n",
        b"",
    );
    assert!(lost.open().stream(2).is_none());
}

/// The tone `qs-sound`'s Ogg tests decode, generated for them (ATTRIBUTION.md).
const TONE: &[u8] = include_bytes!("../../../../crates/qs-sound/src/ogg/tone-stereo.ogg");

// spec RRE-3.5: with no CD image the host plays `music/track<NN>.ogg` from the search path,
// `NN` the track as two digits, looping; no file is "Couldn't find a cdrip", and a file that
// does not open "Couldn't handle music file", and either leaves no music
#[test]
fn a_track_with_no_cd_plays_its_ogg_file() {
    let files: qs_sound::Reader = Box::new(|name: &str| match name {
        "music/track02.ogg" => Some(TONE.to_vec()),
        "music/track05.ogg" => Some(b"not ogg".to_vec()),
        _ => None,
    });
    let mut audio = Audio::open(Box::new(|_: &str| None), Device::None, None, files);
    let TrackFile::Found(stream) = audio.track_file(2) else {
        panic!("track 2's file");
    };
    assert_eq!((stream.rate(), stream.channels()), (44100, 2));
    assert!(matches!(audio.track_file(3), TrackFile::Missing));
    assert!(matches!(audio.track_file(12), TrackFile::Missing));
    assert!(matches!(
        audio.track_file(5),
        TrackFile::Undecodable(name) if name == "music/track05.ogg"
    ));
    audio.play_track(2);
    assert!(audio.music.playing());
    audio.play_track(5);
    assert!(
        !audio.music.playing(),
        "the track before stopped, and this one does not open"
    );
    audio.play_track(258);
    assert!(audio.music.playing(), "(byte)cl.cdtrack: 258 is track 2");
    audio.play_track(3);
    assert!(!audio.music.playing());
}
