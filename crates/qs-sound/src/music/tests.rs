// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "test code: a failed unwrap or index is a failed test; the numbers are the spec's"
)]

use std::cell::RefCell;
use std::rc::Rc;

use super::{CueSheet, CueTrack, Music, MusicStream, Read, Repeat, SND_CD_SECTOR};
use crate::sfx::SoundTable;
use crate::{
    Listener, ListenerLeaf, Mixer, MixerSpec, Output, SND_MAX_RAW_SAMPLES, SND_PAINTBUFFER_SIZE,
};

/// The frames written, left and right interleaved.
#[derive(Default)]
struct Kept(Vec<i16>);

impl Output for Kept {
    fn write(&mut self, frames: &[i16]) {
        self.0.extend_from_slice(frames);
    }
}

/// A stream of `frames` stereo frames at `rate` whose samples count up from 1, or that fails
/// or ends at once, counting its rewinds.
struct Counting {
    rate: u32,
    frames: usize,
    at: usize,
    fail: bool,
    rewinds: Rc<RefCell<u32>>,
    rewind_ok: bool,
}

fn counting(rate: u32, frames: usize) -> Counting {
    Counting {
        rate,
        frames,
        at: 0,
        fail: false,
        rewinds: Rc::new(RefCell::new(0)),
        rewind_ok: true,
    }
}

impl MusicStream for Counting {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn channels(&self) -> u16 {
        2
    }

    fn read(&mut self, out: &mut [i16]) -> Read {
        if self.fail {
            return Read::Failed(-3);
        }
        let left = (self.frames - self.at) * 2;
        let n = out.len().min(left) & !1;
        if n == 0 {
            return Read::End;
        }
        for (i, sample) in out.iter_mut().take(n).enumerate() {
            *sample = i16::try_from((self.at * 2 + i) % 30000 + 1).unwrap_or(0);
        }
        self.at += n / 2;
        Read::Samples(n)
    }

    fn rewind(&mut self) -> bool {
        *self.rewinds.borrow_mut() += 1;
        self.at = 0;
        self.rewind_ok
    }
}

fn mixer(speed: u32) -> Mixer {
    Mixer::new(MixerSpec {
        speed,
        ..MixerSpec::default()
    })
}

/// SND-6.3's output of a raw frame with no sound effects: `raw >> 8`, clamped to 16 bits.
fn out(raw: i32) -> i16 {
    i16::try_from((raw >> 8).clamp(-32768, 32767)).unwrap()
}

/// Paints `frames` frames from `mixer` with no channels and gives them back.
fn paint(mixer: &mut Mixer, frames: i64) -> Vec<i16> {
    let mut table = SoundTable::new(mixer.speed(), Box::new(|_: &str| None));
    let mut out = Kept::default();
    let end = mixer.painted() + frames;
    mixer.paint_channels(&mut table, end, &mut out);
    out.0
}

// spec SND-6.1: raw samples step by rate / speed and are kept scaled by (int) (256 * volume)
// spec SND-6.3: they reach the output at `raw >> 8`, after the master volume
#[test]
fn raw_samples_are_resampled_scaled_and_painted() {
    let mut mix = mixer(11025);
    // 4 frames at twice the rate: src = i * 2, frames 0 and 2.
    let data = [100_i16, -100, 200, -200, 300, -300, 400, -400];
    mix.raw_samples(4, 22050, 2, &data, 0.5);
    assert_eq!(mix.raw_room(), SND_MAX_RAW_SAMPLES - 2);
    let painted = paint(&mut mix, 3);
    // 100 * (int) (256 * 0.5) = 12800, out at 12800 >> 8 = 50; frame 2 is 300.
    assert_eq!(painted, [50, -50, 150, -150, 0, 0]);
    // A mono stream is both sides; at half the rate each frame is used twice.
    let mut mono = mixer(11025);
    mono.raw_samples(2, 5513, 1, &[1000, 2000], 1.0);
    assert_eq!(mono.raw_room(), SND_MAX_RAW_SAMPLES - 4);
    let painted = paint(&mut mono, 4);
    assert_eq!(painted, [1000, 1000, 1000, 1000, 2000, 2000, 2000, 2000]);
}

// spec SND-6.1: the buffer wraps at MAX_RAW_SAMPLES; s_rawend starts at paintedtime
#[test]
fn the_raw_buffer_wraps_and_follows_the_paint() {
    let mut mix = mixer(11025);
    paint(&mut mix, 100);
    assert_eq!(
        mix.raw_room(),
        SND_MAX_RAW_SAMPLES,
        "rawend raised to painted"
    );
    let data = vec![7_i16; 2 * SND_MAX_RAW_SAMPLES];
    mix.raw_samples(SND_MAX_RAW_SAMPLES, 11025, 2, &data, 1.0);
    assert_eq!(mix.raw_room(), 0);
    let painted = paint(&mut mix, i64::try_from(SND_PAINTBUFFER_SIZE).unwrap());
    let seven = out(7 * 256);
    assert!(painted.iter().all(|&s| s == seven));
    assert_eq!(mix.raw_room(), SND_PAINTBUFFER_SIZE);
    mix.stop_raw();
    assert_eq!(
        mix.raw_room(),
        SND_MAX_RAW_SAMPLES,
        "BGM_Stop's s_rawend = 0"
    );
}

// spec SND-6.2: the stream fills the raw buffer to MAX_RAW_SAMPLES past paintedtime, reading
// at most 16384 bytes at a time, and stops at once when the volume is 0 or it is paused
#[test]
fn the_stream_fills_the_raw_buffer() {
    let mut mix = mixer(11025);
    let mut music = Music::new();
    music.play(&mut mix, Box::new(counting(11025, 100_000)), Repeat::Loop);
    assert!(music.update(&mut mix, 0.0).is_empty());
    assert_eq!(
        mix.raw_room(),
        SND_MAX_RAW_SAMPLES,
        "volume 0 plays nothing"
    );
    music.pause();
    music.update(&mut mix, 1.0);
    assert_eq!(mix.raw_room(), SND_MAX_RAW_SAMPLES, "paused");
    music.resume();
    music.update(&mut mix, 1.0);
    assert_eq!(mix.raw_room(), 0);
    let painted = paint(&mut mix, 1);
    assert_eq!(painted[0], 1, "the stream's first sample");
    // A CD's rate: 44100 into 11025 reads four times the frames it stores.
    let mut cd = mixer(11025);
    let stream = counting(44100, 100_000);
    music.play(&mut cd, Box::new(stream), Repeat::Loop);
    music.update(&mut cd, 1.0);
    assert_eq!(cd.raw_room(), 0);
    assert!(music.playing());
}

// spec SND-6.2: at the end a looping stream rewinds; one that keeps ending stops with
// "Stream keeps returning EOF."; a failed rewind stops; one not looping stops
#[test]
fn the_stream_loops_and_stops() {
    let mut mix = mixer(11025);
    let mut music = Music::new();
    let short = counting(11025, 1000);
    let rewinds = Rc::clone(&short.rewinds);
    music.play(&mut mix, Box::new(short), Repeat::Loop);
    assert!(music.update(&mut mix, 1.0).is_empty());
    assert_eq!(mix.raw_room(), 0, "looped to fill the buffer");
    assert!(*rewinds.borrow() >= 8);
    let mut mix = mixer(11025);
    let mut empty = Music::new();
    empty.play(&mut mix, Box::new(counting(11025, 0)), Repeat::Loop);
    assert_eq!(empty.update(&mut mix, 1.0), ["Stream keeps returning EOF."]);
    assert!(!empty.playing());
    let mut stuck = counting(11025, 0);
    stuck.rewind_ok = false;
    let mut music = Music::new();
    music.play(&mut mix, Box::new(stuck), Repeat::Loop);
    assert_eq!(
        music.update(&mut mix, 1.0),
        ["Stream seek error (-1), stopping."]
    );
    let mut once = Music::new();
    let mut fresh = mixer(11025);
    once.play(&mut fresh, Box::new(counting(11025, 10)), Repeat::Once);
    assert!(once.update(&mut fresh, 1.0).is_empty());
    assert!(!once.playing());
    assert_eq!(
        fresh.raw_room(),
        SND_MAX_RAW_SAMPLES,
        "BGM_Stop clears s_rawend"
    );
}

// spec SND-6.2: a read error stops the stream with its code
#[test]
fn a_read_error_stops_the_stream() {
    let mut mix = mixer(11025);
    let mut music = Music::new();
    let mut broken = counting(11025, 10);
    broken.fail = true;
    music.play(&mut mix, Box::new(broken), Repeat::Loop);
    assert_eq!(
        music.update(&mut mix, 1.0),
        ["Stream read error (-3), stopping."]
    );
    assert!(!music.playing());
}

// spec SND-6.4: a cue sheet's binary file and its audio tracks, each from its INDEX 01 to the
// next track's INDEX 00, else its INDEX 01, the last to the file's end; PREGAP adds nothing
#[test]
fn a_cue_sheet_names_its_audio_tracks() {
    let sheet = CueSheet::read(
        "FILE \"game.gog\" BINARY\r\n  TRACK 01 MODE1/2352\r\n    INDEX 01 00:00:00\r\n  \
         TRACK 02 AUDIO\r\n    PREGAP 00:02:00\r\n    INDEX 01 13:20:14\r\n  TRACK 03 AUDIO\r\n    \
         INDEX 00 18:30:00\r\n    INDEX 01 18:32:61\r\n  track 04 audio\r\n    INDEX 01 21:00:63\r\n",
    )
    .unwrap();
    assert_eq!(sheet.file, "game.gog");
    let sector = |m: u64, s: u64, f: u64| ((m * 60 + s) * 75 + f) * SND_CD_SECTOR;
    assert_eq!(
        sheet.tracks,
        [
            CueTrack {
                number: 2,
                start: sector(13, 20, 14),
                end: Some(sector(18, 30, 0))
            },
            CueTrack {
                number: 3,
                start: sector(18, 32, 61),
                end: Some(sector(21, 0, 63))
            },
            CueTrack {
                number: 4,
                start: sector(21, 0, 63),
                end: None
            },
        ]
    );
    assert_eq!(sheet.track(1), None, "the data track is not music");
    assert_eq!(sheet.track(3).map(|t| t.number), Some(3));
    let unquoted = CueSheet::read("FILE disc.bin BINARY\nTRACK 2 AUDIO\nINDEX 01 00:00:00\n");
    assert_eq!(unquoted.map(|s| s.file), Some("disc.bin".to_owned()));
}

// spec SND-6.4: no binary file, two files, a wave file, or a bad time reads as nothing
#[test]
fn a_cue_sheet_that_does_not_read() {
    assert_eq!(CueSheet::read(""), None);
    assert_eq!(CueSheet::read("TRACK 02 AUDIO\nINDEX 01 00:00:00\n"), None);
    assert_eq!(
        CueSheet::read("FILE \"a.bin\" BINARY\nFILE \"b.bin\" BINARY\n"),
        None
    );
    assert_eq!(CueSheet::read("FILE \"a.wav\" WAVE\n"), None);
    for time in ["00:60:00", "00:00:75", "1:2", "a:b:c", "00:00:00:00"] {
        let text = format!("FILE \"a.bin\" BINARY\nTRACK 02 AUDIO\nINDEX 01 {time}\n");
        assert_eq!(CueSheet::read(&text), None, "{time}");
    }
    assert_eq!(
        CueSheet::read("FILE \"a.bin\" BINARY\nINDEX 01 00:00:00\n"),
        None,
        "no track"
    );
}

// spec SND-6.3: an update's paint mixes the music in
#[test]
fn the_mixer_paints_music_in_an_update() {
    // The listener update paints the raw samples too, through Mixer::update.
    let mut mix = mixer(11025);
    mix.raw_samples(2, 11025, 2, &[512, 512, 512, 512], 1.0);
    let mut table = SoundTable::new(11025, Box::new(|_: &str| None));
    let mut kept = Kept::default();
    mix.update(
        &mut table,
        Listener::default(),
        ListenerLeaf::Outside,
        0.0,
        &mut kept,
    );
    assert!(kept.0.iter().take(4).all(|&s| s == 512));
}
