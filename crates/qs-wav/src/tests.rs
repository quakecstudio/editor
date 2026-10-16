// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use crate::fixtures::{ChunkSpec, WavSpec, chunk, cue, fmt, list};
use crate::{
    Sound, WAV_CHUNK_HEADER_SIZE, WAV_CUE_LOOPSTART_OFFSET, WAV_FORMAT_PCM, WAV_LIST_LENGTH_OFFSET,
    WAV_LIST_MARK_OFFSET, WAV_NO_LOOP, WAV_SOUND_DIR, WavError, WavInfo, get_wavinfo, s_load_sound,
    s_load_sound_path, sound_name,
};

/// The header of the default fixture: 11025, 1, 1, no loop, 5 samples at 44.
const DEFAULT: WavInfo = WavInfo {
    rate: 11025,
    width: 1,
    channels: 1,
    loopstart: WAV_NO_LOOP,
    samples: 5,
    dataofs: 44,
};

fn info(spec: &WavSpec) -> Result<WavInfo, WavError> {
    get_wavinfo(&spec.build())
}

fn with_fmt(format: i16, channels: i16, rate: i32, bits: i16) -> WavSpec {
    let mut spec = WavSpec::default();
    spec.chunks[0] = fmt(format, channels, rate, bits);
    spec
}

// spec WAV-1.1, WAV-2.1, WAV-2.4, WAV-2.10: the first WAV-3 vector.
#[test]
fn reads_the_default_vector() {
    assert_eq!(WAV_CHUNK_HEADER_SIZE, 8);
    assert_eq!(info(&WavSpec::default()), Ok(DEFAULT));
}

// spec WAV-1.1, WAV-1.2: an odd-length chunk is padded, and chunks before `fmt ` are skipped.
#[test]
fn skips_padded_chunks() {
    let mut spec = WavSpec::default();
    spec.chunks.insert(0, chunk(b"junk", b"abc"));
    assert_eq!(
        info(&spec),
        Ok(WavInfo {
            dataofs: 56,
            ..DEFAULT
        })
    );
}

// spec WAV-1.2: ids are compared exactly, so `DATA` and `Fmt ` are not found.
#[test]
fn chunk_ids_are_case_sensitive() {
    let mut spec = WavSpec::default();
    spec.chunks[1].id = *b"DATA";
    assert_eq!(info(&spec), Err(WavError::MissingData));
    let mut spec = WavSpec::default();
    spec.chunks[0].id = *b"Fmt ";
    assert_eq!(info(&spec), Err(WavError::MissingFmt));
}

// spec WAV-1.2: the RIFF length is not used, so a chunk after the RIFF's end is found.
#[test]
fn riff_length_is_ignored() {
    let mut bytes = WavSpec::default().build();
    bytes[4..8].copy_from_slice(&4_i32.to_le_bytes());
    assert_eq!(get_wavinfo(&bytes), Ok(DEFAULT));
    // The RIFF chunk need not come first.
    let mut file = b"JUNK\x02\x00\x00\x00ab".to_vec();
    file.extend_from_slice(&WavSpec::default().build());
    assert_eq!(
        get_wavinfo(&file),
        Ok(WavInfo {
            dataofs: 54,
            ..DEFAULT
        })
    );
}

// spec WAV-1.3: a negative length ends the search.
#[test]
fn negative_length_stops_the_search() {
    let mut spec = WavSpec::default();
    spec.chunks.insert(
        1,
        ChunkSpec {
            len: Some(-2),
            ..chunk(b"junk", b"")
        },
    );
    assert_eq!(info(&spec), Err(WavError::MissingData));
    let mut spec = WavSpec::default();
    spec.chunks[1].len = Some(-1);
    assert_eq!(info(&spec), Err(WavError::MissingData));
}

// spec WAV-1.3: a chunk whose length runs past the end ends the search there.
#[test]
fn long_chunk_hides_the_rest() {
    let mut spec = WavSpec::default();
    spec.chunks.insert(
        1,
        ChunkSpec {
            len: Some(1000),
            ..chunk(b"junk", b"")
        },
    );
    assert_eq!(info(&spec), Err(WavError::MissingData));
}

// spec WAV-1.4: fewer than 8 bytes left for a chunk header is not found.
#[test]
fn truncated_chunk_header_is_not_found() {
    let mut bytes = WavSpec::default().build();
    let data = bytes.len() - 6 - 8;
    bytes.truncate(data + 7);
    assert_eq!(get_wavinfo(&bytes), Err(WavError::MissingData));
    for len in 0..12 {
        assert_eq!(
            get_wavinfo(&WavSpec::default().build()[..len]),
            Err(WavError::MissingRiff)
        );
    }
}

// spec WAV-1.5: a length of i32::MAX does not overflow; the search stops.
#[test]
fn maximal_length_stops_the_search() {
    let mut spec = WavSpec::default();
    spec.chunks.insert(
        1,
        ChunkSpec {
            len: Some(i32::MAX),
            ..chunk(b"junk", b"")
        },
    );
    assert_eq!(info(&spec), Err(WavError::MissingData));
}

// spec WAV-2.1: the form type must be `WAVE`.
#[test]
fn form_type_must_be_wave() {
    for form in [*b"WAVX", *b"wave", *b"AVI "] {
        let spec = WavSpec {
            form,
            ..WavSpec::default()
        };
        assert_eq!(info(&spec), Err(WavError::MissingRiff));
    }
    assert_eq!(get_wavinfo(b""), Err(WavError::MissingRiff));
    assert_eq!(
        get_wavinfo(b"RIFX\x04\x00\x00\x00WAVE"),
        Err(WavError::MissingRiff)
    );
    assert_eq!(
        WavError::MissingRiff.to_string(),
        "Missing RIFF/WAVE chunks"
    );
}

// spec WAV-2.2: sub-chunks are searched from 12 bytes after the RIFF chunk's start.
#[test]
fn search_starts_after_the_form_type() {
    // A `fmt ` chunk before the RIFF chunk is not seen.
    let format = fmt(1, 1, 11025, 8);
    let mut bytes = format.id.to_vec();
    bytes.extend_from_slice(&16_i32.to_le_bytes());
    bytes.extend_from_slice(&format.data);
    let riff = WavSpec {
        chunks: vec![chunk(b"data", b"ab")],
        ..WavSpec::default()
    };
    bytes.extend_from_slice(&riff.build());
    assert_eq!(get_wavinfo(&bytes), Err(WavError::MissingFmt));
}

// spec WAV-2.3: a missing `fmt ` chunk or a format tag other than 1 fails.
#[test]
fn fmt_must_be_pcm() {
    let spec = WavSpec {
        chunks: vec![chunk(b"data", b"abc")],
        ..WavSpec::default()
    };
    assert_eq!(info(&spec), Err(WavError::MissingFmt));
    assert_eq!(WavError::MissingFmt.to_string(), "Missing fmt chunk");
    assert_eq!(WAV_FORMAT_PCM, 1);
    for format in [0, 2, 3, -2] {
        assert_eq!(
            info(&with_fmt(format, 1, 11025, 8)),
            Err(WavError::NotPcm { format })
        );
    }
    assert_eq!(
        WavError::NotPcm { format: 3 }.to_string(),
        "Microsoft PCM format only"
    );
}

// spec WAV-2.4: width is bits / 8 truncated toward zero; other fields are as stored.
#[test]
fn width_truncates_toward_zero() {
    for (bits, width) in [(16, 2), (8, 1), (12, 1), (24, 3), (-16, -2), (-9, -1)] {
        let got = info(&with_fmt(1, 1, 11025, bits)).unwrap();
        assert_eq!(got.width, width, "bits {bits}");
    }
    let got = info(&with_fmt(1, 2, 22050, 16)).unwrap();
    assert_eq!((got.channels, got.rate, got.samples), (2, 22050, 2));
    let got = info(&with_fmt(1, -1, -5, 8)).unwrap();
    assert_eq!((got.channels, got.rate), (-1, -5));
}

// spec WAV-2.4: a short `fmt ` chunk's fields come from the bytes after it.
#[test]
fn short_fmt_reads_on() {
    let mut spec = WavSpec::default();
    let full = spec.chunks[0].data.clone();
    spec.chunks[0] = ChunkSpec {
        len: Some(2),
        ..chunk(b"fmt ", &full[..2])
    };
    // The bytes after the 2-byte chunk are the next chunk's: channels from its id, the
    // rate from its id and length, and the bits from its data.
    spec.chunks
        .insert(1, chunk(b"pad ", &[0, 0, 0, 0, 16, 0, 0, 0]));
    let got = info(&spec).unwrap();
    assert_eq!(got.channels, i32::from(i16::from_le_bytes(*b"pa")));
    assert_eq!(got.rate, i32::from_le_bytes([b'd', b' ', 8, 0]));
    assert_eq!(got.width, 2);
}

// spec WAV-2.5: `fmt ` fields past the end of the file fail.
#[test]
fn fmt_past_the_end_fails() {
    let bytes = WavSpec {
        chunks: vec![fmt(1, 1, 11025, 8)],
        ..WavSpec::default()
    }
    .build();
    for len in 20..bytes.len() {
        let got = get_wavinfo(&bytes[..len]);
        assert_eq!(got, Err(WavError::MissingFmt), "length {len}");
    }
    assert_eq!(get_wavinfo(&bytes), Err(WavError::MissingData));
    // Format tag 3 is reported before the later fields are missed.
    let bytes = WavSpec {
        chunks: vec![fmt(3, 1, 11025, 8)],
        ..WavSpec::default()
    }
    .build();
    assert_eq!(
        get_wavinfo(&bytes[..24]),
        Err(WavError::NotPcm { format: 3 })
    );
}

// spec WAV-2.6: a `cue ` chunk gives `loopstart`; without one it is -1.
#[test]
fn cue_gives_loopstart() {
    assert_eq!(WAV_CUE_LOOPSTART_OFFSET, 32);
    assert_eq!(WAV_NO_LOOP, -1);
    let got = info(&WavSpec::before_data(vec![cue(3)])).unwrap();
    assert_eq!((got.loopstart, got.samples), (3, 5));
    // The value is not checked.
    let got = info(&WavSpec::before_data(vec![cue(-7)])).unwrap();
    assert_eq!(got.loopstart, -7);
    // A cue chunk after `data` counts too.
    let mut spec = WavSpec::default();
    spec.chunks.push(cue(4));
    assert_eq!(info(&spec).unwrap().loopstart, 4);
}

// spec WAV-2.6: a cue chunk whose sample offset lies past the end is ignored.
#[test]
fn truncated_cue_is_ignored() {
    let mut spec = WavSpec::default();
    let mut short = cue(3);
    short.data.truncate(20);
    spec.chunks.push(short);
    let mut bytes = spec.build();
    bytes.truncate(bytes.len() - 2);
    assert_eq!(get_wavinfo(&bytes).unwrap().loopstart, WAV_NO_LOOP);
}

// spec WAV-2.7, WAV-2.10: a `LIST` chunk marked `mark` gives the loop's sample count.
#[test]
fn list_mark_gives_loop_length() {
    assert_eq!((WAV_LIST_LENGTH_OFFSET, WAV_LIST_MARK_OFFSET), (24, 28));
    let got = info(&WavSpec::before_data(vec![cue(3), list(2, b"mark")])).unwrap();
    assert_eq!((got.loopstart, got.samples), (3, 5));
    let got = info(&WavSpec::before_data(vec![cue(1), list(2, b"mark")])).unwrap();
    assert_eq!((got.loopstart, got.samples), (1, 3));
    // Any other purpose is not a loop length.
    let got = info(&WavSpec::before_data(vec![cue(1), list(2, b"Mark")])).unwrap();
    assert_eq!(got.samples, 5);
    // A LIST before the cue chunk is not searched.
    let got = info(&WavSpec::before_data(vec![list(2, b"mark"), cue(1)])).unwrap();
    assert_eq!(got.samples, 5);
    // A LIST with no cue chunk is not read.
    let got = info(&WavSpec::before_data(vec![list(2, b"mark")])).unwrap();
    assert_eq!((got.loopstart, got.samples), (WAV_NO_LOOP, 5));
    // Chunks between the cue and the LIST are skipped.
    let spec = WavSpec::before_data(vec![cue(1), chunk(b"junk", b"x"), list(1, b"mark")]);
    assert_eq!(info(&spec).unwrap().samples, 2);
}

// spec WAV-2.7: a `mark` past the end of the file is no mark.
#[test]
fn truncated_list_is_ignored() {
    let mut spec = WavSpec::default();
    spec.chunks.push(cue(1));
    let mut short = list(2, b"mark");
    short.data.truncate(22);
    spec.chunks.push(short);
    let bytes = spec.build();
    assert_eq!(get_wavinfo(&bytes).unwrap().samples, 5);
}

// spec WAV-2.8: no `data` chunk fails.
#[test]
fn missing_data_fails() {
    let spec = WavSpec {
        chunks: vec![fmt(1, 1, 11025, 8)],
        ..WavSpec::default()
    };
    assert_eq!(info(&spec), Err(WavError::MissingData));
    assert_eq!(WavError::MissingData.to_string(), "Missing data chunk");
}

// spec WAV-2.9: a width of 0 fails when the data is read.
#[test]
fn zero_width_fails() {
    for bits in [0, 1, 7, -7] {
        assert_eq!(
            info(&with_fmt(1, 1, 11025, bits)),
            Err(WavError::BadWidth { width: 0 })
        );
    }
    let spec = WavSpec {
        chunks: vec![fmt(1, 1, 11025, 0)],
        ..WavSpec::default()
    };
    assert_eq!(info(&spec), Err(WavError::MissingData));
    assert_eq!(
        format!("Sound t.wav {}", WavError::BadWidth { width: 0 }),
        "Sound t.wav has a bad sample width"
    );
}

// spec WAV-2.10: the loop may not be longer than the data; a zero loop length is ignored.
#[test]
fn loop_longer_than_data_fails() {
    let spec = WavSpec::before_data(vec![cue(3), list(4, b"mark")]);
    assert_eq!(info(&spec), Err(WavError::BadLoopLength));
    assert_eq!(
        format!("Sound t.wav {}", WavError::BadLoopLength),
        "Sound t.wav has a bad loop length"
    );
    let spec = WavSpec::before_data(vec![cue(0), list(0, b"mark")]);
    assert_eq!(info(&spec).unwrap().samples, 5);
    let spec = WavSpec::before_data(vec![cue(3), list(-3, b"mark")]);
    assert_eq!(info(&spec).unwrap().samples, 5);
}

// spec WAV-2.11: a negative or overflowing loop length fails.
#[test]
fn bad_loop_arithmetic_fails() {
    for (start, count) in [(1, -3), (i32::MAX, 1), (i32::MIN, -1), (-5, 1)] {
        let spec = WavSpec::before_data(vec![cue(start), list(count, b"mark")]);
        assert_eq!(info(&spec), Err(WavError::BadLoopLength), "{start} {count}");
    }
}

// spec WAV-2.12: the data length is not checked; only the bytes in the file are returned.
#[test]
fn long_data_is_not_checked() {
    let mut spec = WavSpec::default();
    spec.chunks[1].len = Some(1000);
    let bytes = spec.build();
    assert_eq!(bytes.len(), 50);
    let got = get_wavinfo(&bytes).unwrap();
    assert_eq!(got.samples, 1000);
    let sound = s_load_sound(&bytes).unwrap();
    assert_eq!(sound.data, &bytes[44..]);
    assert_eq!(sound.sample(5), -32768);
    assert_eq!(sound.sample(6), 0);
    assert_eq!(sound.sample(999), 0);
}

// spec WAV-3.1: only one channel plays.
#[test]
fn stereo_is_rejected() {
    for channels in [0, 2, -1] {
        assert_eq!(
            s_load_sound(&with_fmt(1, channels, 11025, 8).build()),
            Err(WavError::Stereo {
                channels: i32::from(channels)
            })
        );
    }
    assert_eq!(
        format!("t.wav {}", WavError::Stereo { channels: 2 }),
        "t.wav is a stereo sample"
    );
    // A file GetWavinfo rejects is not loaded either.
    assert_eq!(s_load_sound(b"junk"), Err(WavError::MissingRiff));
}

// spec WAV-3.5: `S_LoadSound` loads `sound/<name>` only, so a `.wav` elsewhere (the
// rerelease's haptics clips) is never loaded as a sound.
#[test]
fn sounds_load_from_sound_only() {
    assert_eq!(WAV_SOUND_DIR, "sound/");
    assert_eq!(
        s_load_sound_path("weapons/ax1.wav"),
        "sound/weapons/ax1.wav"
    );
    assert_eq!(
        sound_name(b"sound/weapons/ax1.wav"),
        Some(b"weapons/ax1.wav".as_slice())
    );
    assert_eq!(sound_name(b"sound/"), Some(b"".as_slice()));
    for elsewhere in [
        b"tactile/dualsense/weapons/ax1_vb.wav".as_slice(),
        b"sounds/ax1.wav",
        b"music/track02.wav",
        b"progs/sound/x.wav",
        b"sound",
        b"",
    ] {
        assert_eq!(sound_name(elsewhere), None, "{}", elsewhere.escape_ascii());
    }
    let name = "misc/talk.wav";
    let path = s_load_sound_path(name);
    assert_eq!(sound_name(path.as_bytes()), Some(name.as_bytes()));
}

// spec WAV-3.2: 8-bit samples are unsigned around 128 and 16-bit samples signed.
#[test]
fn sample_formats() {
    let bytes = WavSpec::default().build();
    let sound = s_load_sound(&bytes).unwrap();
    assert_eq!(sound.data, &[128, 160, 255, 0, 96]);
    let samples: Vec<i16> = (0..6).map(|i| sound.sample(i)).collect();
    assert_eq!(samples, [0, 32 << 8, 127 << 8, -32768, -32 << 8, 0]);

    let mut spec = with_fmt(1, 1, 22050, 16);
    spec.chunks[1] = chunk(b"data", &[0x34, 0x12, 0x00, 0x80, 0xFF]);
    let bytes = spec.build();
    let sound = s_load_sound(&bytes).unwrap();
    assert_eq!(sound.info.samples, 2);
    assert_eq!(sound.data, &[0x34, 0x12, 0x00, 0x80]);
    assert_eq!(
        [sound.sample(0), sound.sample(1), sound.sample(2)],
        [0x1234, i16::MIN, 0]
    );
}

// spec WAV-3.2: widths other than 1 and 2 are rejected.
#[test]
fn other_widths_are_rejected() {
    for (bits, width) in [(24, 3), (32, 4), (-8, -1)] {
        assert_eq!(
            s_load_sound(&with_fmt(1, 1, 11025, bits).build()),
            Err(WavError::BadWidth { width })
        );
    }
}

// spec WAV-3.3: the rate must be above 0.
#[test]
fn bad_rate_is_rejected() {
    for rate in [0, -1, i32::MIN] {
        assert_eq!(
            s_load_sound(&with_fmt(1, 1, rate, 8).build()),
            Err(WavError::BadRate { rate })
        );
    }
    assert!(s_load_sound(&with_fmt(1, 1, 1, 8).build()).is_ok());
    assert_eq!(
        format!("t.wav {}", WavError::BadRate { rate: 0 }),
        "t.wav has a bad sample rate"
    );
}

// spec WAV-3.4: `loopstart` is kept as read, even past the samples.
#[test]
fn loopstart_is_not_checked() {
    let bytes = WavSpec::before_data(vec![cue(99)]).build();
    let sound: Sound<'_> = s_load_sound(&bytes).unwrap();
    assert_eq!((sound.info.loopstart, sound.info.samples), (99, 5));
    let bytes = WavSpec::default().build();
    assert_eq!(s_load_sound(&bytes).unwrap().info.loopstart, WAV_NO_LOOP);
}

proptest! {
    // spec WAV-1.4, WAV-2.5, WAV-2.12: arbitrary bytes never panic the reader.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _info = get_wavinfo(&bytes);
        if let Ok(sound) = s_load_sound(&bytes) {
            prop_assert!(sound.data.len() <= bytes.len());
            let _sample = sound.sample(usize::MAX);
        }
    }

    // spec WAV-2.12: a valid header followed by arbitrary bytes never panics, and the
    // sample bytes always lie inside the file.
    #[test]
    fn arbitrary_tail_never_panics(tail in proptest::collection::vec(any::<u8>(), 0..256)) {
        let mut bytes = WavSpec::default().build();
        bytes.truncate(36);
        bytes.extend_from_slice(&tail);
        if let Ok(sound) = s_load_sound(&bytes) {
            prop_assert!(sound.info.dataofs <= bytes.len());
            for index in 0..8 {
                let _sample = sound.sample(index);
            }
        }
    }
}
