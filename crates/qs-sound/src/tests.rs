// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; the numbers are the spec's"
)]

use std::collections::HashMap;

use crate::{
    Ambient, Listener, Loaded, Mixer, MixerSpec, Random, SND_FIRST_STATIC, SND_MAX_CHANNELS,
    SND_MAX_DYNAMIC_CHANNELS, SND_MAX_NAME, SND_MAX_SFX, SND_NUM_AMBIENTS, SfxId, SoundData,
    SoundError, SoundTable, Start, Width, track_files,
};

/// A RIFF WAVE file of one channel: PCM at `rate`, `bits` per sample, the raw sample bytes
/// `data`, and a `cue ` chunk at `loopstart` when given.
fn wav(rate: i32, bits: i16, channels: i16, data: &[u8], loopstart: Option<i32>) -> Vec<u8> {
    let mut chunks = Vec::new();
    let block = i16::try_from((i32::from(channels) * i32::from(bits)) >> 3).unwrap();
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1_i16.to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&(rate * i32::from(block)).to_le_bytes());
    fmt.extend_from_slice(&block.to_le_bytes());
    fmt.extend_from_slice(&bits.to_le_bytes());
    chunks.push((*b"fmt ", fmt));
    if let Some(offset) = loopstart {
        let mut cue = Vec::new();
        cue.extend_from_slice(&1_i32.to_le_bytes());
        cue.extend_from_slice(&1_i32.to_le_bytes());
        cue.extend_from_slice(&offset.to_le_bytes());
        cue.extend_from_slice(b"data");
        cue.extend_from_slice(&0_i32.to_le_bytes());
        cue.extend_from_slice(&0_i32.to_le_bytes());
        cue.extend_from_slice(&offset.to_le_bytes());
        chunks.push((*b"cue ", cue));
    }
    chunks.push((*b"data", data.to_vec()));
    let mut body = b"WAVE".to_vec();
    for (id, data) in chunks {
        body.extend_from_slice(&id);
        body.extend_from_slice(&i32::try_from(data.len()).unwrap().to_le_bytes());
        body.extend_from_slice(&data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&i32::try_from(body.len()).unwrap().to_le_bytes());
    file.extend_from_slice(&body);
    file
}

/// Eight 16-bit samples: 1000, -1000, 2000, -2000, ..., as little-endian bytes.
fn sixteen_bit() -> Vec<u8> {
    [1000_i16, -1000, 2000, -2000, 3000, -3000, 4000, -4000]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect()
}

/// A game filesystem of the WAV files given by path.
struct Files(HashMap<String, Vec<u8>>);

impl Files {
    fn reader(&self) -> impl FnMut(&str) -> Option<Vec<u8>> + '_ {
        move |path| self.0.get(path).cloned()
    }
}

fn files(entries: &[(&str, Vec<u8>)]) -> Files {
    Files(
        entries
            .iter()
            .map(|(name, bytes)| (format!("sound/{name}"), bytes.clone()))
            .collect(),
    )
}

/// `rand ()` returning the values given, then 0.
struct Fixed(Vec<i32>);

impl Random for Fixed {
    fn rand(&mut self) -> i32 {
        if self.0.is_empty() {
            0
        } else {
            self.0.remove(0)
        }
    }
}

fn start(entnum: i32, entchannel: i32, sfx: SfxId, origin: [f32; 3]) -> Start {
    Start {
        entnum,
        entchannel,
        sfx,
        origin,
        fvol: 1.0,
        attenuation: 1.0,
    }
}

/// A mixer at the oracle's rate, a table with `beep` (16-bit, 8 samples, no loop), `loop`
/// (8-bit, 8 samples, looping from 4) and `missing` (no file).
fn fixture() -> (Mixer, SoundTable, Files, SfxId, SfxId, SfxId) {
    let files = files(&[
        ("beep.wav", wav(11025, 16, 1, &sixteen_bit(), None)),
        (
            "loop.wav",
            wav(
                11025,
                8,
                1,
                &[200, 128, 100, 128, 150, 128, 50, 128],
                Some(4),
            ),
        ),
    ]);
    let mut table = SoundTable::new();
    let mut read = files.reader();
    let beep = table.precache("beep.wav", 11025, &mut read).unwrap();
    let looped = table.precache("loop.wav", 11025, &mut read).unwrap();
    let missing = table.precache("missing.wav", 11025, &mut read).unwrap();
    drop(read);
    (
        Mixer::new(MixerSpec::default()),
        table,
        files,
        beep,
        looped,
        missing,
    )
}

// spec SND-1.1: 128 channels, the four ambient then eight dynamic ones first, static ones
// from 12 up to total_channels; stop_all forgets the static ones and clears every channel
#[test]
fn the_channel_array_and_stop_all() {
    let (mut mixer, table, _, beep, looped, _) = fixture();
    assert_eq!(mixer.channels().len(), SND_MAX_CHANNELS);
    assert_eq!(
        SND_NUM_AMBIENTS + SND_MAX_DYNAMIC_CHANNELS,
        SND_FIRST_STATIC
    );
    assert_eq!(mixer.total_channels(), SND_FIRST_STATIC);
    assert_eq!(Ambient::Lava.index(), 3);
    mixer.static_sound(&table, looped, [0.0; 3], 255.0, 1.0);
    mixer.start_sound(&table, &mut Fixed(vec![]), start(1, 1, beep, [0.0; 3]));
    assert_eq!(mixer.total_channels(), SND_FIRST_STATIC + 1);
    assert!(mixer.channels()[4].sfx.is_some());
    mixer.stop_all();
    assert_eq!(mixer.total_channels(), SND_FIRST_STATIC);
    assert!(
        mixer
            .channels()
            .iter()
            .all(|c| c.sfx.is_none() && c.end == 0)
    );
}

// spec SND-1.2: a started channel holds the sound, its volumes, end, pos, entity and channel,
// origin, dist_mult and master volume
#[test]
fn a_started_channel_s_fields() {
    let (mut mixer, table, _, beep, _, _) = fixture();
    let index = mixer
        .start_sound(
            &table,
            &mut Fixed(vec![]),
            start(1, 2, beep, [1.0, 2.0, 3.0]),
        )
        .unwrap();
    let channel = mixer.channels()[index];
    assert_eq!(channel.sfx, Some(beep));
    assert_eq!((channel.leftvol, channel.rightvol), (255, 255));
    assert_eq!(channel.end, 8);
    assert_eq!(channel.pos, 0);
    assert_eq!((channel.entnum, channel.entchannel), (1, 2));
    assert_eq!(channel.origin, [1.0, 2.0, 3.0]);
    assert_eq!(channel.dist_mult, 1.0 / 1000.0);
    assert_eq!(channel.master_vol, 255);
}

// spec SND-1.3: a name is known once, at most 63 bytes, in a table of at most 512
#[test]
fn names_are_known_once_with_their_limits() {
    let mut table = SoundTable::new();
    let a = table.find("a.wav").unwrap();
    assert_eq!(table.find("a.wav").unwrap(), a);
    assert_eq!(table.len(), 1);
    let long = "x".repeat(SND_MAX_NAME + 1);
    assert_eq!(
        table.find(&long),
        Err(SoundError::NameTooLong(long.clone()))
    );
    assert!(table.find(&"y".repeat(SND_MAX_NAME)).is_ok());
    for i in 2..SND_MAX_SFX {
        table.find(&format!("{i}.wav")).unwrap();
    }
    assert_eq!(table.len(), SND_MAX_SFX);
    assert_eq!(table.find("one-more.wav"), Err(SoundError::OutOfSfx));
    assert!(table.find("a.wav").is_ok(), "a known name is still found");
}

// spec SND-1.4: a sound is loaded from sound/<name>; a missing file and a stereo one leave it
// without data, and the sound stays known
#[test]
fn missing_and_stereo_files_leave_a_sound_without_data() {
    let files = files(&[("stereo.wav", wav(11025, 8, 2, &[128; 8], None))]);
    let mut table = SoundTable::new();
    let mut read = files.reader();
    let missing = table.precache("missing.wav", 11025, &mut read).unwrap();
    let stereo = table.precache("stereo.wav", 11025, &mut read).unwrap();
    assert_eq!(table.loaded(missing), Some(&Loaded::Missing));
    assert_eq!(table.loaded(stereo), Some(&Loaded::Stereo));
    assert!(table.data(missing).is_none() && table.data(stereo).is_none());
    assert_eq!(table.name(missing), Some("missing.wav"));
    let bad = table.precache("bad.wav", 11025, &mut |_| Some(b"not a wav".to_vec()));
    assert!(matches!(
        table.loaded(bad.unwrap()),
        Some(Loaded::Refused(_))
    ));
}

// spec SND-1.5, SND-1.6: the samples are resampled by stepscale = rate / speed: the length
// and the loop start truncated after the division, each output sample read at
// (i * fracstep) >> 8, an 8-bit file kept as byte - 128 and a 16-bit one as its values
#[test]
fn resampling_decimates_and_repeats() {
    let bytes = wav(
        22050,
        8,
        1,
        &[128, 138, 148, 158, 168, 178, 188, 198],
        Some(5),
    );
    let sound = qs_wav::s_load_sound(&bytes).unwrap();
    let data = SoundData::resample(&sound, 11025);
    assert_eq!(data.length(), 4);
    assert_eq!(data.loopstart(), Some(2));
    assert_eq!(data.width(), Width::Eight);
    assert_eq!(
        (0..5)
            .map(|i| data.sample_8(i).unwrap())
            .collect::<Vec<_>>(),
        [0, 20, 40, 60, 0]
    );
    let slow = wav(5512, 8, 1, &[128, 138, 148], None);
    let data = SoundData::resample(&qs_wav::s_load_sound(&slow).unwrap(), 11025);
    // stepscale 0.49995: 3 / 0.49995 is 6.0006, fracstep 127, so samples 0,0,0,1,1,2.
    assert_eq!(data.length(), 6);
    assert_eq!(data.loopstart(), None);
    assert_eq!(
        (0..6)
            .map(|i| data.sample_8(i).unwrap())
            .collect::<Vec<_>>(),
        [0, 0, 0, 10, 10, 20]
    );
    let same = wav(11025, 16, 1, &sixteen_bit(), None);
    let data = SoundData::resample(&qs_wav::s_load_sound(&same).unwrap(), 11025);
    assert_eq!(data.width(), Width::Sixteen);
    assert_eq!(data.sample_16(1), Some(-1000));
    assert_eq!(data.sample_8(1), None);
    assert_eq!(data.sample_16(8), Some(0), "past the end is silence");
}

// spec SND-2.1: nothing starts under nosound, and the master volume is fvol * 255 truncated
#[test]
fn nosound_and_the_master_volume() {
    let (mut mixer, table, _, beep, _, _) = fixture();
    let mut settings = *mixer.settings();
    settings.nosound = true;
    mixer.set_settings(settings);
    assert_eq!(
        mixer.start_sound(&table, &mut Fixed(vec![]), start(1, 1, beep, [0.0; 3])),
        None
    );
    settings.nosound = false;
    mixer.set_settings(settings);
    let half = Start {
        fvol: 0.5,
        ..start(1, 1, beep, [0.0; 3])
    };
    let index = mixer.start_sound(&table, &mut Fixed(vec![]), half).unwrap();
    assert_eq!(mixer.channels()[index].master_vol, 127);
}

// spec SND-2.2: a sound of the same entity and channel (or any channel for -1) takes that
// channel; channel 0 never overrides; the view entity's sounds are kept from other
// entities; else the channel with the least life left; every channel protected gives none
#[test]
fn picking_a_channel() {
    let (mut mixer, table, _, beep, looped, _) = fixture();
    let mut rand = Fixed(vec![]);
    let first = mixer
        .start_sound(&table, &mut rand, start(2, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(first, SND_NUM_AMBIENTS, "the first dynamic channel");
    let second = mixer
        .start_sound(&table, &mut rand, start(3, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(second, 5, "the first channel with no life left");
    let again = mixer
        .start_sound(&table, &mut rand, start(3, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(again, 5, "the same entity and channel override");
    let any = mixer
        .start_sound(&table, &mut rand, start(2, -1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(any, 4, "channel -1 takes any channel of the entity");
    let zero = mixer
        .start_sound(&table, &mut rand, start(2, 0, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(zero, 6, "channel 0 never overrides");
    // The view entity fills every dynamic channel, a frame apart so each start finds a fresh
    // channel with the least life left; a monster sound then has nowhere to go.
    mixer.stop_all();
    for channel in 1..=8 {
        mixer
            .start_sound(&table, &mut rand, start(1, channel, looped, [0.0; 3]))
            .unwrap();
        mixer.paint_channels(&table, mixer.painted() + 1);
    }
    let ents: Vec<_> = mixer.channels()[SND_NUM_AMBIENTS..SND_FIRST_STATIC]
        .iter()
        .map(|c| (c.entnum, c.entchannel))
        .collect();
    assert!(ents.iter().all(|c| c.0 == 1), "{ents:?}");
    assert_eq!(
        mixer.start_sound(&table, &mut rand, start(2, 1, beep, [0.0; 3])),
        None
    );
    assert!(
        matches!(
            mixer.start_sound(&table, &mut rand, start(1, 9, beep, [0.0; 3])),
            Some(4..=11)
        ),
        "the view entity's own sound takes the channel with the least life left"
    );
}

// spec SND-2.3, SND-2.4: an inaudible sound and one with no data take the channel without
// playing; a playing one starts at 0 and ends at paintedtime + length
#[test]
fn silent_and_dataless_sounds_take_the_channel() {
    let (mut mixer, table, _, beep, _, missing) = fixture();
    let mut rand = Fixed(vec![]);
    let far = start(2, 1, beep, [5000.0, 0.0, 0.0]);
    assert_eq!(mixer.start_sound(&table, &mut rand, far), None);
    let taken = mixer.channels()[4];
    assert_eq!(taken.sfx, None);
    assert_eq!((taken.entnum, taken.entchannel), (2, 1));
    assert_eq!(
        mixer.start_sound(&table, &mut rand, start(3, 1, missing, [0.0; 3])),
        None
    );
    assert_eq!(mixer.channels()[4].sfx, None);
    assert_eq!(
        mixer.channels()[4].entnum,
        3,
        "the same channel, with no life left"
    );
    mixer.paint_channels(&table, 100);
    let index = mixer
        .start_sound(&table, &mut rand, start(4, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(mixer.channels()[index].end, 108);
}

// spec SND-2.5: an identical sound still at its start skips rand () % (0.1 * speed)
// samples, at most end - 1
#[test]
fn an_identical_sound_this_frame_is_offset() {
    let (mut mixer, table, _, beep, _, _) = fixture();
    mixer
        .start_sound(&table, &mut Fixed(vec![]), start(2, 1, beep, [0.0; 3]))
        .unwrap();
    let second = mixer
        .start_sound(
            &table,
            &mut Fixed(vec![1102 + 3]),
            start(3, 1, beep, [0.0; 3]),
        )
        .unwrap();
    assert_eq!(mixer.channels()[second].pos, 3);
    assert_eq!(mixer.channels()[second].end, 5);
    let third = mixer
        .start_sound(&table, &mut Fixed(vec![500]), start(4, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(
        mixer.channels()[third].pos,
        7,
        "the skip is capped at end - 1"
    );
    assert_eq!(mixer.channels()[third].end, 1);
}

// spec SND-2.6: stop_sound scans channels 0 to 7 only, so a sound on a later dynamic channel
// plays on
#[test]
fn stopping_scans_the_first_eight_channels() {
    let (mut mixer, table, _, beep, _, _) = fixture();
    let mut rand = Fixed(vec![]);
    for entity in 2..=9 {
        mixer
            .start_sound(&table, &mut rand, start(entity, 1, beep, [0.0; 3]))
            .unwrap();
    }
    assert_eq!(mixer.channels()[11].entnum, 9);
    mixer.stop_sound(9, 1);
    assert!(
        mixer.channels()[11].sfx.is_some(),
        "channel 11 is past the scan"
    );
    mixer.stop_sound(5, 1);
    assert_eq!(mixer.channels()[7].sfx, None);
    assert_eq!(mixer.channels()[7].end, 0);
}

// spec SND-2.7: a local sound plays on the view entity, channel -1, at full volume
#[test]
fn a_local_sound_is_the_view_entity_s() {
    let (mut mixer, mut table, files, _, _, _) = fixture();
    let mut read = files.reader();
    let index = mixer
        .local_sound(&mut table, &mut read, &mut Fixed(vec![]), "beep.wav")
        .unwrap();
    let channel = mixer.channels()[index];
    assert_eq!((channel.entnum, channel.entchannel), (1, -1));
    assert_eq!((channel.leftvol, channel.rightvol), (255, 255));
    let long = "x".repeat(SND_MAX_NAME + 1);
    assert_eq!(
        mixer.local_sound(&mut table, &mut read, &mut Fixed(vec![]), &long),
        None
    );
    assert!(mixer.take_messages()[0].starts_with("S_LocalSound: can't cache"));
}

// spec SND-3.1, SND-3.2: the view entity's sound is at full volume; another's is scaled by
// 1 - dist and panned by the listener's right vector, silent at or past the clip distance,
// and unpanned on a mono output
#[test]
fn spatialization() {
    let (mut mixer, table, _, beep, _, _) = fixture();
    let mut rand = Fixed(vec![]);
    let listener = Listener {
        origin: [100.0, 100.0, 0.0],
        ..Listener::default()
    };
    mixer.update(&table, listener, None, 0.0);
    let own = mixer
        .start_sound(&table, &mut rand, start(1, 1, beep, [9000.0, 0.0, 0.0]))
        .unwrap();
    assert_eq!(
        (
            mixer.channels()[own].leftvol,
            mixer.channels()[own].rightvol
        ),
        (255, 255)
    );
    // 500 units to the listener's right (-y): dist 0.5, dot 1.
    let right = mixer
        .start_sound(&table, &mut rand, start(2, 1, beep, [100.0, -400.0, 0.0]))
        .unwrap();
    assert_eq!(
        (
            mixer.channels()[right].leftvol,
            mixer.channels()[right].rightvol
        ),
        (0, 255)
    );
    let ahead = mixer
        .start_sound(&table, &mut rand, start(3, 1, beep, [600.0, 100.0, 0.0]))
        .unwrap();
    assert_eq!(
        (
            mixer.channels()[ahead].leftvol,
            mixer.channels()[ahead].rightvol
        ),
        (127, 127)
    );
    assert_eq!(
        mixer.start_sound(&table, &mut rand, start(4, 1, beep, [1100.0, 100.0, 0.0])),
        None,
        "at the clip distance"
    );
    let mut mono = Mixer::new(MixerSpec {
        stereo: false,
        ..MixerSpec::default()
    });
    mono.update(&table, listener, None, 0.0);
    let index = mono
        .start_sound(&table, &mut rand, start(2, 1, beep, [100.0, -400.0, 0.0]))
        .unwrap();
    assert_eq!(
        (
            mono.channels()[index].leftvol,
            mono.channels()[index].rightvol
        ),
        (127, 127)
    );
}

// spec SND-4.1, SND-4.2: a static sound takes the next channel, counted even when it has no
// data or does not loop; a looped one plays at attenuation / 64 over the clip distance
#[test]
fn static_sounds() {
    let (mut mixer, table, _, beep, looped, missing) = fixture();
    assert_eq!(
        mixer.static_sound(&table, beep, [0.0; 3], 255.0, 1.0),
        Some(SND_FIRST_STATIC)
    );
    assert_eq!(mixer.take_messages(), ["Sound beep.wav not looped"]);
    assert_eq!(
        mixer.static_sound(&table, missing, [0.0; 3], 255.0, 1.0),
        Some(SND_FIRST_STATIC + 1)
    );
    let index = mixer
        .static_sound(&table, looped, [10.0, 0.0, 0.0], 100.5, 2.0)
        .unwrap();
    assert_eq!(index, SND_FIRST_STATIC + 2);
    assert_eq!(mixer.total_channels(), SND_FIRST_STATIC + 3);
    let channel = mixer.channels()[index];
    assert_eq!(channel.sfx, Some(looped));
    assert_eq!(channel.master_vol, 100);
    assert_eq!(channel.dist_mult, (2.0 / 64.0) / 1000.0);
    assert_eq!(channel.end, 8);
    while mixer.total_channels() < SND_MAX_CHANNELS {
        mixer.static_sound(&table, looped, [0.0; 3], 1.0, 1.0);
    }
    assert_eq!(mixer.static_sound(&table, looped, [0.0; 3], 1.0, 1.0), None);
    assert!(
        mixer
            .take_messages()
            .contains(&"total_channels == MAX_CHANNELS".to_owned())
    );
}

// spec SND-4.3: the ambient channels follow the leaf's levels, scaled by ambient_level, 0
// below 8, moving by frametime * ambient_fade truncated per update; no leaf silences them
#[test]
fn ambient_sounds_follow_the_leaf() {
    let (mut mixer, mut table, files, _, _, _) = fixture();
    let water = wav(11025, 8, 1, &[128; 8], Some(0));
    let wind = wav(11025, 8, 1, &[128; 8], Some(0));
    let files = Files(
        files
            .0
            .into_iter()
            .chain([
                ("sound/ambience/water1.wav".to_owned(), water),
                ("sound/ambience/wind2.wav".to_owned(), wind),
            ])
            .collect(),
    );
    mixer.precache_ambients(&mut table, &mut files.reader());
    let listener = Listener::default();
    mixer.update(&table, listener, Some([255, 20, 255, 0]), 0.1);
    let channels = mixer.channels();
    assert_eq!(channels[0].master_vol, 10, "toward 76.5 by 0.1 * 100");
    assert_eq!((channels[0].leftvol, channels[0].rightvol), (10, 10));
    assert_eq!(channels[1].master_vol, 0, "0.3 * 20 is below 8");
    assert_eq!(channels[2].sfx, None, "slime has no sound");
    for _ in 0..7 {
        mixer.update(&table, listener, Some([255, 20, 255, 0]), 0.1);
    }
    assert_eq!(mixer.channels()[0].master_vol, 76, "never past the target");
    mixer.update(&table, listener, Some([0, 0, 0, 0]), 0.005);
    assert_eq!(
        mixer.channels()[0].master_vol,
        75,
        "falling, a half step truncates toward zero to a whole one"
    );
    mixer.update(&table, listener, Some([0, 0, 0, 0]), 0.1);
    assert_eq!(mixer.channels()[0].master_vol, 65);
    mixer.update(&table, listener, Some([255, 0, 0, 0]), 0.005);
    assert_eq!(
        mixer.channels()[0].master_vol,
        65,
        "rising, a step under 1 / ambient_fade truncates to 0"
    );
    mixer.update(&table, listener, None, 0.1);
    assert!(
        mixer.channels()[..SND_NUM_AMBIENTS]
            .iter()
            .all(|c| c.sfx.is_none())
    );
}

// spec SND-4.4: static channels of one sound are combined into the first: its volumes grow
// and theirs become 0; a different sound keeps its own
#[test]
fn static_sounds_of_one_sound_combine() {
    let (mut mixer, table, _, _, looped, _) = fixture();
    let a = mixer
        .static_sound(&table, looped, [100.0, 0.0, 0.0], 50.0, 1.0)
        .unwrap();
    let b = mixer
        .static_sound(&table, looped, [100.0, 0.0, 0.0], 50.0, 1.0)
        .unwrap();
    let c = mixer
        .static_sound(&table, looped, [100.0, 0.0, 0.0], 50.0, 1.0)
        .unwrap();
    mixer.update(&table, Listener::default(), None, 0.0);
    let channels = mixer.channels();
    // Each is (int) (50 * (1 - 100 / 64000)), 49; the first holds all three.
    assert_eq!((channels[a].leftvol, channels[a].rightvol), (147, 147));
    assert_eq!((channels[b].leftvol, channels[b].rightvol), (0, 0));
    assert_eq!((channels[c].leftvol, channels[c].rightvol), (0, 0));
}

// spec SND-5.1, SND-5.5: each update plays frametime * speed frames and paints mixahead
// ahead of them, at most the buffer's frames
#[test]
fn the_update_paints_ahead_of_the_clock() {
    let (mut mixer, table, _, _, _, _) = fixture();
    mixer.update(&table, Listener::default(), None, 0.1);
    assert_eq!(mixer.sound_time(), 1102);
    assert_eq!(mixer.painted(), 1102 + 1102);
    assert_eq!(
        mixer.take_output().len(),
        2 * 1102,
        "1102 frames, left and right"
    );
    mixer.update(&table, Listener::default(), None, 0.05);
    assert_eq!(mixer.sound_time(), 1102 + 551);
    assert_eq!(mixer.painted(), 1102 + 551 + 1102);
    let mut small = Mixer::new(MixerSpec {
        buffer_frames: 100,
        ..MixerSpec::default()
    });
    small.update(&table, Listener::default(), None, 0.0);
    assert_eq!(small.painted(), 100, "capped at the buffer");
}

// spec SND-5.2, SND-5.3, SND-5.4: a 16-bit sample adds (sample * vol) >> 8 and an 8-bit
// one sample * (vol >> 3) * 8; the sum times the master volume over 256 is clamped to 16
// bits; a sound that does not loop stops at its end and one that loops restarts at loopstart
#[test]
fn painting_mixes_and_loops() {
    let (mut mixer, table, _, beep, looped, _) = fixture();
    let mut rand = Fixed(vec![]);
    mixer
        .start_sound(&table, &mut rand, start(1, 1, beep, [0.0; 3]))
        .unwrap();
    mixer.paint_channels(&table, 10);
    let out = mixer.take_output();
    assert_eq!(out.len(), 20);
    // (1000 * 255) >> 8 is 996; (996 * 179) >> 8 is 696.
    // (-1000 * 255) >> 8 is -997 (an arithmetic shift rounds down); (-997 * 179) >> 8 is
    // -698.
    assert_eq!(&out[..4], [696, 696, -698, -698]);
    assert_eq!(&out[16..], [0, 0, 0, 0], "silence after the sound ends");
    assert_eq!(mixer.channels()[4].sfx, None, "the channel stopped");
    let index = mixer
        .static_sound(&table, looped, [0.0; 3], 255.0, 1.0)
        .unwrap();
    mixer.paint_channels(&table, 22);
    let out = mixer.take_output();
    assert_eq!(out.len(), 24);
    // 72 * 248 is 17856; (17856 * 179) >> 8 is 12485.
    assert_eq!(out[0], 12485);
    // After 8 samples the loop restarts at 4: samples 4..8 again (150 - 128 = 22).
    // 22 * 248 is 5456; (5456 * 179) >> 8 is 3814.
    assert_eq!(out[16], 3814);
    assert_eq!(mixer.channels()[index].sfx, Some(looped));
    // Frames 18 to 22 play samples 4 to 8, and at frame 22, the end again, the loop restarts.
    assert_eq!(mixer.channels()[index].pos, 4);
    assert_eq!(mixer.channels()[index].end, 26);
    let loud = Start {
        sfx: beep,
        ..start(1, 2, beep, [0.0; 3])
    };
    let mut settings = *mixer.settings();
    settings.volume = 100.0;
    mixer.set_settings(settings);
    mixer.start_sound(&table, &mut rand, loud);
    mixer.paint_channels(&table, 24);
    let out = mixer.take_output();
    assert_eq!(out[0], i16::MAX, "clamped");
}

// spec RRE-3.1: a track's files are music/track<NN>.<ext> for each extension, in order
#[test]
fn track_files_name_the_candidates() {
    assert_eq!(
        track_files(2, &["ogg", "mp3"]),
        ["music/track02.ogg", "music/track02.mp3"]
    );
    assert_eq!(track_files(11, &["wav"]), ["music/track11.wav"]);
}

// spec SND-1: the water and sky channels have their sounds precached, slime and lava none
#[test]
fn the_ambient_sounds_are_precached() {
    assert_eq!(Ambient::Water.sound_name(), Some("ambience/water1.wav"));
    assert_eq!(Ambient::Sky.sound_name(), Some("ambience/wind2.wav"));
    assert_eq!(Ambient::Slime.sound_name(), None);
    assert_eq!(Ambient::Lava.sound_name(), None);
    let (mut mixer, mut table, _, _, _, _) = fixture();
    let files = files(&[
        ("ambience/water1.wav", wav(11025, 8, 1, &[128; 8], Some(0))),
        ("ambience/wind2.wav", wav(11025, 8, 1, &[128; 8], Some(0))),
    ]);
    mixer.precache_ambients(&mut table, &mut files.reader());
    let water = table.find("ambience/water1.wav").unwrap();
    let wind = table.find("ambience/wind2.wav").unwrap();
    assert_eq!(table.loaded(water), Some(&Loaded::Data));
    mixer.update(&table, Listener::default(), Some([255; 4]), 0.1);
    assert_eq!(mixer.channels()[0].sfx, Some(water));
    assert_eq!(mixer.channels()[1].sfx, Some(wind));
    assert_eq!(mixer.channels()[2].sfx, None);
    assert_eq!(mixer.channels()[3].sfx, None);
    assert_eq!(mixer.speed(), 11025);
    assert_eq!(water.index(), 3, "after beep, loop and missing");
}

// spec SND-3.2: the pan follows the dot product of the normalised offset with the listener's
// right vector, the distance its length times dist_mult, from the listener's own origin
#[test]
fn spatialization_at_an_angle() {
    let (mut mixer, table, _, beep, _, _) = fixture();
    let mut rand = Fixed(vec![]);
    let listener = Listener {
        origin: [100.0, 100.0, 0.0],
        ..Listener::default()
    };
    mixer.update(&table, listener, None, 0.0);
    // Offset (300, -400, 0): length 500, dist 0.5, dot 0.8: right (int) (255 * 0.5 * 1.8),
    // left (int) (255 * 0.5 * 0.2).
    let a = mixer
        .start_sound(&table, &mut rand, start(2, 1, beep, [400.0, -300.0, 0.0]))
        .unwrap();
    assert_eq!(
        (mixer.channels()[a].leftvol, mixer.channels()[a].rightvol),
        (25, 229)
    );
    // Offset (0, -300, 400): length 500, dot 0.6; in single precision 1 - dot is just under
    // 0.4 and 255 * 0.5 * that just under 51, so the left truncates to 50, as C does.
    let b = mixer
        .start_sound(&table, &mut rand, start(3, 1, beep, [100.0, -200.0, 400.0]))
        .unwrap();
    assert_eq!(
        (mixer.channels()[b].leftvol, mixer.channels()[b].rightvol),
        (50, 204)
    );
    // Offset (-300, 400, 0): dot -0.8, to the left.
    let c = mixer
        .start_sound(&table, &mut rand, start(4, 1, beep, [-200.0, 500.0, 0.0]))
        .unwrap();
    assert_eq!(
        (mixer.channels()[c].leftvol, mixer.channels()[c].rightvol),
        (229, 25)
    );
}

// spec SND-2.5: only an identical sound still at its start offsets the new one; one that has
// played on, or another sound at its start, does not
#[test]
fn only_an_identical_sound_at_its_start_offsets() {
    let (mut mixer, table, _, beep, looped, _) = fixture();
    mixer
        .start_sound(&table, &mut Fixed(vec![]), start(2, 1, beep, [0.0; 3]))
        .unwrap();
    mixer.paint_channels(&table, 3);
    assert_eq!(mixer.channels()[4].pos, 3);
    let second = mixer
        .start_sound(&table, &mut Fixed(vec![500]), start(3, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(mixer.channels()[second].pos, 0, "the first has played on");
    let other = mixer
        .start_sound(&table, &mut Fixed(vec![500]), start(5, 1, looped, [0.0; 3]))
        .unwrap();
    assert_eq!(
        mixer.channels()[other].pos,
        0,
        "a different sound at its start"
    );
}

// spec SND-4.3: the ambient volume boundaries: a target of exactly 8 is kept, a volume at its
// target stays, and a step past the target in either direction lands on it
#[test]
fn ambient_volume_boundaries() {
    let (mut mixer, mut table, _, _, _, _) = fixture();
    let files = files(&[
        ("ambience/water1.wav", wav(11025, 8, 1, &[128; 8], Some(0))),
        ("ambience/wind2.wav", wav(11025, 8, 1, &[128; 8], Some(0))),
    ]);
    mixer.precache_ambients(&mut table, &mut files.reader());
    let mut settings = *mixer.settings();
    settings.ambient_level = 1.0;
    mixer.set_settings(settings);
    let listener = Listener::default();
    mixer.update(&table, listener, Some([8, 50, 0, 0]), 1.0);
    assert_eq!(mixer.channels()[0].master_vol, 8, "exactly 8 is not zeroed");
    assert_eq!(
        mixer.channels()[1].master_vol,
        50,
        "a step of 100 lands on 50"
    );
    mixer.update(&table, listener, Some([8, 50, 0, 0]), 1.0);
    assert_eq!(
        mixer.channels()[1].master_vol,
        50,
        "at the target nothing moves"
    );
    mixer.update(&table, listener, Some([7, 0, 0, 0]), 1.0);
    assert_eq!(mixer.channels()[0].master_vol, 0, "7 is below 8, so 0");
    assert_eq!(
        mixer.channels()[1].master_vol,
        0,
        "a step of 100 lands on 0"
    );
}

// spec SND-4.4: a static channel silent on one side still combines; dynamic channels never
// combine; a static channel of another sound breaks the chain and the next of the first
// sound finds the first again
#[test]
fn combining_edges() {
    let files = files(&[
        (
            "loop.wav",
            wav(
                11025,
                8,
                1,
                &[200, 128, 100, 128, 150, 128, 50, 128],
                Some(4),
            ),
        ),
        ("loop2.wav", wav(11025, 8, 1, &[128; 8], Some(0))),
    ]);
    let mut table = SoundTable::new();
    let mut read = files.reader();
    let looped = table.precache("loop.wav", 11025, &mut read).unwrap();
    let other = table.precache("loop2.wav", 11025, &mut read).unwrap();
    let mut mixer = Mixer::new(MixerSpec::default());
    let mut rand = Fixed(vec![]);
    // Two dynamic sounds of one sound keep their own volumes.
    mixer.start_sound(&table, &mut rand, start(2, 1, looped, [0.0; 3]));
    mixer.start_sound(&table, &mut rand, start(3, 1, looped, [0.0; 3]));
    mixer.update(&table, Listener::default(), None, 0.0);
    assert_eq!(mixer.channels()[4].leftvol, 255);
    assert_eq!(mixer.channels()[5].leftvol, 255);
    // Two static sounds 500 units to the listener's right: left 0, right 255; they combine.
    let a = mixer
        .static_sound(&table, looped, [0.0, -500.0, 0.0], 255.0, 64.0)
        .unwrap();
    let b = mixer
        .static_sound(&table, looped, [0.0, -500.0, 0.0], 255.0, 64.0)
        .unwrap();
    let c = mixer
        .static_sound(&table, other, [0.0; 3], 100.0, 64.0)
        .unwrap();
    let d = mixer
        .static_sound(&table, looped, [0.0, -500.0, 0.0], 255.0, 64.0)
        .unwrap();
    mixer.update(&table, Listener::default(), None, 0.0);
    let channels = mixer.channels();
    assert_eq!((channels[a].leftvol, channels[a].rightvol), (0, 255 * 3));
    assert_eq!((channels[b].leftvol, channels[b].rightvol), (0, 0));
    assert_eq!((channels[c].leftvol, channels[c].rightvol), (100, 100));
    assert_eq!((channels[d].leftvol, channels[d].rightvol), (0, 0));
}

// spec SND-5.3: the right side is scaled by its own volume, and a channel silent on one side
// is still painted on the other
#[test]
fn each_side_has_its_own_volume() {
    let (mut mixer, table, _, _, looped, _) = fixture();
    let listener = Listener::default();
    mixer.update(&table, listener, None, 0.0);
    // 500 units to the right at attenuation 64: dist 0.5, right 255, left 0.
    let index = mixer
        .static_sound(&table, looped, [0.0, -500.0, 0.0], 255.0, 64.0)
        .unwrap();
    assert_eq!(
        (
            mixer.channels()[index].leftvol,
            mixer.channels()[index].rightvol
        ),
        (0, 255)
    );
    let from = mixer.painted();
    mixer.take_output();
    mixer.paint_channels(&table, from + 1);
    let out = mixer.take_output();
    assert_eq!(out, [0, 12485]);
}

// The accessors and the null output
#[test]
fn accessors_and_the_null_output() {
    use crate::{NullOutput, Output};
    let mut table = SoundTable::new();
    assert!(table.is_empty());
    let id = table.find("a.wav").unwrap();
    assert!(!table.is_empty());
    assert_eq!(id.index(), 0);
    assert_eq!(table.find("b.wav").unwrap().index(), 1);
    assert_eq!(
        SoundError::NameTooLong("x".to_owned()).to_string(),
        "Sound name too long: x"
    );
    assert_eq!(SoundError::OutOfSfx.to_string(), "S_FindName: out of sfx_t");
    let mut output = NullOutput::default();
    assert_eq!(output.frames(), 0);
    output.write(&[1, 2, 3, 4, 5, 6]);
    assert_eq!(output.frames(), 3);
}
