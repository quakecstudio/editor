// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's sound: the client's sound events started on `qs-sound`'s mixer, the mixer
//! updated once a host frame from the player's eye as `S_Update` is (SND-3 to SND-5), and its
//! frames played on the system's sound device through `cpal`.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use qc_builtins::math::makevectors;
use qs_bsp::Bsp;
use qs_client::{Event, SoundRef, SoundStart};
use qs_sound::{
    CueSheet, CueTrack, Listener, ListenerLeaf, Mixer, MixerSpec, Music, MusicStream, NullOutput,
    OggStream, Output, Random, Read, SND_CD_RATE, SfxId, SoundTable, Start, track_files,
};

/// The music files' extensions this host decodes, of QuakeSpasm's `RRE_MUSIC_EXTENSIONS`
/// (RRE-3.5).
const MUSIC_CODECS: [&str; 1] = ["ogg"];

/// What the search path has for a track (RRE-3.5).
enum TrackFile {
    /// Its file, opened.
    Found(Box<dyn MusicStream>),
    /// No file: "Couldn't find a cdrip for track <n>".
    Missing,
    /// A file that does not open: "Couldn't handle music file <name>".
    Undecodable(String),
}

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
    /// The CD music and where its tracks come from (SND-6).
    music: Music,
    cd: Option<CdImage>,
    /// Reads `music/track<NN>.ogg` from the game's search path (RRE-3.5).
    music_files: qs_sound::Reader,
    /// `bgmvolume` (CON-7.4).
    music_volume: f32,
}

/// A CD image: its cue sheet and the binary file the sheet names (SND-6.4).
pub(crate) struct CdImage {
    sheet: CueSheet,
    file: PathBuf,
}

impl CdImage {
    /// The cue sheet at `cue` and its binary file beside it; `None` when it does not read.
    pub(crate) fn open(cue: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(cue).ok()?;
        let sheet = CueSheet::read(&text)?;
        let file = cue.parent().unwrap_or(Path::new(".")).join(&sheet.file);
        Some(Self { sheet, file })
    }

    /// Track `number`'s audio as a stream, if the image has it.
    fn stream(&self, number: u8) -> Option<CdTrackStream> {
        let track = self.sheet.track(number)?;
        let mut file = File::open(&self.file).ok()?;
        file.seek(SeekFrom::Start(track.start)).ok()?;
        Some(CdTrackStream {
            file,
            track,
            at: track.start,
        })
    }
}

/// One CD track read from the image: 16-bit little-endian stereo at 44100 Hz (SND-6.4).
struct CdTrackStream {
    file: File,
    track: CueTrack,
    at: u64,
}

impl MusicStream for CdTrackStream {
    fn rate(&self) -> u32 {
        SND_CD_RATE
    }

    fn channels(&self) -> u16 {
        2
    }

    fn read(&mut self, out: &mut [i16]) -> Read {
        let wanted = u64::try_from(out.len().saturating_mul(2)).unwrap_or(u64::MAX);
        let left = self
            .track
            .end
            .map_or(wanted, |end| end.saturating_sub(self.at));
        let bytes = usize::try_from(wanted.min(left)).unwrap_or(0) & !3;
        if bytes == 0 {
            return Read::End;
        }
        // Whole stereo frames only: a short read keeps the frames it got and goes back over
        // the part of one it did not finish, so the left and right samples stay in step.
        let mut raw = Vec::with_capacity(bytes);
        let wanted = u64::try_from(bytes).unwrap_or(0);
        let got = match (&mut self.file).take(wanted).read_to_end(&mut raw) {
            Ok(0) => return Read::End,
            Ok(got) => got,
            Err(_) => return Read::Failed(-1),
        };
        let read = got & !3;
        if read == 0 {
            return Read::End;
        }
        let over = i64::try_from(got.saturating_sub(read)).unwrap_or(0);
        if over > 0
            && self
                .file
                .seek(SeekFrom::Current(over.saturating_neg()))
                .is_err()
        {
            return Read::Failed(-1);
        }
        let (pairs, _) = raw.get(..read).unwrap_or_default().as_chunks::<2>();
        for (sample, pair) in out.iter_mut().zip(pairs) {
            *sample = i16::from_le_bytes(*pair);
        }
        self.at = self.at.saturating_add(u64::try_from(read).unwrap_or(0));
        Read::Samples(read >> 1)
    }

    fn rewind(&mut self) -> bool {
        self.at = self.track.start;
        self.file.seek(SeekFrom::Start(self.track.start)).is_ok()
    }
}

impl Audio {
    /// The sound of a map whose files `read` reads, on `output`, silent when it does not
    /// open, with its music from the CD image `cd` or the files `music_files` reads.
    pub(crate) fn open(
        read: qs_sound::Reader,
        output: Device,
        cd: Option<CdImage>,
        music_files: qs_sound::Reader,
    ) -> Self {
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
            music: Music::new(),
            cd,
            music_files,
            music_volume: 1.0,
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
                Event::StopSound { entity, channel } => {
                    self.mixer.stop_sound(entity.get(), *channel)
                }
                // `BGM_PlayCDtrack ((byte)cl.cdtrack, true)` (RRE-3.5).
                Event::CdTrack { track, .. } => self.play_track(*track),
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
        // `BGM_Update` before `S_Update` (SND-6.2).
        for message in self.music.update(&mut self.mixer, self.music_volume) {
            tracing::info!(%message, "music");
        }
        self.mixer
            .update(&mut self.table, listener, leaf, frametime, output);
        for message in self.mixer.take_messages() {
            tracing::debug!(%message, "sound");
        }
    }

    /// `BGM_PlayCDtrack (track, true)` (RRE-3.5): the CD image's track, looping, else the file
    /// `music/track<NN>.ogg` from the search path, else nothing, the music before it stopped.
    fn play_track(&mut self, track: i32) {
        self.music.stop(&mut self.mixer);
        // `(byte)cl.cdtrack`.
        let number = track.to_le_bytes()[0];
        if let Some(stream) = self.cd.as_ref().and_then(|cd| cd.stream(number)) {
            tracing::info!(track = number, "music: the CD image's track");
            self.music.play(&mut self.mixer, Box::new(stream), true);
            return;
        }
        match self.track_file(number) {
            TrackFile::Found(stream) => self.music.play(&mut self.mixer, stream, true),
            TrackFile::Missing => {
                tracing::info!(track = number, "Couldn't find a cdrip for track {number}");
            }
            TrackFile::Undecodable(name) => tracing::info!("Couldn't handle music file {name}"),
        }
    }

    /// The file of track `number`: `music/track<NN>.<ext>` for the first extension this host
    /// decodes that the search path has, opened (RRE-3.5).
    fn track_file(&mut self, number: u8) -> TrackFile {
        for name in track_files(number, &MUSIC_CODECS) {
            let Some(bytes) = (self.music_files)(&name) else {
                continue;
            };
            return match OggStream::new(bytes) {
                Ok(stream) => {
                    tracing::info!(track = number, %name, "music: the track's file");
                    TrackFile::Found(Box::new(stream))
                }
                Err(error) => {
                    tracing::debug!(%name, %error, "music");
                    TrackFile::Undecodable(name)
                }
            };
        }
        TrackFile::Missing
    }

    /// The `bgmvolume` setting (SND-6.2, CON-7.4).
    pub(crate) const fn set_music_volume(&mut self, volume: f32) {
        self.music_volume = volume;
    }

    /// The `volume` setting, the master volume (SND-5.4, CON-7.4).
    pub(crate) const fn set_volume(&mut self, volume: f32) {
        let mut settings = *self.mixer.settings();
        settings.volume = volume;
        self.mixer.set_settings(settings);
    }

    /// `S_LocalSound` of the sound named `name`, as the menu plays its clicks (MENU-1.3).
    pub(crate) fn local(&mut self, name: &str, rand: &mut dyn Random) {
        self.mixer.local_sound(&mut self.table, rand, name);
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
                entnum: start.entity.get(),
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
