// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1999-2005 Id Software, Inc.
// Copyright (C) 2010-2018 O.Sezer <sezero@users.sourceforge.net>
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/bgmusic.c, BGM_PlayCDtrack, BGM_Stop, BGM_UpdateStream, BGM_Update

//! The music (`docs/spec/engine/rerelease.md` RRE-3, `docs/spec/engine/sound.md` SND-6): the
//! files a CD track number names, a CD image's audio tracks, and the stream that feeds the
//! mixer's raw samples as QuakeSpasm's `bgmusic.c` feeds them.

use crate::mixer::Mixer;

/// The directory the tracks are in (RRE-3).
pub const RRE_MUSIC_DIR: &str = "music";

/// The extensions QuakeSpasm's codecs read, in its handler order (RRE-3.1).
pub const RRE_MUSIC_EXTENSIONS: [&str; 8] =
    ["ogg", "opus", "mp3", "flac", "wav", "it", "s3m", "xm"];

/// A CD track and whether it loops, as `svc_cdtrack` names them (CLIENT-1.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    /// The track number.
    pub number: u8,
    /// Whether it loops; the server's `looptrack` is not read by QuakeSpasm, which always
    /// loops.
    pub looping: bool,
}

/// The files that may hold `track`, `music/track<NN>.<ext>` for each extension the host
/// decodes, in the order given (RRE-3.1); the host takes the one from the search path entry
/// of highest priority.
#[must_use]
pub fn track_files(track: u8, extensions: &[&str]) -> Vec<String> {
    extensions
        .iter()
        .map(|ext| format!("{RRE_MUSIC_DIR}/track{track:02}.{ext}"))
        .collect()
}

/// The most bytes read from a stream at once, `byte raw[16384]` (SND-6.2).
const STREAM_BYTES: usize = 16384;
/// A 16-bit sample's bytes, the only width this host streams.
const SAMPLE_WIDTH: usize = 2;
/// The samples of [`STREAM_BYTES`].
const STREAM_SAMPLES: usize = 8192;
/// A CD's audio: 16-bit stereo at 44100 frames a second (SND-6.4).
pub const SND_CD_RATE: u32 = 44100;
/// A raw CD image's sector, 2352 bytes of audio (SND-6.4).
pub const SND_CD_SECTOR: u64 = 2352;
/// A cue sheet's frames, sectors, a second (SND-6.4).
const CUE_FRAMES_PER_SECOND: u64 = 75;

/// What a stream gives on a read (SND-6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// This many samples were written, more than none.
    Samples(usize),
    /// The end of the stream.
    End,
    /// It failed; `S_CodecReadStream`'s negative result.
    Failed(i32),
}

/// A decoded music stream: 16-bit samples, interleaved by channel.
pub trait MusicStream {
    /// Frames a second.
    fn rate(&self) -> u32;
    /// One or two.
    fn channels(&self) -> u16;
    /// Fills the start of `out` with the next samples.
    fn read(&mut self, out: &mut [i16]) -> Read;
    /// Back to the start; false when it cannot, `S_CodecRewindStream`'s error.
    fn rewind(&mut self) -> bool;
}

/// `bgmstream` and its state (SND-6.2): the stream playing, whether it loops and whether it
/// is paused.
#[derive(Default)]
pub struct Music {
    stream: Option<Box<dyn MusicStream>>,
    looping: bool,
    paused: bool,
}

impl Music {
    /// No music.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `stream` playing from its start, the one before it stopped (RRE-3.3, SND-6.2).
    pub fn play(&mut self, mixer: &mut Mixer, stream: Box<dyn MusicStream>, looping: bool) {
        self.stop(mixer);
        self.stream = Some(stream);
        self.looping = looping;
    }

    /// `BGM_Stop`: the stream closed and `s_rawend = 0` (SND-6.2).
    pub fn stop(&mut self, mixer: &mut Mixer) {
        if self.stream.take().is_some() {
            mixer.stop_raw();
        }
        self.paused = false;
    }

    /// `BGM_Pause` and `BGM_Resume`.
    pub const fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Whether a stream is open.
    #[must_use]
    pub const fn playing(&self) -> bool {
        self.stream.is_some()
    }

    /// `BGM_UpdateStream` (SND-6.2): with music at `volume` above 0, the stream read into the
    /// mixer's raw samples until they hold `MAX_RAW_SAMPLES` frames past `paintedtime`; at its
    /// end a looping stream rewinds and any other stops. What it prints is returned.
    pub fn update(&mut self, mixer: &mut Mixer, volume: f32) -> Vec<String> {
        let mut printed = Vec::new();
        if self.paused || volume <= 0.0 {
            return printed;
        }
        let mut rewound = false;
        let mut raw = [0_i16; STREAM_SAMPLES];
        loop {
            let Some(stream) = self.stream.as_mut() else {
                return printed;
            };
            let room = mixer.raw_room();
            if room == 0 {
                return printed;
            }
            let rate = u64::from(stream.rate());
            let channels = usize::from(stream.channels().max(1));
            // `fileSamples = bufferSamples * bgmstream->info.rate / shm->speed`.
            let wanted = u64::try_from(room)
                .unwrap_or(u64::MAX)
                .saturating_mul(rate)
                .checked_div(u64::from(mixer.speed()))
                .unwrap_or(0);
            let wanted = usize::try_from(wanted).unwrap_or(usize::MAX);
            if wanted == 0 {
                return printed;
            }
            let frame_bytes = SAMPLE_WIDTH.saturating_mul(channels);
            let bytes = wanted.saturating_mul(frame_bytes).min(STREAM_BYTES);
            let samples = bytes.checked_div(SAMPLE_WIDTH).unwrap_or(0);
            let out = raw.get_mut(..samples).unwrap_or_default();
            match stream.read(out) {
                Read::Samples(read) => {
                    let read = read.min(samples);
                    let frames = read.checked_div(channels).unwrap_or(0);
                    let data = raw.get(..read).unwrap_or_default();
                    let rate = stream.rate();
                    let channels = stream.channels();
                    mixer.raw_samples(frames, rate, channels, data, volume);
                    rewound = false;
                }
                Read::End if self.looping => {
                    if rewound {
                        printed.push("Stream keeps returning EOF.".to_owned());
                        self.stop(mixer);
                        return printed;
                    }
                    if !stream.rewind() {
                        printed.push("Stream seek error (-1), stopping.".to_owned());
                        self.stop(mixer);
                        return printed;
                    }
                    rewound = true;
                }
                Read::End => {
                    self.stop(mixer);
                    return printed;
                }
                Read::Failed(code) => {
                    printed.push(format!("Stream read error ({code}), stopping."));
                    self.stop(mixer);
                    return printed;
                }
            }
        }
    }
}

/// One audio track of a cue sheet's single binary file: where its audio starts and where the
/// next track's starts, in bytes (SND-6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CueTrack {
    /// The track number.
    pub number: u8,
    /// The byte its `INDEX 01` is at.
    pub start: u64,
    /// The byte the next track starts at, its `INDEX 00` or else `INDEX 01`; `None` for the
    /// last, which runs to the end of the file.
    pub end: Option<u64>,
}

/// A cue sheet read (SND-6.4): its binary file and its audio tracks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CueSheet {
    /// The `FILE "<name>" BINARY` it names.
    pub file: String,
    /// Its `AUDIO` tracks, in order.
    pub tracks: Vec<CueTrack>,
}

impl CueSheet {
    /// Reads `text` (SND-6.4): `None` when it names no binary file, or more than one.
    #[must_use]
    pub fn read(text: &str) -> Option<Self> {
        let mut file = None;
        // Each track: its number, whether it is audio, its INDEX 00 and its INDEX 01.
        let mut tracks: Vec<(u8, bool, Option<u64>, Option<u64>)> = Vec::new();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            match words.next().map(str::to_ascii_uppercase).as_deref() {
                Some("FILE") => {
                    let rest = line.trim_start().get(4..).unwrap_or_default().trim();
                    let (name, kind) = match rest.strip_prefix('"') {
                        Some(quoted) => {
                            let (name, kind) = quoted.split_once('"')?;
                            (name.to_owned(), kind.trim())
                        }
                        None => {
                            let (name, kind) = rest.rsplit_once(' ')?;
                            (name.trim().to_owned(), kind.trim())
                        }
                    };
                    if file.is_some() || !kind.eq_ignore_ascii_case("BINARY") {
                        return None;
                    }
                    file = Some(name);
                }
                Some("TRACK") => {
                    let number = words.next()?.parse().ok()?;
                    let audio = words
                        .next()
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("AUDIO"));
                    tracks.push((number, audio, None, None));
                }
                Some("INDEX") => {
                    let index: u8 = words.next()?.parse().ok()?;
                    let sector = msf(words.next()?)?;
                    let byte = sector.saturating_mul(SND_CD_SECTOR);
                    let track = tracks.last_mut()?;
                    match index {
                        0 => track.2 = Some(byte),
                        1 => track.3 = Some(byte),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        let file = file?;
        let mut audio = Vec::new();
        for (i, &(number, is_audio, _, start)) in tracks.iter().enumerate() {
            let (true, Some(start)) = (is_audio, start) else {
                continue;
            };
            let end = tracks
                .get(i.saturating_add(1))
                .and_then(|&(_, _, pregap, next)| pregap.or(next));
            audio.push(CueTrack { number, start, end });
        }
        Some(Self {
            file,
            tracks: audio,
        })
    }

    /// The audio track `number`, if the sheet has it.
    #[must_use]
    pub fn track(&self, number: u8) -> Option<CueTrack> {
        self.tracks.iter().copied().find(|t| t.number == number)
    }
}

/// `mm:ss:ff` as sectors, 75 to a second (SND-6.4).
fn msf(text: &str) -> Option<u64> {
    let mut parts = text.split(':').map(str::parse::<u64>);
    let (Some(Ok(minutes)), Some(Ok(seconds)), Some(Ok(frames)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if seconds >= 60 || frames >= CUE_FRAMES_PER_SECOND {
        return None;
    }
    Some(
        minutes
            .saturating_mul(60)
            .saturating_add(seconds)
            .saturating_mul(CUE_FRAMES_PER_SECOND)
            .saturating_add(frames),
    )
}

#[cfg(test)]
mod tests;
