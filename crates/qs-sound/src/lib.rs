// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview host's sound engine (P3-27, #241): the sounds the mixer knows and loads
//! (`sfx`), the channels a sound plays on, its place in stereo, the ambient and static
//! sounds and the mixer that writes 16-bit stereo frames (`mixer`), the music tracks the
//! rerelease ships as files (`music`), and where the frames go (`output`), with a null output
//! for headless runs.
//!
//! Implements `docs/spec/engine/sound.md` SND-1 to SND-5 and `docs/spec/engine/rerelease.md`
//! RRE-3.1.

mod mixer;
mod music;
mod output;
mod sfx;
#[cfg(test)]
mod tests;

pub use mixer::{
    Ambient, Channel, Listener, ListenerLeaf, LoopPaint, Mixer, MixerSpec, Random,
    SND_AMBIENT_FADE, SND_AMBIENT_LEVEL, SND_AMBIENT_MIN_VOLUME, SND_BUFFER_FRAMES,
    SND_DEFAULT_VOLUME, SND_FIRST_STATIC, SND_MAX_CHANNELS, SND_MAX_DYNAMIC_CHANNELS,
    SND_MAX_RAW_SAMPLES, SND_MIXAHEAD, SND_NOMINAL_CLIP_DIST, SND_NUM_AMBIENTS, SND_ORACLE_SPEED,
    SND_PACKET_ATTENUATION, SND_PACKET_VOLUME, SND_PAINTBUFFER_SIZE,
    SND_STATIC_ATTENUATION_DIVISOR, Settings, Start,
};
pub use music::{
    CueSheet, CueTrack, Music, MusicStream, RRE_MUSIC_DIR, RRE_MUSIC_EXTENSIONS, Read, SND_CD_RATE,
    SND_CD_SECTOR, Track, track_files,
};
pub use output::{NullOutput, Output};
pub use sfx::{
    Loaded, Reader, SND_MAX_NAME, SND_MAX_SFX, SfxId, SoundData, SoundError, SoundTable, Width,
};
