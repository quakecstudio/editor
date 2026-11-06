// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2007-2008 Kristian Duske
// Copyright (C) 2010-2011 O. Sezer <sezero@users.sourceforge.net>
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/snd_dma.c, S_StartSound, SND_PickChannel, SND_Spatialize, S_StopSound, S_StopAllSounds, S_StaticSound, S_UpdateAmbientSounds, S_Update, S_Update_, S_LocalSound
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/snd_mix.c, S_PaintChannels, SND_PaintChannelFrom8, SND_PaintChannelFrom16, SND_InitScaletable, S_TransferPaintBuffer, Snd_WriteLinearBlastStereo16
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/snd_dma.c, S_RawSamples
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/snd_mix.c, S_PaintChannels

//! The channels and the mixer (`docs/spec/engine/sound.md` SND-1 to SND-5): a sound starts
//! on a channel, is placed in stereo by the listener, and the channels are painted into
//! 16-bit stereo frames.

use qc_builtins::math::vector_normalize;
use qc_vm::convert::{double_to_float, double_to_int, float_to_int, int_to_float};

use crate::output::Output;
use crate::sfx::{SfxId, SoundData, SoundTable, Width};

/// The channels in the mixer (SND-1.1).
pub const SND_MAX_CHANNELS: usize = 128;
/// The dynamic channels, which `S_StartSound` picks among (SND-1.1).
pub const SND_MAX_DYNAMIC_CHANNELS: usize = 8;
/// The ambient channels, the first in the array (SND-1.1).
pub const SND_NUM_AMBIENTS: usize = 4;
/// The index of the first static channel (SND-1.1).
pub const SND_FIRST_STATIC: usize = SND_NUM_AMBIENTS + SND_MAX_DYNAMIC_CHANNELS;
/// Where a sound at attenuation 1 falls silent, in units (SND-2.3).
pub const SND_NOMINAL_CLIP_DIST: f32 = 1000.0;
/// A sound message's volume when it carries none (SND-2).
pub const SND_PACKET_VOLUME: u8 = 255;
/// A sound message's attenuation when it carries none (SND-2).
pub const SND_PACKET_ATTENUATION: f32 = 1.0;
/// A static sound's attenuation is divided by this (SND-4.2).
pub const SND_STATIC_ATTENUATION_DIVISOR: f32 = 64.0;
/// The `ambient_level` setting's default (SND-4.3).
pub const SND_AMBIENT_LEVEL: f32 = 0.3;
/// The `ambient_fade` setting's default, volume per second (SND-4.3).
pub const SND_AMBIENT_FADE: f32 = 100.0;
/// An ambient volume below this is 0 (SND-4.3).
pub const SND_AMBIENT_MIN_VOLUME: f32 = 8.0;
/// The most frames painted in one block (SND-5.2).
pub const SND_PAINTBUFFER_SIZE: usize = 512;
/// The `volume` setting's default (SND-5.4).
pub const SND_DEFAULT_VOLUME: f32 = 0.7;
/// The `_snd_mixahead` setting's default, seconds (SND-5.1).
pub const SND_MIXAHEAD: f32 = 0.1;
/// The oracle's output rate (SND-5.4).
pub const SND_ORACLE_SPEED: u32 = 11025;
/// The frames of id's fake DMA buffer, 32768 samples over two channels (SND-5.1).
pub const SND_BUFFER_FRAMES: usize = 16384;
/// `MAX_RAW_SAMPLES`: the frames of streamed music the mixer holds (SND-6.1).
pub const SND_MAX_RAW_SAMPLES: usize = 8192;

/// The ambient channels, by index (SND-1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ambient {
    /// `AMBIENT_WATER`, `ambience/water1.wav`.
    Water,
    /// `AMBIENT_SKY`, `ambience/wind2.wav`.
    Sky,
    /// `AMBIENT_SLIME`, with no sound.
    Slime,
    /// `AMBIENT_LAVA`, with no sound.
    Lava,
}

impl Ambient {
    /// Every ambient channel, in index order.
    pub const ALL: [Self; SND_NUM_AMBIENTS] = [Self::Water, Self::Sky, Self::Slime, Self::Lava];

    /// The channel's index.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Water => 0,
            Self::Sky => 1,
            Self::Slime => 2,
            Self::Lava => 3,
        }
    }

    /// The sound `S_Init` precaches for the channel (SND-4.3).
    #[must_use]
    pub const fn sound_name(self) -> Option<&'static str> {
        match self {
            Self::Water => Some("ambience/water1.wav"),
            Self::Sky => Some("ambience/wind2.wav"),
            Self::Slime | Self::Lava => None,
        }
    }
}

/// What `S_StartSound` is called with (SND-2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    /// The entity the sound comes from.
    pub entnum: i32,
    /// The entity channel, 0 to 7, or -1 for any (SND-2.2).
    pub entchannel: i32,
    /// The sound.
    pub sfx: SfxId,
    /// Where the sound is.
    pub origin: [f32; 3],
    /// The volume, 0 to 1.
    pub fvol: f32,
    /// The attenuation, 1 by default (SND-2.3).
    pub attenuation: f32,
}

/// A source of `rand ()` for the skip of SND-2.5.
pub trait Random {
    /// The next value, as C's `rand ()`: non-negative.
    fn rand(&mut self) -> i32;
}

/// The listener: the player's eye and its axes (SND-3, SND-4.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    /// `listener_origin`.
    pub origin: [f32; 3],
    /// `listener_forward`.
    pub forward: [f32; 3],
    /// `listener_right`.
    pub right: [f32; 3],
    /// `listener_up`.
    pub up: [f32; 3],
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            origin: [0.0; 3],
            forward: [1.0, 0.0, 0.0],
            right: [0.0, -1.0, 0.0],
            up: [0.0, 0.0, 1.0],
        }
    }
}

/// Where the listener is, for the ambient channels (SND-4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenerLeaf {
    /// No world model is loaded: the ambient channels are left as they are.
    NoWorld,
    /// The listener is outside the world: the ambient channels lose their sounds.
    Outside,
    /// The listener's leaf, with its four ambient levels in [`Ambient`] order.
    In([u8; SND_NUM_AMBIENTS]),
}

/// Where the frames after a loop restart inside a block are painted (SND-5.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoopPaint {
    /// At their place in the block, as QuakeSpasm paints them.
    #[default]
    Fixed,
    /// Over the block's first frames, as id's engine paints them; the engine differential
    /// selects this.
    Id,
}

/// One channel, `channel_t` (SND-1.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Channel {
    /// The sound it plays, none when cleared.
    pub sfx: Option<SfxId>,
    /// `leftvol`, 0 to 255 (a static channel's combined volume may pass 255, SND-4.4).
    pub leftvol: i32,
    /// `rightvol`.
    pub rightvol: i32,
    /// The `paintedtime` at which it ends.
    pub end: i64,
    /// The next sample of the sound to mix.
    pub pos: usize,
    /// The entity it was started on.
    pub entnum: i32,
    /// The entity channel it was started on.
    pub entchannel: i32,
    /// Where the sound is.
    pub origin: [f32; 3],
    /// `attenuation / SND_NOMINAL_CLIP_DIST`.
    pub dist_mult: f32,
    /// The volume before spatialization.
    pub master_vol: i32,
}

/// The settings the mixer reads, each a cvar of id's with its default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// `volume`, the master volume (SND-5.4).
    pub volume: f32,
    /// `nosound` (SND-2.1).
    pub nosound: bool,
    /// `ambient_level` (SND-4.3).
    pub ambient_level: f32,
    /// `ambient_fade` (SND-4.3).
    pub ambient_fade: f32,
    /// `_snd_mixahead` (SND-5.6).
    pub mixahead: f32,
    /// Where a loop restart's frames go (SND-5.7).
    pub loop_paint: LoopPaint,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: SND_DEFAULT_VOLUME,
            nosound: false,
            ambient_level: SND_AMBIENT_LEVEL,
            ambient_fade: SND_AMBIENT_FADE,
            mixahead: SND_MIXAHEAD,
            loop_paint: LoopPaint::Fixed,
        }
    }
}

/// The mixer: the channels, the clock and the listener (SND-1 to SND-5).
#[derive(Clone, Debug, PartialEq)]
pub struct Mixer {
    channels: Vec<Channel>,
    total_channels: usize,
    /// The output rate, `shm->speed`.
    speed: u32,
    /// `shm->channels == 2`.
    stereo: bool,
    /// The output buffer's frames, `shm->samples >> (shm->channels - 1)`.
    buffer_frames: usize,
    /// `paintedtime`, in frames.
    painted: i64,
    /// `soundtime`, in frames (SND-5.9).
    sound_time: i64,
    /// The seconds the headless clock has stepped (SND-5.9).
    elapsed: f64,
    listener: Listener,
    /// `cl.viewentity`.
    view_entity: i32,
    ambient_sfx: [Option<SfxId>; SND_NUM_AMBIENTS],
    settings: Settings,
    /// What the mixer would print to the console.
    messages: Vec<String>,
    /// `s_rawsamples`: streamed music, already scaled by its volume (SND-6.1).
    raw: Vec<(i32, i32)>,
    /// `s_rawend`: the frame after the last streamed one.
    raw_end: i64,
}

/// How a mixer is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixerSpec {
    /// The output rate in samples per second.
    pub speed: u32,
    /// Whether the output has two channels.
    pub stereo: bool,
    /// The output buffer's frames, which caps how far ahead the mixer paints (SND-5.6).
    pub buffer_frames: usize,
    /// The entity the listener is, `cl.viewentity`.
    pub view_entity: i32,
    /// The settings.
    pub settings: Settings,
}

impl Default for MixerSpec {
    /// The oracle's output: 11025 Hz, stereo, id's fake DMA buffer.
    fn default() -> Self {
        Self {
            speed: SND_ORACLE_SPEED,
            stereo: true,
            buffer_frames: SND_BUFFER_FRAMES,
            view_entity: 1,
            settings: Settings::default(),
        }
    }
}

impl Mixer {
    /// A mixer with no sound playing, `S_Init` then `S_StopAllSounds`; the ambient sounds
    /// are precached by [`Mixer::precache_ambients`].
    #[must_use]
    pub fn new(spec: MixerSpec) -> Self {
        Self {
            channels: vec![Channel::default(); SND_MAX_CHANNELS],
            total_channels: SND_FIRST_STATIC,
            speed: spec.speed,
            stereo: spec.stereo,
            buffer_frames: spec.buffer_frames,
            painted: 0,
            sound_time: 0,
            elapsed: 0.0,
            listener: Listener::default(),
            view_entity: spec.view_entity,
            ambient_sfx: [None; SND_NUM_AMBIENTS],
            settings: spec.settings,
            messages: Vec::new(),
            raw: vec![(0, 0); SND_MAX_RAW_SAMPLES],
            raw_end: 0,
        }
    }

    /// The frames of streamed music the mixer can take now, `MAX_RAW_SAMPLES - (s_rawend -
    /// paintedtime)` after `s_rawend` is raised to `paintedtime` (SND-6.2).
    pub fn raw_room(&mut self) -> usize {
        if self.raw_end < self.painted {
            self.raw_end = self.painted;
        }
        let ahead = usize::try_from(self.raw_end.saturating_sub(self.painted)).unwrap_or(0);
        SND_MAX_RAW_SAMPLES.saturating_sub(ahead)
    }

    /// `s_rawend = 0`, as `BGM_Stop` leaves it (SND-6.2).
    pub const fn stop_raw(&mut self) {
        self.raw_end = 0;
    }

    /// `S_RawSamples (samples, rate, 2, channels, data, volume)` (SND-6.1): `frames` 16-bit
    /// frames of `channels` interleaved in `data` at `rate`, stepped to the mixer's rate by
    /// `src = i * scale` and stored scaled by `(int) (256 * volume)`.
    pub fn raw_samples(
        &mut self,
        frames: usize,
        rate: u32,
        channels: u16,
        data: &[i16],
        volume: f32,
    ) {
        if self.raw_end < self.painted {
            self.raw_end = self.painted;
        }
        // `scale = (float) rate / shm->speed`, `intVolume = (int) (256 * volume)`.
        let scale = int_to_float(i32::try_from(rate).unwrap_or(i32::MAX))
            / int_to_float(i32::try_from(self.speed).unwrap_or(i32::MAX));
        let volume = float_to_int(256.0 * volume);
        let width = usize::from(channels.max(1));
        let mut i = 0_i32;
        loop {
            let src = float_to_int(int_to_float(i) * scale);
            let Ok(src) = usize::try_from(src) else {
                break;
            };
            if src >= frames {
                break;
            }
            let at = src.saturating_mul(width);
            let left = data.get(at).copied().map_or(0, i32::from);
            let right = if channels == 2 {
                data.get(at.saturating_add(1)).copied().map_or(0, i32::from)
            } else {
                left
            };
            let slot = usize::try_from(self.raw_end).unwrap_or(0) & (SND_MAX_RAW_SAMPLES - 1);
            if let Some(pair) = self.raw.get_mut(slot) {
                *pair = (left.saturating_mul(volume), right.saturating_mul(volume));
            }
            self.raw_end = self.raw_end.saturating_add(1);
            i = i.saturating_add(1);
        }
    }

    /// `S_Init`'s precaches: the water and sky ambient sounds (SND-4.3), through `table`.
    pub fn precache_ambients(&mut self, table: &mut SoundTable) {
        for ambient in Ambient::ALL {
            let Some(name) = ambient.sound_name() else {
                continue;
            };
            if let Ok(id) = table.precache(name)
                && let Some(slot) = self.ambient_sfx.get_mut(ambient.index())
            {
                *slot = Some(id);
            }
        }
    }

    /// The output rate.
    #[must_use]
    pub const fn speed(&self) -> u32 {
        self.speed
    }

    /// The channels, every one of the array (SND-1.1).
    #[must_use]
    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    /// How many channels are in use: the ambient and dynamic ones and the static ones added.
    #[must_use]
    pub const fn total_channels(&self) -> usize {
        self.total_channels
    }

    /// `paintedtime`.
    #[must_use]
    pub const fn painted(&self) -> i64 {
        self.painted
    }

    /// `soundtime`.
    #[must_use]
    pub const fn sound_time(&self) -> i64 {
        self.sound_time
    }

    /// The settings.
    #[must_use]
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings.
    pub const fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// The listener.
    #[must_use]
    pub const fn listener(&self) -> &Listener {
        &self.listener
    }

    /// What the mixer printed to the console since the last call.
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    /// `S_StopAllSounds`: no static channels, every channel cleared (SND-1.1).
    pub fn stop_all(&mut self) {
        self.total_channels = SND_FIRST_STATIC;
        for channel in &mut self.channels {
            *channel = Channel::default();
        }
    }

    /// `SND_PickChannel`: the dynamic channel a sound of `entnum` on `entchannel` plays on,
    /// which loses its sound; `None` when every one plays a sound of the view entity and this
    /// one is not the view entity's (SND-2.2).
    fn pick_channel(&mut self, entnum: i32, entchannel: i32) -> Option<usize> {
        let mut first_to_die = None;
        let mut life_left = i64::MAX;
        for index in SND_NUM_AMBIENTS..SND_FIRST_STATIC {
            let Some(channel) = self.channels.get(index) else {
                break;
            };
            if entchannel != 0
                && channel.entnum == entnum
                && (channel.entchannel == entchannel || entchannel == -1)
            {
                first_to_die = Some(index);
                break;
            }
            if channel.entnum == self.view_entity
                && entnum != self.view_entity
                && channel.sfx.is_some()
            {
                continue;
            }
            let left = channel.end.saturating_sub(self.painted);
            if left < life_left {
                life_left = left;
                first_to_die = Some(index);
            }
        }
        let index = first_to_die?;
        if let Some(channel) = self.channels.get_mut(index) {
            channel.sfx = None;
        }
        Some(index)
    }

    /// `SND_Spatialize`: the channel's left and right volumes from its origin and the
    /// listener (SND-3).
    fn spatialize(&self, channel: &mut Channel) {
        if channel.entnum == self.view_entity {
            channel.leftvol = channel.master_vol;
            channel.rightvol = channel.master_vol;
            return;
        }
        let source = [
            channel.origin[0] - self.listener.origin[0],
            channel.origin[1] - self.listener.origin[1],
            channel.origin[2] - self.listener.origin[2],
        ];
        let (source, length) = vector_normalize(source);
        let dist = length * channel.dist_mult;
        let right = self.listener.right;
        let dot = right[0] * source[0] + right[1] * source[1] + right[2] * source[2];
        let (rscale, lscale) = if self.stereo {
            // `rscale = 1.0 + dot`: a double expression stored in a float.
            (
                double_to_float(1.0 + f64::from(dot)),
                double_to_float(1.0 - f64::from(dot)),
            )
        } else {
            (1.0, 1.0)
        };
        let volume = |scale: f32| {
            // `scale = (1.0 - dist) * rscale`, a double stored in a float, then
            // `(int) (ch->master_vol * scale)`.
            let scale = double_to_float((1.0 - f64::from(dist)) * f64::from(scale));
            float_to_int(int_to_float(channel.master_vol) * scale).max(0)
        };
        channel.rightvol = volume(rscale);
        channel.leftvol = volume(lscale);
    }

    /// `S_StartSound (entnum, entchannel, sfx, origin, fvol, attenuation)` (SND-2.1 to
    /// SND-2.5): the index of the channel it plays on, `None` when nothing starts.
    ///
    /// `rand` draws the skip of SND-2.5, `rand () % (int) (0.1 * speed)`.
    pub fn start_sound(
        &mut self,
        table: &mut SoundTable,
        rand: &mut dyn Random,
        start: Start,
    ) -> Option<usize> {
        if self.settings.nosound {
            return None;
        }
        let Start {
            entnum,
            entchannel,
            sfx,
            origin,
            fvol,
            attenuation,
        } = start;
        let vol = float_to_int(fvol * 255.0);
        let index = self.pick_channel(entnum, entchannel)?;
        let mut channel = Channel {
            origin,
            dist_mult: attenuation / SND_NOMINAL_CLIP_DIST,
            master_vol: vol,
            entnum,
            entchannel,
            ..Channel::default()
        };
        self.spatialize(&mut channel);
        if channel.leftvol == 0 && channel.rightvol == 0 {
            *self.channels.get_mut(index)? = channel;
            return None;
        }
        // SND-2.4: `S_LoadSound`.
        table.load(sfx);
        let Some(data) = table.data(sfx) else {
            *self.channels.get_mut(index)? = channel;
            return None;
        };
        channel.sfx = Some(sfx);
        channel.pos = 0;
        channel.end = self
            .painted
            .saturating_add(i64::try_from(data.length()).unwrap_or(i64::MAX));
        // SND-2.5: an identical sound started this frame is offset.
        for other in SND_NUM_AMBIENTS..SND_FIRST_STATIC {
            if other == index {
                continue;
            }
            let Some(check) = self.channels.get(other) else {
                break;
            };
            if check.sfx == Some(sfx) && check.pos == 0 {
                let modulus = double_to_int(0.1 * f64::from(self.speed));
                let mut skip = i64::from(rand.rand().checked_rem(modulus).unwrap_or(0));
                if skip >= channel.end {
                    skip = channel.end.saturating_sub(1);
                }
                channel.pos = channel
                    .pos
                    .saturating_add(usize::try_from(skip).unwrap_or(0));
                channel.end = channel.end.saturating_sub(skip);
                break;
            }
        }
        *self.channels.get_mut(index)? = channel;
        Some(index)
    }

    /// `S_StopSound (entnum, entchannel)`: the first of the channels 0 to
    /// `SND_MAX_DYNAMIC_CHANNELS - 1` started on that entity and channel stops (SND-2.6).
    pub fn stop_sound(&mut self, entnum: i32, entchannel: i32) {
        for channel in self.channels.iter_mut().take(SND_MAX_DYNAMIC_CHANNELS) {
            if channel.entnum == entnum && channel.entchannel == entchannel {
                channel.end = 0;
                channel.sfx = None;
                return;
            }
        }
    }

    /// `S_LocalSound (name)`: `name` precached and started on the view entity, channel -1,
    /// at the origin, volume 1, attenuation 1 (SND-2.7).
    pub fn local_sound(
        &mut self,
        table: &mut SoundTable,
        rand: &mut dyn Random,
        name: &str,
    ) -> Option<usize> {
        if self.settings.nosound {
            return None;
        }
        let Ok(id) = table.precache(name) else {
            self.messages
                .push(format!("S_LocalSound: can't cache {name}"));
            return None;
        };
        let start = Start {
            entnum: self.view_entity,
            entchannel: -1,
            sfx: id,
            origin: [0.0; 3],
            fvol: 1.0,
            attenuation: 1.0,
        };
        self.start_sound(table, rand, start)
    }

    /// `S_StaticSound (sfx, origin, vol, attenuation)`: a looped sound on the next static
    /// channel (SND-4.1, SND-4.2); the channel's index, which is counted even when the sound
    /// has no data or does not loop.
    pub fn static_sound(
        &mut self,
        table: &mut SoundTable,
        sfx: SfxId,
        origin: [f32; 3],
        vol: f32,
        attenuation: f32,
    ) -> Option<usize> {
        if self.total_channels == SND_MAX_CHANNELS {
            self.messages
                .push("total_channels == MAX_CHANNELS".to_owned());
            return None;
        }
        let index = self.total_channels;
        self.total_channels = self.total_channels.saturating_add(1);
        table.load(sfx);
        let Some(data) = table.data(sfx) else {
            return Some(index);
        };
        if data.loopstart().is_none() {
            let name = table.name(sfx).unwrap_or_default();
            self.messages.push(format!("Sound {name} not looped"));
            return Some(index);
        }
        let mut channel = Channel {
            sfx: Some(sfx),
            origin,
            master_vol: float_to_int(vol),
            dist_mult: (attenuation / SND_STATIC_ATTENUATION_DIVISOR) / SND_NOMINAL_CLIP_DIST,
            end: self
                .painted
                .saturating_add(i64::try_from(data.length()).unwrap_or(i64::MAX)),
            ..Channel::default()
        };
        self.spatialize(&mut channel);
        *self.channels.get_mut(index)? = channel;
        Some(index)
    }

    /// `S_UpdateAmbientSounds`: the ambient channels follow the listener's leaf (SND-4.3);
    /// `frametime` is `host_frametime`.
    fn update_ambients(&mut self, leaf: ListenerLeaf, frametime: f64) {
        let levels = match leaf {
            ListenerLeaf::NoWorld => return,
            ListenerLeaf::In(levels) if self.settings.ambient_level != 0.0 => levels,
            ListenerLeaf::Outside | ListenerLeaf::In(_) => {
                for channel in self.channels.iter_mut().take(SND_NUM_AMBIENTS) {
                    channel.sfx = None;
                }
                return;
            }
        };
        for ambient in Ambient::ALL {
            let index = ambient.index();
            let Some(channel) = self.channels.get_mut(index) else {
                break;
            };
            channel.sfx = self.ambient_sfx.get(index).copied().flatten();
            let level = levels.get(index).copied().unwrap_or(0);
            let mut vol = self.settings.ambient_level * f32::from(level);
            if vol < SND_AMBIENT_MIN_VOLUME {
                vol = 0.0;
            }
            // `chan->master_vol += host_frametime * ambient_fade.value`: an int plus a
            // double, stored in the int.
            let step = frametime * f64::from(self.settings.ambient_fade);
            if int_to_float(channel.master_vol) < vol {
                channel.master_vol = double_to_int(f64::from(channel.master_vol) + step);
                if int_to_float(channel.master_vol) > vol {
                    channel.master_vol = float_to_int(vol);
                }
            } else if int_to_float(channel.master_vol) > vol {
                channel.master_vol = double_to_int(f64::from(channel.master_vol) - step);
                if int_to_float(channel.master_vol) < vol {
                    channel.master_vol = float_to_int(vol);
                }
            }
            channel.leftvol = channel.master_vol;
            channel.rightvol = channel.master_vol;
        }
    }

    /// `S_Update (origin, forward, right, up)` then `S_Update_` (SND-4.3, SND-4.4, SND-5.6):
    /// the listener moves, the ambient channels follow its leaf, every channel is spatialized,
    /// static channels of one sound are combined, and the mixer paints ahead of `soundtime`,
    /// which `frametime` seconds advance (SND-5.9), into `output`.
    pub fn update(
        &mut self,
        table: &mut SoundTable,
        listener: Listener,
        leaf: ListenerLeaf,
        frametime: f64,
        output: &mut dyn Output,
    ) {
        self.listener = listener;
        self.update_ambients(leaf, frametime);
        let mut combine: Option<usize> = None;
        for i in SND_NUM_AMBIENTS..self.total_channels {
            let Some(mut channel) = self.channels.get(i).copied() else {
                break;
            };
            if channel.sfx.is_none() {
                continue;
            }
            self.spatialize(&mut channel);
            if let Some(slot) = self.channels.get_mut(i) {
                *slot = channel;
            }
            if channel.leftvol == 0 && channel.rightvol == 0 {
                continue;
            }
            if i < SND_FIRST_STATIC {
                continue;
            }
            // SND-4.4: a static sound joins an earlier channel of the same sound.
            let same = |index: usize, channels: &[Channel]| {
                channels.get(index).is_some_and(|c| c.sfx == channel.sfx)
            };
            let target = match combine {
                Some(c) if same(c, &self.channels) => Some(c),
                _ => {
                    let found = (SND_FIRST_STATIC..i).find(|&j| same(j, &self.channels));
                    // `if (j == total_channels) combine = NULL`: the search stops at `i`, so
                    // a sound not found leaves `combine` at this channel itself.
                    combine = Some(found.unwrap_or(i));
                    found
                }
            };
            if let Some(c) = target.filter(|&c| c != i) {
                let (left, right) = (channel.leftvol, channel.rightvol);
                if let Some(into) = self.channels.get_mut(c) {
                    into.leftvol = into.leftvol.saturating_add(left);
                    into.rightvol = into.rightvol.saturating_add(right);
                }
                if let Some(slot) = self.channels.get_mut(i) {
                    slot.leftvol = 0;
                    slot.rightvol = 0;
                }
            }
        }
        // SND-5.9: the headless clock has stepped `frametime` seconds more.
        self.elapsed += frametime.max(0.0);
        let played = double_to_int(self.elapsed * f64::from(self.speed));
        self.sound_time = i64::from(played.max(0));
        self.mix(table, output);
    }

    /// `S_Update_`: paints from `paintedtime` to `soundtime + mixahead`, at most the buffer
    /// ahead (SND-5.6).
    fn mix(&mut self, table: &mut SoundTable, output: &mut dyn Output) {
        if self.painted < self.sound_time {
            self.painted = self.sound_time;
        }
        // SND-5.6: `endtime = soundtime + _snd_mixahead.value * shm->speed`, a float sum.
        let soundtime = int_to_float(i32::try_from(self.sound_time).unwrap_or(i32::MAX));
        let speed = int_to_float(i32::try_from(self.speed).unwrap_or(i32::MAX));
        let mut endtime = i64::from(float_to_int(soundtime + self.settings.mixahead * speed));
        let cap = i64::try_from(self.buffer_frames).unwrap_or(i64::MAX);
        if endtime.saturating_sub(self.sound_time) > cap {
            endtime = self.sound_time.saturating_add(cap);
        }
        self.paint_channels(table, endtime, output);
    }

    /// `S_PaintChannels (endtime)` (SND-5.2 to SND-5.4, SND-5.7, SND-5.8, SND-5.10): the
    /// frames from `paintedtime` to `endtime` are mixed and written to `output`.
    pub fn paint_channels(
        &mut self,
        table: &mut SoundTable,
        endtime: i64,
        output: &mut dyn Output,
    ) {
        // `snd_vol = volume.value * 256`, an int.
        let snd_vol = float_to_int(self.settings.volume * 256.0);
        let mut paintbuffer = vec![(0_i32, 0_i32); SND_PAINTBUFFER_SIZE];
        while self.painted < endtime {
            let block = i64::try_from(SND_PAINTBUFFER_SIZE).unwrap_or(i64::MAX);
            let end = endtime.min(self.painted.saturating_add(block));
            let frames = usize::try_from(end.saturating_sub(self.painted)).unwrap_or(0);
            for pair in paintbuffer.iter_mut().take(frames) {
                *pair = (0, 0);
            }
            for i in 0..self.total_channels {
                let Some(channel) = self.channels.get_mut(i) else {
                    break;
                };
                let Some(sfx) = channel.sfx else {
                    continue;
                };
                if channel.leftvol == 0 && channel.rightvol == 0 {
                    continue;
                }
                table.load(sfx);
                let Some(data) = table.data(sfx) else {
                    continue;
                };
                let mut ltime = self.painted;
                while ltime < end {
                    let count = channel.end.min(end).saturating_sub(ltime);
                    if count > 0 {
                        let count = usize::try_from(count).unwrap_or(0);
                        // SND-5.7: id paints from the block's start whatever `ltime` is.
                        let offset = match self.settings.loop_paint {
                            LoopPaint::Fixed => {
                                usize::try_from(ltime.saturating_sub(self.painted)).unwrap_or(0)
                            }
                            LoopPaint::Id => 0,
                        };
                        paint_channel(channel, data, &mut paintbuffer, offset, count);
                        ltime = ltime.saturating_add(i64::try_from(count).unwrap_or(0));
                    }
                    if ltime >= channel.end {
                        match data.loopstart() {
                            // SND-5.8: a loop that would never end stops instead.
                            Some(loopstart) if loopstart < data.length() => {
                                channel.pos = loopstart;
                                let left = data.length().saturating_sub(loopstart);
                                channel.end =
                                    ltime.saturating_add(i64::try_from(left).unwrap_or(0));
                            }
                            Some(_) | None => {
                                channel.sfx = None;
                                break;
                            }
                        }
                    }
                }
            }
            // SND-6.3: the streamed music, at the level QuakeSpasm plays it beside the sound
            // effects, not scaled by `volume`.
            let mut music = vec![(0_i32, 0_i32); frames];
            if self.raw_end >= self.painted {
                let stop = end.min(self.raw_end);
                let mut i = self.painted;
                while i < stop {
                    let slot = usize::try_from(i).unwrap_or(0) & (SND_MAX_RAW_SAMPLES - 1);
                    let at = usize::try_from(i.saturating_sub(self.painted)).unwrap_or(0);
                    if let (Some(&(left, right)), Some(pair)) =
                        (self.raw.get(slot), music.get_mut(at))
                    {
                        *pair = (left >> 8, right >> 8);
                    }
                    i = i.saturating_add(1);
                }
            }
            // SND-5.4: `(sum * snd_vol) >> 8`, clamped to 16 bits; SND-5.10: a mono output
            // takes the left sum alone.
            let mut written = Vec::with_capacity(frames.saturating_mul(2));
            for (&(left, right), &(music_left, music_right)) in
                paintbuffer.iter().take(frames).zip(&music)
            {
                written.push(transfer(left, snd_vol, music_left));
                if self.stereo {
                    written.push(transfer(right, snd_vol, music_right));
                }
            }
            output.write(&written);
            self.painted = end;
        }
    }
}

/// `SND_PaintChannelFrom8` and `SND_PaintChannelFrom16`: `count` samples of `channel` from
/// `channel.pos` added into `paintbuffer` from `offset` (SND-5.3).
fn paint_channel(
    channel: &mut Channel,
    data: &SoundData,
    paintbuffer: &mut [(i32, i32)],
    offset: usize,
    count: usize,
) {
    match data.width() {
        Width::Eight => {
            // The scale table: `snd_scaletable[i][j] = (signed char) j * i * 8` for the
            // volume's high five bits.
            channel.leftvol = channel.leftvol.min(255);
            channel.rightvol = channel.rightvol.min(255);
            let lscale = (channel.leftvol >> 3).wrapping_mul(8);
            let rscale = (channel.rightvol >> 3).wrapping_mul(8);
            for i in 0..count {
                let sample = data.sample_8(channel.pos.saturating_add(i)).unwrap_or(0);
                let Some(pair) = paintbuffer.get_mut(offset.saturating_add(i)) else {
                    break;
                };
                pair.0 = pair.0.wrapping_add(i32::from(sample).wrapping_mul(lscale));
                pair.1 = pair.1.wrapping_add(i32::from(sample).wrapping_mul(rscale));
            }
        }
        Width::Sixteen => {
            for i in 0..count {
                let sample = i32::from(data.sample_16(channel.pos.saturating_add(i)).unwrap_or(0));
                let Some(pair) = paintbuffer.get_mut(offset.saturating_add(i)) else {
                    break;
                };
                pair.0 = pair
                    .0
                    .wrapping_add(sample.wrapping_mul(channel.leftvol) >> 8);
                pair.1 = pair
                    .1
                    .wrapping_add(sample.wrapping_mul(channel.rightvol) >> 8);
            }
        }
    }
    channel.pos = channel.pos.saturating_add(count);
}

/// `Snd_WriteLinearBlastStereo16`'s sample: `(sum * snd_vol) >> 8`, plus the frame's
/// `music` (SND-6.3), clamped to 16 bits (SND-5.4).
fn transfer(sum: i32, snd_vol: i32, music: i32) -> i16 {
    let val = (sum.wrapping_mul(snd_vol) >> 8).saturating_add(music);
    i16::try_from(val.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).unwrap_or(0)
}
