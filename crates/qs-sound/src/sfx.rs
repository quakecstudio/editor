// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/snd_mem.c, S_LoadSound, ResampleSfx
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/snd_dma.c, S_FindName, S_PrecacheSound

//! The sounds the mixer plays (`docs/spec/engine/sound.md` SND-1): known by name, loaded
//! through the game filesystem and resampled to the output rate.

use std::fmt;

use qc_vm::convert::{double_to_int, int_to_float};

/// The fraction bits of the resampler's fixed-point step: `stepscale * 256`, `>> 8` (SND-1.6).
const FRAC_BITS: u32 = 8;
/// A step of one in that fixed point, `1 << FRAC_BITS`.
const FRAC_ONE: f32 = 256.0;
/// A 16-bit sample's high byte, the 8-bit sample it makes (SND-1.6).
const HIGH_BYTE_SHIFT: u32 = 8;

/// The most sounds the table knows (SND-1.3).
pub const SND_MAX_SFX: usize = 512;

/// The longest sound name, `MAX_QPATH - 1` (SND-1.3).
pub const SND_MAX_NAME: usize = 63;

/// The width of a sound's samples (SND-1.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    /// One byte, signed, as `ResampleSfx` stores an 8-bit file's samples.
    Eight,
    /// Two bytes, little-endian signed.
    Sixteen,
}

/// A sound's samples at the output rate.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Samples {
    /// 8-bit samples, each `byte - 128` of the file (SND-1.6).
    Eight(Vec<i8>),
    /// 16-bit samples.
    Sixteen(Vec<i16>),
}

/// A loaded sound: `sfxcache_t` after `ResampleSfx` (SND-1.5, SND-1.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundData {
    /// The number of samples at the output rate.
    length: usize,
    /// The sample the sound loops back to, or `None` for a sound that stops.
    loopstart: Option<usize>,
    samples: Samples,
}

impl SoundData {
    /// A sound of `wav`'s samples resampled to `speed` samples per second, `ResampleSfx`
    /// (SND-1.5, SND-1.6): `stepscale = rate / speed`, the output count and the loop start
    /// each truncated after the division, and each output sample read at `(i * fracstep) >> 8`
    /// with `fracstep = (int) (stepscale * 256)`.
    ///
    /// `speed` is the output rate in samples per second; a `wav` with other than one channel
    /// is refused by [`SoundTable::load`], not here.
    #[must_use]
    pub fn resample(wav: &qs_wav::Sound<'_>, speed: u32) -> Self {
        let rate = int_to_float(wav.info.rate);
        let stepscale = rate / int_to_float(i32::try_from(speed).unwrap_or(i32::MAX));
        let count = |samples: i32| {
            let scaled = int_to_float(samples) / stepscale;
            usize::try_from(double_to_int(f64::from(scaled))).unwrap_or(0)
        };
        let length = count(wav.info.samples);
        let loopstart = if wav.info.loopstart == qs_wav::WAV_NO_LOOP {
            None
        } else {
            Some(count(wav.info.loopstart))
        };
        // SND-1.6: `fracstep = stepscale * 256`, stored in an int.
        let fracstep = double_to_int(f64::from(stepscale * FRAC_ONE));
        // Reserve no more than the file holds: a low rate makes `length` many times it.
        let held = usize::try_from(wav.info.samples).unwrap_or(0);
        let mut samplefrac = 0_i32;
        let source = |samplefrac: i32| {
            // `srcsample = samplefrac >> 8`; a negative one (a rate too high for an int)
            // reads nothing, as the sample is silence past the data.
            usize::try_from(samplefrac >> FRAC_BITS).unwrap_or(usize::MAX)
        };
        let samples = match wav.info.width {
            1 => {
                let mut out = Vec::with_capacity(length.min(held));
                for _ in 0..length {
                    let sample = wav.sample(source(samplefrac));
                    samplefrac = samplefrac.wrapping_add(fracstep);
                    // SND-1.6: `((signed char *) sc->data)[i] = sample >> 8`.
                    let [high, _] = (sample >> HIGH_BYTE_SHIFT).to_le_bytes();
                    out.push(i8::from_le_bytes([high]));
                }
                Samples::Eight(out)
            }
            _ => {
                let mut out = Vec::with_capacity(length.min(held));
                for _ in 0..length {
                    out.push(wav.sample(source(samplefrac)));
                    samplefrac = samplefrac.wrapping_add(fracstep);
                }
                Samples::Sixteen(out)
            }
        };
        Self {
            length,
            loopstart,
            samples,
        }
    }

    /// The number of samples.
    #[must_use]
    pub const fn length(&self) -> usize {
        self.length
    }

    /// The sample the sound loops back to, `None` for a sound that stops (SND-5.2).
    #[must_use]
    pub const fn loopstart(&self) -> Option<usize> {
        self.loopstart
    }

    /// The samples' width (SND-5.3).
    #[must_use]
    pub const fn width(&self) -> Width {
        match self.samples {
            Samples::Eight(_) => Width::Eight,
            Samples::Sixteen(_) => Width::Sixteen,
        }
    }

    /// The 8-bit sample at `at`, 0 past the end; `None` for a 16-bit sound.
    #[must_use]
    pub fn sample_8(&self, at: SamplePos) -> Option<i8> {
        match &self.samples {
            Samples::Eight(samples) => Some(samples.get(at.0).copied().unwrap_or(0)),
            Samples::Sixteen(_) => None,
        }
    }

    /// The 16-bit sample at `at`, 0 past the end; `None` for an 8-bit sound.
    #[must_use]
    pub fn sample_16(&self, at: SamplePos) -> Option<i16> {
        match &self.samples {
            Samples::Sixteen(samples) => Some(samples.get(at.0).copied().unwrap_or(0)),
            Samples::Eight(_) => None,
        }
    }
}

/// A sample's place in a sound, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SamplePos(usize);

impl SamplePos {
    /// Sample `index`.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its index.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// A sound the table knows, by index (SND-1.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SfxId(usize);

impl SfxId {
    /// The index in the table.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Why a sound could not be known or loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundError {
    /// `Sound name too long: <name>` (SND-1.3).
    NameTooLong(String),
    /// `S_FindName: out of sfx_t` (SND-1.3).
    OutOfSfx,
}

impl fmt::Display for SoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameTooLong(name) => write!(f, "Sound name too long: {name}"),
            Self::OutOfSfx => f.write_str("S_FindName: out of sfx_t"),
        }
    }
}

impl std::error::Error for SoundError {}

/// What loading a sound's file came to (SND-1.4), kept for the console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loaded {
    /// The sound has its data.
    Data,
    /// `Couldn't load sound/<name>`.
    Missing,
    /// `<name> is a stereo sample`.
    Stereo,
    /// The file is not a WAV the engine reads (WAV-3).
    Refused(String),
}

/// One known sound.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Sfx {
    name: String,
    data: Option<SoundData>,
    loaded: Option<Loaded>,
}

/// Gives the bytes of a file of the game filesystem by its path, `COM_LoadFile`.
pub type Reader = Box<dyn FnMut(&str) -> Option<Vec<u8>>>;

/// The sounds the engine knows, `known_sfx` (SND-1.3), and their data (SND-1.4), loaded
/// through the game filesystem when a sound is precached or first played.
pub struct SoundTable {
    sfx: Vec<Sfx>,
    /// The output rate the data is resampled to (SND-1.5).
    speed: u32,
    read: Reader,
}

impl fmt::Debug for SoundTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SoundTable")
            .field("sfx", &self.sfx)
            .field("speed", &self.speed)
            .finish_non_exhaustive()
    }
}

impl SoundTable {
    /// An empty table whose sounds are read through `read` and resampled to `speed` samples
    /// per second.
    #[must_use]
    pub fn new(speed: u32, read: Reader) -> Self {
        Self {
            sfx: Vec::new(),
            speed,
            read,
        }
    }

    /// `S_FindName`: the sound named `name`, known from now on (SND-1.3).
    ///
    /// # Errors
    ///
    /// [`SoundError::NameTooLong`] past [`SND_MAX_NAME`] bytes; [`SoundError::OutOfSfx`] for
    /// a new name once [`SND_MAX_SFX`] are known.
    pub fn find(&mut self, name: &str) -> Result<SfxId, SoundError> {
        if name.len() > SND_MAX_NAME {
            return Err(SoundError::NameTooLong(name.to_owned()));
        }
        if let Some(index) = self.sfx.iter().position(|sfx| sfx.name == name) {
            return Ok(SfxId(index));
        }
        if self.sfx.len() == SND_MAX_SFX {
            return Err(SoundError::OutOfSfx);
        }
        self.sfx.push(Sfx {
            name: name.to_owned(),
            data: None,
            loaded: None,
        });
        Ok(SfxId(self.sfx.len().saturating_sub(1)))
    }

    /// `S_PrecacheSound` with `precache` 1: the sound named `name`, loaded (SND-1.3,
    /// SND-1.4).
    ///
    /// # Errors
    ///
    /// As [`SoundTable::find`]. A file that is missing or not playable is not an error: the
    /// sound is known without data and [`SoundTable::loaded`] says why.
    pub fn precache(&mut self, name: &str) -> Result<SfxId, SoundError> {
        let id = self.find(name)?;
        self.load(id);
        Ok(id)
    }

    /// `S_LoadSound`: loads `id`'s data if it was never tried (SND-1.4), as every start of a
    /// sound does (SND-2.4, SND-4.2, SND-5.2).
    pub fn load(&mut self, id: SfxId) {
        let Some(sfx) = self.sfx.get_mut(id.0) else {
            return;
        };
        if sfx.loaded.is_some() {
            return;
        }
        let path = qs_wav::s_load_sound_path(&sfx.name);
        let Some(bytes) = (self.read)(&path) else {
            sfx.loaded = Some(Loaded::Missing);
            return;
        };
        match qs_wav::s_load_sound(&bytes) {
            Ok(wav) => {
                sfx.data = Some(SoundData::resample(&wav, self.speed));
                sfx.loaded = Some(Loaded::Data);
            }
            Err(qs_wav::WavError::Stereo { .. }) => sfx.loaded = Some(Loaded::Stereo),
            Err(error) => sfx.loaded = Some(Loaded::Refused(error.to_string())),
        }
    }

    /// `id`'s data, `None` until it is loaded or when it could not be.
    #[must_use]
    pub fn data(&self, id: SfxId) -> Option<&SoundData> {
        self.sfx.get(id.0)?.data.as_ref()
    }

    /// What loading `id` came to, `None` before it was tried.
    #[must_use]
    pub fn loaded(&self, id: SfxId) -> Option<&Loaded> {
        self.sfx.get(id.0)?.loaded.as_ref()
    }

    /// The output rate the data is resampled to.
    #[must_use]
    pub const fn speed(&self) -> u32 {
        self.speed
    }

    /// `id`'s name.
    #[must_use]
    pub fn name(&self, id: SfxId) -> Option<&str> {
        self.sfx.get(id.0).map(|sfx| sfx.name.as_str())
    }

    /// How many sounds are known.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.sfx.len()
    }

    /// Whether no sound is known.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.sfx.is_empty()
    }
}
