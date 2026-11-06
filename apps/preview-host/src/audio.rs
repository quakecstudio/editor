// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's sound: the client's sound events started on `qs-sound`'s mixer, the mixer
//! updated once a host frame from the player's eye as `S_Update` is (SND-3 to SND-5), and its
//! frames played on the system's sound device through `cpal`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use qc_builtins::math::makevectors;
use qs_bsp::Bsp;
use qs_client::{Event, SoundRef, SoundStart};
use qs_sound::{
    Listener, ListenerLeaf, Mixer, MixerSpec, NullOutput, Output, Random, SfxId, SoundTable, Start,
};

/// The most seconds of mixed sound waiting for the device; past it new frames are dropped,
/// so a stall never builds up delay.
const MOST_QUEUED: f64 = 0.25;

/// Whether the sound plays on a device, the engine's `-nosound`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Device {
    /// The system's default output device.
    Default,
    /// None: the mixer runs and its frames are dropped.
    None,
    /// None: the mixer runs at the oracle's rate and its frames are kept, for
    /// `--view-script` (P3-28).
    Capture,
}

/// Where the mixed frames go.
enum Sink {
    /// The system's sound device, fed from a queue its callback empties.
    Device {
        queue: Queue,
        /// Kept so the stream plays; dropping it stops the sound.
        _stream: cpal::Stream,
    },
    /// No device: the frames are counted and dropped.
    Silent(NullOutput),
    /// No device: the frames are kept, left and right interleaved.
    Capture(Captured),
}

/// The frames a capture keeps.
#[derive(Default)]
struct Captured(Vec<i16>);

impl Output for Captured {
    fn write(&mut self, frames: &[i16]) {
        self.0.extend_from_slice(frames);
    }
}

/// The mixed frames waiting for the device, left and right interleaved, and how many samples
/// it may hold.
#[derive(Clone)]
struct Queue {
    samples: Arc<Mutex<VecDeque<i16>>>,
    most: usize,
}

impl Output for Queue {
    fn write(&mut self, frames: &[i16]) {
        let mut samples = self.samples.lock().unwrap_or_else(PoisonError::into_inner);
        let room = self.most.saturating_sub(samples.len());
        samples.extend(frames.iter().take(room));
    }
}

/// The sound of one map: the mixer, the sounds it knows and the device it plays on.
pub(crate) struct Audio {
    mixer: Mixer,
    table: SoundTable,
    /// The server's sound precache, index 1 first (CLIENT-1.1).
    precache: Vec<Option<SfxId>>,
    sink: Sink,
}

impl Audio {
    /// The sound of a map whose files `read` reads, on `output`, silent when it does not
    /// open.
    pub(crate) fn open(read: qs_sound::Reader, output: Device) -> Self {
        let opened = match output {
            Device::Default => device(),
            Device::None => Err("-nosound".to_owned()),
            Device::Capture => Ok((
                qs_sound::SND_ORACLE_SPEED,
                Sink::Capture(Captured::default()),
            )),
        };
        let (speed, sink) = match opened {
            Ok((speed, sink)) => (speed, sink),
            Err(error) => {
                tracing::warn!(%error, "no sound");
                (
                    qs_sound::SND_ORACLE_SPEED,
                    Sink::Silent(NullOutput::default()),
                )
            }
        };
        let mut table = SoundTable::new(speed, read);
        let mut mixer = Mixer::new(MixerSpec {
            speed,
            ..MixerSpec::default()
        });
        mixer.precache_ambients(&mut table);
        Self {
            mixer,
            table,
            precache: Vec::new(),
            sink,
        }
    }

    /// Starts and stops what the client's `events` ask for (CLIENT-4, SND-2, SND-4.1).
    pub(crate) fn handle(&mut self, events: &[Event], rand: &mut dyn Random) {
        for event in events {
            match event {
                Event::NewMap { sounds, .. } => {
                    self.mixer.stop_all();
                    self.precache = sounds
                        .iter()
                        .map(|name| self.table.precache(name).ok())
                        .collect();
                }
                Event::Sound(start) => self.start(start, rand),
                Event::StopSound { entity, channel } => self.mixer.stop_sound(*entity, *channel),
                Event::LocalSound(index) => {
                    if let Some(name) = self
                        .sound(*index)
                        .and_then(|id| self.table.name(id))
                        .map(str::to_owned)
                    {
                        self.mixer.local_sound(&mut self.table, rand, &name);
                    }
                }
                Event::StaticSound {
                    origin,
                    sound,
                    volume,
                    attenuation,
                } => {
                    // `S_StaticSound (cl.sound_precache[sound_num], org, vol, atten)`, the
                    // bytes as they were read (SND-4.1).
                    if let Some(id) = self.sound(*sound) {
                        self.mixer.static_sound(
                            &mut self.table,
                            id,
                            *origin,
                            int_to_f32(*volume),
                            int_to_f32(*attenuation),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// `S_Update` for a host frame of `frametime` seconds, the listener at `origin` looking
    /// along `angles` in the map `bsp` (SND-4.3, SND-4.4, SND-5.6).
    pub(crate) fn update(
        &mut self,
        bsp: &Bsp<'_>,
        origin: [f32; 3],
        angles: [f32; 3],
        frametime: f64,
    ) {
        let (forward, right, up) = makevectors(angles);
        let listener = Listener {
            origin,
            forward,
            right,
            up,
        };
        // `S_UpdateAmbientSounds`: `Mod_PointInLeaf (listener_origin, cl.worldmodel)`
        // (BSP-21.1).
        let leaf = bsp
            .point_in_leaf(origin)
            .and_then(|leaf| bsp.leafs.get(leaf))
            .map_or(ListenerLeaf::Outside, |leaf| {
                ListenerLeaf::In(leaf.ambient_level)
            });
        let output: &mut dyn Output = match &mut self.sink {
            Sink::Device { queue, .. } => queue,
            Sink::Silent(null) => null,
            Sink::Capture(captured) => captured,
        };
        self.mixer
            .update(&mut self.table, listener, leaf, frametime, output);
        for message in self.mixer.take_messages() {
            tracing::debug!(%message, "sound");
        }
    }

    /// Whether the mixed frames are kept, for `--view-script`.
    pub(crate) const fn captures(&self) -> bool {
        match self.sink {
            Sink::Capture(_) => true,
            Sink::Device { .. } | Sink::Silent(_) => false,
        }
    }

    /// The frames painted since the last call, left and right interleaved, when the sound is
    /// captured; none otherwise.
    pub(crate) fn take_captured(&mut self) -> Vec<i16> {
        match &mut self.sink {
            Sink::Capture(captured) => core::mem::take(&mut captured.0),
            Sink::Device { .. } | Sink::Silent(_) => Vec::new(),
        }
    }

    /// The server's precached sound `index`, counted from 1.
    fn sound(&self, index: usize) -> Option<SfxId> {
        self.precache.get(index.checked_sub(1)?).copied().flatten()
    }

    fn start(&mut self, start: &SoundStart, rand: &mut dyn Random) {
        let sfx = match start.sound {
            SoundRef::Index(index) => self.sound(index),
            SoundRef::Name(name) => self.table.precache(name).ok(),
        };
        let Some(sfx) = sfx else {
            return;
        };
        self.mixer.start_sound(
            &mut self.table,
            rand,
            Start {
                entnum: start.entity,
                entchannel: start.channel,
                sfx,
                origin: start.origin,
                fvol: start.volume,
                attenuation: start.attenuation,
            },
        );
    }
}

/// The default output device's rate and a stream playing a queue on it.
fn device() -> Result<(u32, Sink), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let speed = config.sample_rate();
    let channels = usize::from(config.channels());
    let most = seconds_of_samples(MOST_QUEUED, speed);
    let queue = Queue {
        samples: Arc::new(Mutex::new(VecDeque::with_capacity(most))),
        most,
    };
    let errors = |error: cpal::Error| tracing::warn!(%error, "the sound device");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let queue = queue.clone();
            device.build_output_stream(
                config.config(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    fill(&queue, data, channels, |s| f32::from(s) / 32768.0);
                },
                errors,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let queue = queue.clone();
            device.build_output_stream(
                config.config(),
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    fill(&queue, data, channels, |s| s);
                },
                errors,
                None,
            )
        }
        other => return Err(format!("the device takes {other} samples")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((
        speed,
        Sink::Device {
            queue,
            _stream: stream,
        },
    ))
}

/// Fills the device's `data`, `channels` samples a frame, from the queue's stereo frames: left
/// to the first channel, right to the second, silence to the rest and when the queue is empty.
fn fill<T: Copy + Default>(
    queue: &Queue,
    data: &mut [T],
    channels: usize,
    convert: impl Fn(i16) -> T,
) {
    let mut samples = queue.samples.lock().unwrap_or_else(PoisonError::into_inner);
    for frame in data.chunks_mut(channels.max(1)) {
        let left = samples.pop_front();
        let right = samples.pop_front();
        for (i, out) in frame.iter_mut().enumerate() {
            *out = match i {
                0 => left.map_or_else(T::default, &convert),
                1 => right.map_or_else(T::default, &convert),
                _ => T::default(),
            };
        }
        // A mono device hears the left channel.
    }
}

/// The samples of `seconds` of stereo sound at `speed` frames a second.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a quarter second of samples at an audio rate, positive and small"
)]
fn seconds_of_samples(seconds: f64, speed: u32) -> usize {
    (seconds * f64::from(speed) * 2.0) as usize
}

/// An `int` as C converts it to a `float`.
#[expect(clippy::cast_precision_loss, reason = "a byte's value, exact in f32")]
const fn int_to_f32(value: i32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
