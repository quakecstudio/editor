// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/snd_mem.c, GetWavinfo, FindNextChunk, FindChunk, S_LoadSound

//! Reads the RIFF WAVE files Quake plays its sounds from, loop points included.
//!
//! [`get_wavinfo`] is id's `GetWavinfo`, with the bounds checks id's engine leaves out: it
//! finds the chunks the engine reads and returns their header values as a [`WavInfo`], and
//! rejects malformed input with a [`WavError`], never a panic. [`s_load_sound`] adds the
//! checks id's `S_LoadSound` makes before it plays a sound, and returns the sample bytes as
//! a zero-copy [`Sound`]. Resampling and mixing are the sound system's.
//!
//! The format is specified in `docs/spec/wav.md` (WAV-1 to WAV-3).

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use core::cmp::Ordering;
use core::fmt;

/// Bytes in a chunk's id and length (WAV-1.1).
pub const WAV_CHUNK_HEADER_SIZE: usize = 8;

/// The format tag of PCM samples, the only one the engine reads (WAV-2.3).
pub const WAV_FORMAT_PCM: i16 = 1;

/// Offset in the `cue ` chunk of the first cue point's sample offset (WAV-2.6).
pub const WAV_CUE_LOOPSTART_OFFSET: usize = 32;

/// Offset in the `LIST` chunk of the loop's sample count (WAV-2.7).
pub const WAV_LIST_LENGTH_OFFSET: usize = 24;

/// Offset in the `LIST` chunk of the `mark` that says it holds a loop length (WAV-2.7).
pub const WAV_LIST_MARK_OFFSET: usize = 28;

/// `loopstart` of a sound that does not loop (WAV-2.6, WAV-3.4).
pub const WAV_NO_LOOP: i32 = -1;

/// Why a WAV file could not be read or played.
///
/// The messages of [`WavError::BadLoopLength`], [`WavError::BadWidth`],
/// [`WavError::BadRate`] and [`WavError::Stereo`] are written to follow the sound's name,
/// as id's are; the others are whole messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavError {
    /// No `RIFF` chunk with the form type `WAVE` (WAV-2.1).
    MissingRiff,
    /// No `fmt ` chunk, or one whose fields lie past the end of the file (WAV-2.3, WAV-2.5).
    MissingFmt,
    /// The format tag is not PCM (WAV-2.3).
    NotPcm {
        /// The stored format tag.
        format: i16,
    },
    /// No `data` chunk (WAV-2.8).
    MissingData,
    /// The loop is longer than the data, negative, or overflows (WAV-2.10, WAV-2.11).
    BadLoopLength,
    /// The sample width is 0 (WAV-2.9), or one the engine does not play (WAV-3.2).
    BadWidth {
        /// Bytes per sample: bits per sample divided by 8.
        width: i32,
    },
    /// The sample rate is 0 or negative (WAV-3.3).
    BadRate {
        /// The stored rate.
        rate: i32,
    },
    /// The sound does not have exactly one channel (WAV-3.1).
    Stereo {
        /// The stored channel count.
        channels: i32,
    },
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRiff => f.write_str("Missing RIFF/WAVE chunks"),
            Self::MissingFmt => f.write_str("Missing fmt chunk"),
            Self::NotPcm { .. } => f.write_str("Microsoft PCM format only"),
            Self::MissingData => f.write_str("Missing data chunk"),
            Self::BadLoopLength => f.write_str("has a bad loop length"),
            Self::BadWidth { .. } => f.write_str("has a bad sample width"),
            Self::BadRate { .. } => f.write_str("has a bad sample rate"),
            Self::Stereo { .. } => f.write_str("is a stereo sample"),
        }
    }
}

impl core::error::Error for WavError {}

/// What id's `GetWavinfo` reads from a WAV file, its `wavinfo_t` (WAV-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WavInfo {
    /// Samples per second.
    pub rate: i32,
    /// Bytes per sample: the stored bits per sample divided by 8, truncated (WAV-2.4).
    pub width: i32,
    /// The number of channels.
    pub channels: i32,
    /// The sample the sound loops back to, or [`WAV_NO_LOOP`] (WAV-2.6).
    pub loopstart: i32,
    /// The number of samples (WAV-2.10).
    pub samples: i32,
    /// Offset of the first sample from the start of the file (WAV-2.10).
    pub dataofs: usize,
}

/// A chunk found by [`find_next_chunk`].
#[derive(Clone, Copy, Debug)]
struct Chunk {
    /// Offset of the chunk's id.
    start: usize,
    /// The stored length, never negative.
    len: i32,
    /// Offset of the chunk after it, which may be past the end of the file.
    next: usize,
}

/// The first chunk named `name` at or after `pos`, id's `FindNextChunk`.
///
/// Implements WAV-1.1 to WAV-1.5.
fn find_next_chunk(wav: &[u8], mut pos: usize, name: [u8; 4]) -> Option<Chunk> {
    loop {
        if pos >= wav.len() {
            return None;
        }
        let header = wav.get(pos..pos.checked_add(WAV_CHUNK_HEADER_SIZE)?)?;
        let len = le_i32(header, 4)?;
        let padded = u64::try_from(len).ok()?.saturating_add(1) & !1;
        let next = u64::try_from(pos)
            .ok()?
            .saturating_add(8)
            .saturating_add(padded);
        let next = usize::try_from(next).unwrap_or(usize::MAX);
        // One 32-bit comparison, which coverage-guided fuzzers can solve.
        if le_i32(header, 0) == Some(i32::from_le_bytes(name)) {
            return Some(Chunk {
                start: pos,
                len,
                next,
            });
        }
        pos = next;
    }
}

/// Reads a WAV file's header as id's `GetWavinfo` does.
///
/// Implements WAV-1 and WAV-2: finds `RIFF`/`WAVE`, `fmt `, the optional `cue ` and `LIST`
/// loop markers and `data`, and returns what they hold. The sample bytes are not checked
/// against the file (WAV-2.12).
///
/// # Errors
///
/// [`WavError::MissingRiff`], [`WavError::MissingFmt`], [`WavError::NotPcm`],
/// [`WavError::MissingData`], [`WavError::BadWidth`] for a width of 0, or
/// [`WavError::BadLoopLength`].
pub fn get_wavinfo(wav: &[u8]) -> Result<WavInfo, WavError> {
    let riff = find_next_chunk(wav, 0, *b"RIFF").ok_or(WavError::MissingRiff)?;
    let form = riff
        .start
        .checked_add(8)
        .and_then(|at| at.checked_add(4).map(|end| at..end));
    if form.and_then(|range| wav.get(range)) != Some(b"WAVE".as_slice()) {
        return Err(WavError::MissingRiff);
    }
    let iff_data = riff.start.saturating_add(12);

    let fmt = find_next_chunk(wav, iff_data, *b"fmt ").ok_or(WavError::MissingFmt)?;
    let field = |offset: usize| fmt.start.checked_add(offset);
    let format = field(8)
        .and_then(|at| le_i16(wav, at))
        .ok_or(WavError::MissingFmt)?;
    if format != WAV_FORMAT_PCM {
        return Err(WavError::NotPcm { format });
    }
    let channels = field(10).and_then(|at| le_i16(wav, at));
    let rate = field(12).and_then(|at| le_i32(wav, at));
    let bits = field(22).and_then(|at| le_i16(wav, at));
    let (Some(channels), Some(rate), Some(bits)) = (channels, rate, bits) else {
        return Err(WavError::MissingFmt);
    };
    // WAV-2.4: C division truncates toward zero, as `checked_div` does.
    let width = i32::from(bits.checked_div(8).unwrap_or_default());

    let mut loopstart = WAV_NO_LOOP;
    let mut loop_samples = 0_i64;
    if let Some(cue) = find_next_chunk(wav, iff_data, *b"cue ")
        && let Some(start) = cue
            .start
            .checked_add(WAV_CUE_LOOPSTART_OFFSET)
            .and_then(|at| le_i32(wav, at))
    {
        loopstart = start;
        if let Some(list) = find_next_chunk(wav, cue.next, *b"LIST")
            && list
                .start
                .checked_add(WAV_LIST_MARK_OFFSET)
                .and_then(|at| wav.get(at..at.checked_add(4)?))
                == Some(b"mark".as_slice())
            && let Some(count) = list
                .start
                .checked_add(WAV_LIST_LENGTH_OFFSET)
                .and_then(|at| le_i32(wav, at))
        {
            loop_samples = i64::from(start).saturating_add(i64::from(count));
        }
    }

    let data = find_next_chunk(wav, iff_data, *b"data").ok_or(WavError::MissingData)?;
    let Some(data_samples) = data.len.checked_div(width) else {
        return Err(WavError::BadWidth { width });
    };
    // WAV-2.10, WAV-2.11: a zero loop length means none; a negative one is an error.
    let samples = match loop_samples.cmp(&0) {
        Ordering::Equal => data_samples,
        Ordering::Less => return Err(WavError::BadLoopLength),
        Ordering::Greater => {
            let loop_samples = i32::try_from(loop_samples).map_err(|_| WavError::BadLoopLength)?;
            if data_samples < loop_samples {
                return Err(WavError::BadLoopLength);
            }
            loop_samples
        }
    };
    Ok(WavInfo {
        rate,
        width,
        channels: i32::from(channels),
        loopstart,
        samples,
        dataofs: data.start.saturating_add(WAV_CHUNK_HEADER_SIZE),
    })
}

/// A sound the engine plays: its header and the sample bytes that lie inside the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sound<'a> {
    /// The header values, with `channels` 1, `width` 1 or 2 and `rate` above 0 (WAV-3).
    pub info: WavInfo,
    /// The first `samples * width` bytes at `dataofs`, or as many of them as the file holds
    /// (WAV-2.12). The samples after them are silence.
    pub data: &'a [u8],
}

impl Sound<'_> {
    /// The sample at `index` as a signed 16-bit value: an 8-bit sample is `(byte - 128) << 8`
    /// as `ResampleSfx` makes it, and a sample past [`Sound::data`] or [`WavInfo::samples`]
    /// is silence (WAV-2.12, WAV-3.2).
    #[must_use]
    pub fn sample(&self, index: usize) -> i16 {
        if usize::try_from(self.info.samples).is_ok_and(|samples| index >= samples) {
            return 0;
        }
        if self.info.width == 2 {
            let at = index.saturating_mul(2);
            le_i16(self.data, at).unwrap_or_default()
        } else {
            let byte = self.data.get(index).copied().unwrap_or(128);
            i16::from_be_bytes([byte ^ 0x80, 0])
        }
    }
}

/// The directory id's `S_LoadSound` loads every sound from (WAV-3.5).
pub const WAV_SOUND_DIR: &str = "sound/";

/// The path id's `S_LoadSound` loads the sound `name` from: [`WAV_SOUND_DIR`] followed by the
/// name, as its `Q_strcpy(namebuffer, "sound/")` and `Q_strcat(namebuffer, s->name)` build it.
///
/// Implements WAV-3.5.
#[must_use]
pub fn s_load_sound_path(name: &str) -> String {
    format!("{WAV_SOUND_DIR}{name}")
}

/// The sound name the file at `path` on the search path is loaded by, the inverse of
/// [`s_load_sound_path`]: `None` for a file outside [`WAV_SOUND_DIR`], which `S_LoadSound`
/// never loads.
///
/// Implements WAV-3.5.
#[must_use]
pub fn sound_name(path: &[u8]) -> Option<&[u8]> {
    path.strip_prefix(WAV_SOUND_DIR.as_bytes())
}

/// Reads a WAV file and checks it is one the engine plays, as id's `S_LoadSound` does.
///
/// Implements WAV-3 on top of [`get_wavinfo`]: one channel, a width of 1 or 2 bytes and a
/// positive rate.
///
/// # Errors
///
/// Any error of [`get_wavinfo`], then [`WavError::Stereo`], [`WavError::BadWidth`] or
/// [`WavError::BadRate`].
pub fn s_load_sound(wav: &[u8]) -> Result<Sound<'_>, WavError> {
    let info = get_wavinfo(wav)?;
    if info.channels != 1 {
        return Err(WavError::Stereo {
            channels: info.channels,
        });
    }
    if info.width != 1 && info.width != 2 {
        return Err(WavError::BadWidth { width: info.width });
    }
    if info.rate <= 0 {
        return Err(WavError::BadRate { rate: info.rate });
    }
    let len = usize::try_from(info.samples)
        .unwrap_or_default()
        .saturating_mul(usize::try_from(info.width).unwrap_or_default());
    let end = info.dataofs.saturating_add(len).min(wav.len());
    let data = wav.get(info.dataofs..end).unwrap_or_default();
    Ok(Sound { info, data })
}

/// The signed little-endian 16-bit integer at `offset`, or `None` past the end of `bytes`.
fn le_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    read_array(bytes, offset).map(i16::from_le_bytes)
}

/// The signed little-endian 32-bit integer at `offset`, or `None` past the end of `bytes`.
fn le_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    read_array(bytes, offset).map(i32::from_le_bytes)
}

/// The `N` bytes at `offset`, or `None` past the end of `bytes`.
fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}
