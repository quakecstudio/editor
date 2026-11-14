// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; the numbers are the spec's"
)]

use std::collections::HashMap;

use crate::{
    Ambient, Listener, ListenerLeaf, Loaded, LoopPaint, Mixer, MixerSpec, Random, Reader,
    SND_FIRST_STATIC, SND_MAX_CHANNELS, SND_MAX_DYNAMIC_CHANNELS, SND_MAX_NAME, SND_MAX_SFX,
    SND_NUM_AMBIENTS, SamplePos, SfxId, SoundData, SoundEntity, SoundError, SoundTable, Start,
    Width, track_files,
};
use crate::{mixer, sfx};

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
    fn reader(&self) -> Reader {
        let files = self.0.clone();
        Box::new(move |path| files.get(path).cloned())
    }

    fn table(&self) -> SoundTable {
        SoundTable::new(11025, self.reader())
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
        entnum: SoundEntity::new(entnum),
        entchannel,
        sfx,
        origin,
        fvol: 1.0,
        attenuation: 1.0,
    }
}

/// A mixer at the oracle's rate, a table with `beep` (16-bit, 8 samples, no loop), `loop`
/// (8-bit, 8 samples, looping from 4) and `missing` (no file).
fn fixture() -> (Mixer, SoundTable, SfxId, SfxId, SfxId) {
    fixture_with(&[])
}

/// [`fixture`] with `extra` files in the game filesystem too.
fn fixture_with(extra: &[(&str, Vec<u8>)]) -> (Mixer, SoundTable, SfxId, SfxId, SfxId) {
    let mut entries = vec![
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
    ];
    entries.extend(extra.iter().cloned());
    let mut table = files(&entries).table();
    let beep = table.precache("beep.wav").unwrap();
    let looped = table.precache("loop.wav").unwrap();
    let missing = table.precache("missing.wav").unwrap();
    (
        Mixer::new(MixerSpec::default()),
        table,
        beep,
        looped,
        missing,
    )
}

/// The two ambient sounds, eight silent samples each, looping from 0.
fn ambient_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("ambience/water1.wav", wav(11025, 8, 1, &[128; 8], Some(0))),
        ("ambience/wind2.wav", wav(11025, 8, 1, &[128; 8], Some(0))),
    ]
}

/// The outside-the-world leaf, for a test about something else.
const OUT: ListenerLeaf = ListenerLeaf::Outside;

// spec SND-1.1: 128 channels, the four ambient then eight dynamic ones first, static ones
// from 12 up to total_channels; stop_all forgets the static ones and clears every channel
#[test]
fn the_channel_array_and_stop_all() {
    let (mut mixer, mut table, beep, looped, _) = fixture();
    assert_eq!(mixer.channels().len(), SND_MAX_CHANNELS);
    assert_eq!(
        SND_NUM_AMBIENTS + SND_MAX_DYNAMIC_CHANNELS,
        SND_FIRST_STATIC
    );
    assert_eq!(mixer.total_channels(), SND_FIRST_STATIC);
    assert_eq!(Ambient::Lava.index(), 3);
    mixer.static_sound(&mut table, looped, [0.0; 3], 255.0, 1.0);
    mixer.start_sound(&mut table, &mut Fixed(vec![]), start(1, 1, beep, [0.0; 3]));
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
    let (mut mixer, mut table, beep, _, _) = fixture();
    let index = mixer
        .start_sound(
            &mut table,
            &mut Fixed(vec![]),
            start(1, 2, beep, [1.0, 2.0, 3.0]),
        )
        .unwrap();
    let channel = mixer.channels()[index];
    assert_eq!(channel.sfx, Some(beep));
    assert_eq!((channel.leftvol, channel.rightvol), (255, 255));
    assert_eq!(channel.end, 8);
    assert_eq!(channel.pos, 0);
    assert_eq!((channel.entnum.get(), channel.entchannel), (1, 2));
    assert_eq!(channel.origin, [1.0, 2.0, 3.0]);
    assert_eq!(channel.dist_mult, 1.0 / 1000.0);
    assert_eq!(channel.master_vol, 255);
}

// spec SND-1.3: a name is known once, at most 63 bytes, in a table of at most 512
#[test]
fn names_are_known_once_with_their_limits() {
    let mut table = files(&[]).table();
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
    let mut table = files(&[("stereo.wav", wav(11025, 8, 2, &[128; 8], None))]).table();
    let missing = table.precache("missing.wav").unwrap();
    let stereo = table.precache("stereo.wav").unwrap();
    assert_eq!(table.loaded(missing), Some(&Loaded::Missing));
    assert_eq!(table.loaded(stereo), Some(&Loaded::Stereo));
    assert!(table.data(missing).is_none() && table.data(stereo).is_none());
    assert_eq!(table.name(missing), Some("missing.wav"));
    let mut junk = SoundTable::new(11025, Box::new(|_| Some(b"not a wav".to_vec())));
    let bad = junk.precache("bad.wav").unwrap();
    assert!(matches!(junk.loaded(bad), Some(Loaded::Refused(_))));
    assert_eq!(junk.speed(), 11025);
    assert!(format!("{junk:?}").starts_with("SoundTable { sfx: ["));
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
            .map(|i| data.sample_8(SamplePos::new(i)).unwrap())
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
            .map(|i| data.sample_8(SamplePos::new(i)).unwrap())
            .collect::<Vec<_>>(),
        [0, 0, 0, 10, 10, 20]
    );
    let same = wav(11025, 16, 1, &sixteen_bit(), None);
    let data = SoundData::resample(&qs_wav::s_load_sound(&same).unwrap(), 11025);
    assert_eq!(data.width(), Width::Sixteen);
    assert_eq!(data.sample_16(SamplePos::new(1)), Some(-1000));
    assert_eq!(data.sample_8(SamplePos::new(1)), None);
    assert_eq!(
        data.sample_16(SamplePos::new(8)),
        Some(0),
        "past the end is silence"
    );
}

// spec SND-2.1: nothing starts under nosound, and the master volume is fvol * 255 truncated
#[test]
fn nosound_and_the_master_volume() {
    let (mut mixer, mut table, beep, _, _) = fixture();
    let mut settings = *mixer.settings();
    settings.nosound = true;
    mixer.set_settings(settings);
    assert_eq!(
        mixer.start_sound(&mut table, &mut Fixed(vec![]), start(1, 1, beep, [0.0; 3])),
        None
    );
    settings.nosound = false;
    mixer.set_settings(settings);
    let half = Start {
        fvol: 0.5,
        ..start(1, 1, beep, [0.0; 3])
    };
    let index = mixer
        .start_sound(&mut table, &mut Fixed(vec![]), half)
        .unwrap();
    assert_eq!(mixer.channels()[index].master_vol, 127);
}

// spec SND-2.2: a sound of the same entity and channel (or any channel for -1) takes that
// channel; channel 0 never overrides; the view entity's sounds are kept from other
// entities; else the channel with the least life left; every channel protected gives none
#[test]
fn picking_a_channel() {
    let mut out = Vec::new();
    let (mut mixer, mut table, beep, looped, _) = fixture();
    let mut rand = Fixed(vec![]);
    let first = mixer
        .start_sound(&mut table, &mut rand, start(2, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(first, SND_NUM_AMBIENTS, "the first dynamic channel");
    let second = mixer
        .start_sound(&mut table, &mut rand, start(3, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(second, 5, "the first channel with no life left");
    let again = mixer
        .start_sound(&mut table, &mut rand, start(3, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(again, 5, "the same entity and channel override");
    let any = mixer
        .start_sound(&mut table, &mut rand, start(2, -1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(any, 4, "channel -1 takes any channel of the entity");
    let zero = mixer
        .start_sound(&mut table, &mut rand, start(2, 0, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(zero, 6, "channel 0 never overrides");
    // The view entity fills every dynamic channel, a frame apart so each start finds a fresh
    // channel with the least life left; a monster sound then has nowhere to go.
    mixer.stop_all();
    for channel in 1..=8 {
        mixer
            .start_sound(&mut table, &mut rand, start(1, channel, looped, [0.0; 3]))
            .unwrap();
        let next = mixer.painted() + 1;
        mixer.paint_channels(&mut table, next, &mut out);
    }
    let ents: Vec<_> = mixer.channels()[SND_NUM_AMBIENTS..SND_FIRST_STATIC]
        .iter()
        .map(|c| (c.entnum.get(), c.entchannel))
        .collect();
    assert!(ents.iter().all(|c| c.0 == 1), "{ents:?}");
    assert_eq!(
        mixer.start_sound(&mut table, &mut rand, start(2, 1, beep, [0.0; 3])),
        None
    );
    assert!(
        matches!(
            mixer.start_sound(&mut table, &mut rand, start(1, 9, beep, [0.0; 3])),
            Some(4..=11)
        ),
        "the view entity's own sound takes the channel with the least life left"
    );
}

// spec SND-2.3, SND-2.4: an inaudible sound and one with no data take the channel without
// playing; a playing one starts at 0 and ends at paintedtime + length
#[test]
fn silent_and_dataless_sounds_take_the_channel() {
    let mut out = Vec::new();
    let (mut mixer, mut table, beep, _, missing) = fixture();
    let mut rand = Fixed(vec![]);
    let far = start(2, 1, beep, [5000.0, 0.0, 0.0]);
    assert_eq!(mixer.start_sound(&mut table, &mut rand, far), None);
    let taken = mixer.channels()[4];
    assert_eq!(taken.sfx, None);
    assert_eq!((taken.entnum.get(), taken.entchannel), (2, 1));
    assert_eq!(
        mixer.start_sound(&mut table, &mut rand, start(3, 1, missing, [0.0; 3])),
        None
    );
    assert_eq!(mixer.channels()[4].sfx, None);
    assert_eq!(
        mixer.channels()[4].entnum.get(),
        3,
        "the same channel, with no life left"
    );
    mixer.paint_channels(&mut table, 100, &mut out);
    let index = mixer
        .start_sound(&mut table, &mut rand, start(4, 1, beep, [0.0; 3]))
        .unwrap();
    assert_eq!(mixer.channels()[index].end, 108);
}

// spec SND-2.5: an identical sound still at its start skips rand () % (0.1 * speed)
// samples, at most end - 1
#[test]
fn an_identical_sound_this_frame_is_offset() {
    let (mut mixer, mut table, beep, _, _) = fixture();
    mixer
        .start_sound(&mut table, &mut Fixed(vec![]), start(2, 1, beep, [0.0; 3]))
        .unwrap();
    let second = mixer
        .start_sound(
            &mut table,
            &mut Fixed(vec![1102 + 3]),
            start(3, 1, beep, [0.0; 3]),
        )
        .unwrap();
    assert_eq!(mixer.channels()[second].pos, 3);
    assert_eq!(mixer.channels()[second].end, 5);
    let third = mixer
        .start_sound(
            &mut table,
            &mut Fixed(vec![500]),
            start(4, 1, beep, [0.0; 3]),
        )
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
    let (mut mixer, mut table, beep, _, _) = fixture();
    let mut rand = Fixed(vec![]);
    for entity in 2..=9 {
        mixer
            .start_sound(&mut table, &mut rand, start(entity, 1, beep, [0.0; 3]))
            .unwrap();
    }
    assert_eq!(mixer.channels()[11].entnum.get(), 9);
    mixer.stop_sound(SoundEntity::new(9), 1);
    assert!(
        mixer.channels()[11].sfx.is_some(),
        "channel 11 is past the scan"
    );
    mixer.stop_sound(SoundEntity::new(5), 1);
    assert_eq!(mixer.channels()[7].sfx, None);
    assert_eq!(mixer.channels()[7].end, 0);
}

// spec SND-2.7: a local sound plays on the view entity, channel -1, at full volume
#[test]
fn a_local_sound_is_the_view_entity_s() {
    let (mut mixer, mut table, _, _, _) = fixture();
    let index = mixer
        .local_sound(&mut table, &mut Fixed(vec![]), "beep.wav")
        .unwrap();
    let channel = mixer.channels()[index];
    assert_eq!((channel.entnum.get(), channel.entchannel), (1, -1));
    assert_eq!((channel.leftvol, channel.rightvol), (255, 255));
    let long = "x".repeat(SND_MAX_NAME + 1);
    assert_eq!(
        mixer.local_sound(&mut table, &mut Fixed(vec![]), &long),
        None
    );
    assert_eq!(
        mixer.take_messages(),
        [format!("S_LocalSound: can't cache {long}")]
    );
}

// spec SND-3.1, SND-3.2: the view entity's sound is at full volume; another's is scaled by
// 1 - dist and panned by the listener's right vector, silent at or past the clip distance,
// and unpanned on a mono output
#[test]
fn spatialization() {
    let mut out = Vec::new();
    let (mut mixer, mut table, beep, _, _) = fixture();
    let mut rand = Fixed(vec![]);
    let listener = Listener {
        origin: [100.0, 100.0, 0.0],
        ..Listener::default()
    };
    mixer.update(&mut table, listener, OUT, 0.0, &mut out);
    let own = mixer
        .start_sound(&mut table, &mut rand, start(1, 1, beep, [9000.0, 0.0, 0.0]))
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
        .start_sound(
            &mut table,
            &mut rand,
            start(2, 1, beep, [100.0, -400.0, 0.0]),
        )
        .unwrap();
    assert_eq!(
        (
            mixer.channels()[right].leftvol,
            mixer.channels()[right].rightvol
        ),
        (0, 255)
    );
    let ahead = mixer
        .start_sound(
            &mut table,
            &mut rand,
            start(3, 1, beep, [600.0, 100.0, 0.0]),
        )
        .unwrap();
    assert_eq!(
        (
            mixer.channels()[ahead].leftvol,
            mixer.channels()[ahead].rightvol
        ),
        (127, 127)
    );
    assert_eq!(
        mixer.start_sound(
            &mut table,
            &mut rand,
            start(4, 1, beep, [1100.0, 100.0, 0.0])
        ),
        None,
        "at the clip distance"
    );
    let mut mono = Mixer::new(MixerSpec {
        stereo: false,
        ..MixerSpec::default()
    });
    mono.update(&mut table, listener, OUT, 0.0, &mut out);
    let index = mono
        .start_sound(
            &mut table,
            &mut rand,
            start(2, 1, beep, [100.0, -400.0, 0.0]),
        )
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
    let (mut mixer, mut table, beep, looped, missing) = fixture();
    assert_eq!(
        mixer.static_sound(&mut table, beep, [0.0; 3], 255.0, 1.0),
        Some(SND_FIRST_STATIC)
    );
    assert_eq!(mixer.take_messages(), ["Sound beep.wav not looped"]);
    assert_eq!(
        mixer.static_sound(&mut table, missing, [0.0; 3], 255.0, 1.0),
        Some(SND_FIRST_STATIC + 1)
    );
    let index = mixer
        .static_sound(&mut table, looped, [10.0, 0.0, 0.0], 100.5, 2.0)
        .unwrap();
    assert_eq!(index, SND_FIRST_STATIC + 2);
    assert_eq!(mixer.total_channels(), SND_FIRST_STATIC + 3);
    let channel = mixer.channels()[index];
    assert_eq!(channel.sfx, Some(looped));
    assert_eq!(channel.master_vol, 100);
    assert_eq!(channel.dist_mult, (2.0 / 64.0) / 1000.0);
    assert_eq!(channel.end, 8);
    while mixer.total_channels() < SND_MAX_CHANNELS {
        mixer.static_sound(&mut table, looped, [0.0; 3], 1.0, 1.0);
    }
    assert_eq!(
        mixer.static_sound(&mut table, looped, [0.0; 3], 1.0, 1.0),
        None
    );
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
    let (mut mixer, mut table, _, _, _) = fixture_with(&ambient_files());
    let mut out = Vec::new();
    mixer.precache_ambients(&mut table);
    let water = table.find("ambience/water1.wav").unwrap();
    let listener = Listener::default();
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([255, 20, 255, 0]),
        0.1,
        &mut out,
    );
    let channels = mixer.channels();
    assert_eq!(channels[0].master_vol, 10, "toward 76.5 by 0.1 * 100");
    assert_eq!((channels[0].leftvol, channels[0].rightvol), (10, 10));
    assert_eq!(channels[1].master_vol, 0, "0.3 * 20 is below 8");
    assert_eq!(channels[2].sfx, None, "slime has no sound");
    for _ in 0..7 {
        mixer.update(
            &mut table,
            listener,
            ListenerLeaf::In([255, 20, 255, 0]),
            0.1,
            &mut out,
        );
    }
    assert_eq!(mixer.channels()[0].master_vol, 76, "never past the target");
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([0, 0, 0, 0]),
        0.005,
        &mut out,
    );
    assert_eq!(
        mixer.channels()[0].master_vol,
        75,
        "falling, a half step truncates toward zero to a whole one"
    );
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([0, 0, 0, 0]),
        0.1,
        &mut out,
    );
    assert_eq!(mixer.channels()[0].master_vol, 65);
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([255, 0, 0, 0]),
        0.005,
        &mut out,
    );
    assert_eq!(
        mixer.channels()[0].master_vol,
        65,
        "rising, a step under 1 / ambient_fade truncates to 0"
    );
    mixer.update(&mut table, listener, ListenerLeaf::NoWorld, 0.1, &mut out);
    assert_eq!(
        mixer.channels()[0].master_vol,
        65,
        "with no world the ambient channels are left alone"
    );
    assert_eq!(mixer.channels()[0].sfx, Some(water));
    mixer.update(&mut table, listener, OUT, 0.1, &mut out);
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
    let mut out = Vec::new();
    let (mut mixer, mut table, _, looped, _) = fixture();
    let a = mixer
        .static_sound(&mut table, looped, [100.0, 0.0, 0.0], 50.0, 1.0)
        .unwrap();
    let b = mixer
        .static_sound(&mut table, looped, [100.0, 0.0, 0.0], 50.0, 1.0)
        .unwrap();
    let c = mixer
        .static_sound(&mut table, looped, [100.0, 0.0, 0.0], 50.0, 1.0)
        .unwrap();
    mixer.update(&mut table, Listener::default(), OUT, 0.0, &mut out);
    let channels = mixer.channels();
    // Each is (int) (50 * (1 - 100 / 64000)), 49; the first holds all three.
    assert_eq!((channels[a].leftvol, channels[a].rightvol), (147, 147));
    assert_eq!((channels[b].leftvol, channels[b].rightvol), (0, 0));
    assert_eq!((channels[c].leftvol, channels[c].rightvol), (0, 0));
}

// spec SND-5.6, SND-5.9: each update plays frametime * speed frames and paints mixahead
// ahead of them, at most the buffer's frames
#[test]
fn the_update_paints_ahead_of_the_clock() {
    let mut out = Vec::new();
    let (mut mixer, mut table, _, _, _) = fixture();
    mixer.update(&mut table, Listener::default(), OUT, 0.1, &mut out);
    assert_eq!(mixer.sound_time(), 1102);
    assert_eq!(mixer.painted(), 1102 + 1102);
    assert_eq!(out.len(), 2 * 1102, "1102 frames, left and right");
    mixer.update(&mut table, Listener::default(), OUT, 0.05, &mut out);
    assert_eq!(mixer.sound_time(), 1102 + 551);
    assert_eq!(mixer.painted(), 1102 + 551 + 1102);
    let mut small = Mixer::new(MixerSpec {
        buffer_frames: 100,
        ..MixerSpec::default()
    });
    small.update(&mut table, Listener::default(), OUT, 0.0, &mut out);
    assert_eq!(small.painted(), 100, "capped at the buffer");
}

// spec SND-5.2, SND-5.3, SND-5.4: a 16-bit sample adds (sample * vol) >> 8 and an 8-bit
// one sample * (vol >> 3) * 8; the sum times the master volume over 256 is clamped to 16
// bits; a sound that does not loop stops at its end and one that loops restarts at loopstart
#[test]
fn painting_mixes_and_loops() {
    let mut out = Vec::new();
    let (mut mixer, mut table, beep, looped, _) = fixture();
    let mut rand = Fixed(vec![]);
    mixer
        .start_sound(&mut table, &mut rand, start(1, 1, beep, [0.0; 3]))
        .unwrap();
    mixer.paint_channels(&mut table, 10, &mut out);
    let taken = std::mem::take(&mut out);
    assert_eq!(taken.len(), 20);
    // (1000 * 255) >> 8 is 996; (996 * 179) >> 8 is 696.
    // (-1000 * 255) >> 8 is -997 (an arithmetic shift rounds down); (-997 * 179) >> 8 is
    // -698.
    assert_eq!(&taken[..4], [696, 696, -698, -698]);
    assert_eq!(&taken[16..], [0, 0, 0, 0], "silence after the sound ends");
    assert_eq!(mixer.channels()[4].sfx, None, "the channel stopped");
    let index = mixer
        .static_sound(&mut table, looped, [0.0; 3], 255.0, 1.0)
        .unwrap();
    mixer.paint_channels(&mut table, 22, &mut out);
    let taken = std::mem::take(&mut out);
    assert_eq!(taken.len(), 24);
    // 72 * 248 is 17856; (17856 * 179) >> 8 is 12485.
    assert_eq!(taken[0], 12485);
    // After 8 samples the loop restarts at 4: samples 4..8 again (150 - 128 = 22).
    // 22 * 248 is 5456; (5456 * 179) >> 8 is 3814.
    assert_eq!(taken[16], 3814);
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
    mixer.start_sound(&mut table, &mut rand, loud);
    mixer.paint_channels(&mut table, 24, &mut out);
    let taken = std::mem::take(&mut out);
    assert_eq!(taken[0], i16::MAX, "clamped");
}

// spec RRE-3.5: a track's files are music/track<NN>.<ext> for each extension, in order
#[test]
fn track_files_name_the_candidates() {
    assert_eq!(
        track_files(2, &["ogg", "mp3"]),
        ["music/track02.ogg", "music/track02.mp3"]
    );
    assert_eq!(track_files(11, &["wav"]), ["music/track11.wav"]);
}

// spec SND-4.3: the water and sky channels have their sounds precached, slime and lava none
#[test]
fn the_ambient_sounds_are_precached() {
    assert_eq!(Ambient::Water.sound_name(), Some("ambience/water1.wav"));
    assert_eq!(Ambient::Sky.sound_name(), Some("ambience/wind2.wav"));
    assert_eq!(Ambient::Slime.sound_name(), None);
    assert_eq!(Ambient::Lava.sound_name(), None);
    let (mut mixer, mut table, _, _, _) = fixture_with(&ambient_files());
    let mut out = Vec::new();
    mixer.precache_ambients(&mut table);
    let water = table.find("ambience/water1.wav").unwrap();
    let wind = table.find("ambience/wind2.wav").unwrap();
    assert_eq!(table.loaded(water), Some(&Loaded::Data));
    mixer.update(
        &mut table,
        Listener::default(),
        ListenerLeaf::In([255; 4]),
        0.1,
        &mut out,
    );
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
    let mut out = Vec::new();
    let (mut mixer, mut table, beep, _, _) = fixture();
    let mut rand = Fixed(vec![]);
    let listener = Listener {
        origin: [100.0, 100.0, 0.0],
        ..Listener::default()
    };
    mixer.update(&mut table, listener, OUT, 0.0, &mut out);
    // Offset (300, -400, 0): length 500, dist 0.5, dot 0.8: right (int) (255 * 0.5 * 1.8),
    // left (int) (255 * 0.5 * 0.2).
    let a = mixer
        .start_sound(
            &mut table,
            &mut rand,
            start(2, 1, beep, [400.0, -300.0, 0.0]),
        )
        .unwrap();
    assert_eq!(
        (mixer.channels()[a].leftvol, mixer.channels()[a].rightvol),
        (25, 229)
    );
    // Offset (0, -300, 400): length 500, dot 0.6; in single precision 1 - dot is just under
    // 0.4 and 255 * 0.5 * that just under 51, so the left truncates to 50, as C does.
    let b = mixer
        .start_sound(
            &mut table,
            &mut rand,
            start(3, 1, beep, [100.0, -200.0, 400.0]),
        )
        .unwrap();
    assert_eq!(
        (mixer.channels()[b].leftvol, mixer.channels()[b].rightvol),
        (50, 204)
    );
    // Offset (-300, 400, 0): dot -0.8, to the left.
    let c = mixer
        .start_sound(
            &mut table,
            &mut rand,
            start(4, 1, beep, [-200.0, 500.0, 0.0]),
        )
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
    let mut out = Vec::new();
    let (mut mixer, mut table, beep, looped, _) = fixture();
    mixer
        .start_sound(&mut table, &mut Fixed(vec![]), start(2, 1, beep, [0.0; 3]))
        .unwrap();
    mixer.paint_channels(&mut table, 3, &mut out);
    assert_eq!(mixer.channels()[4].pos, 3);
    let second = mixer
        .start_sound(
            &mut table,
            &mut Fixed(vec![500]),
            start(3, 1, beep, [0.0; 3]),
        )
        .unwrap();
    assert_eq!(mixer.channels()[second].pos, 0, "the first has played on");
    let other = mixer
        .start_sound(
            &mut table,
            &mut Fixed(vec![500]),
            start(5, 1, looped, [0.0; 3]),
        )
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
    let (mut mixer, mut table, _, _, _) = fixture_with(&ambient_files());
    let mut out = Vec::new();
    mixer.precache_ambients(&mut table);
    let mut settings = *mixer.settings();
    settings.ambient_level = 1.0;
    mixer.set_settings(settings);
    let listener = Listener::default();
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([8, 50, 0, 0]),
        1.0,
        &mut out,
    );
    assert_eq!(mixer.channels()[0].master_vol, 8, "exactly 8 is not zeroed");
    assert_eq!(
        mixer.channels()[1].master_vol,
        50,
        "a step of 100 lands on 50"
    );
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([8, 50, 0, 0]),
        1.0,
        &mut out,
    );
    assert_eq!(
        mixer.channels()[1].master_vol,
        50,
        "at the target nothing moves"
    );
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([7, 0, 0, 0]),
        1.0,
        &mut out,
    );
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
    let mut table = files.table();
    let looped = table.precache("loop.wav").unwrap();
    let other = table.precache("loop2.wav").unwrap();
    let mut mixer = Mixer::new(MixerSpec::default());
    let mut out = Vec::new();
    let mut rand = Fixed(vec![]);
    // Two dynamic sounds of one sound keep their own volumes.
    mixer.start_sound(&mut table, &mut rand, start(2, 1, looped, [0.0; 3]));
    mixer.start_sound(&mut table, &mut rand, start(3, 1, looped, [0.0; 3]));
    mixer.update(&mut table, Listener::default(), OUT, 0.0, &mut out);
    assert_eq!(mixer.channels()[4].leftvol, 255);
    assert_eq!(mixer.channels()[5].leftvol, 255);
    // Two static sounds 500 units to the listener's right: left 0, right 255; they combine.
    let a = mixer
        .static_sound(&mut table, looped, [0.0, -500.0, 0.0], 255.0, 64.0)
        .unwrap();
    let b = mixer
        .static_sound(&mut table, looped, [0.0, -500.0, 0.0], 255.0, 64.0)
        .unwrap();
    let c = mixer
        .static_sound(&mut table, other, [0.0; 3], 100.0, 64.0)
        .unwrap();
    let d = mixer
        .static_sound(&mut table, looped, [0.0, -500.0, 0.0], 255.0, 64.0)
        .unwrap();
    mixer.update(&mut table, Listener::default(), OUT, 0.0, &mut out);
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
    let mut out = Vec::new();
    let (mut mixer, mut table, _, looped, _) = fixture();
    let listener = Listener::default();
    mixer.update(&mut table, listener, OUT, 0.0, &mut out);
    // 500 units to the right at attenuation 64: dist 0.5, right 255, left 0.
    let index = mixer
        .static_sound(&mut table, looped, [0.0, -500.0, 0.0], 255.0, 64.0)
        .unwrap();
    assert_eq!(
        (
            mixer.channels()[index].leftvol,
            mixer.channels()[index].rightvol
        ),
        (0, 255)
    );
    let from = mixer.painted();
    out.clear();
    mixer.paint_channels(&mut table, from + 1, &mut out);
    assert_eq!(out, [0, 12485]);
}

// The accessors and the null output
#[test]
fn accessors_and_the_null_output() {
    use crate::{NullOutput, Output};
    let mut table = files(&[]).table();
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

// spec SND-3.2: each axis of the offset weighs in the pan through the listener's right
// vector, scaled by the offset's length
#[test]
fn every_axis_pans() {
    let mut out = Vec::new();
    let (mut mixer, mut table, beep, _, _) = fixture();
    let mut rand = Fixed(vec![]);
    // The listener turned 36.87 degrees: right is (0.6, -0.8, 0).
    let turned = Listener {
        right: [0.6, -0.8, 0.0],
        ..Listener::default()
    };
    mixer.update(&mut table, turned, OUT, 0.0, &mut out);
    // Offset (500, 0, 0): dist 0.5, dot 0.6: right (int) (255 * 0.5 * 1.6), left
    // (int) (255 * 0.5 * 0.4), the latter just under 51 in single precision.
    let x = mixer
        .start_sound(&mut table, &mut rand, start(2, 1, beep, [500.0, 0.0, 0.0]))
        .unwrap();
    assert_eq!(
        (mixer.channels()[x].leftvol, mixer.channels()[x].rightvol),
        (50, 204)
    );
    // The listener looking straight down: right is (0, 0, -1); an offset (0, 0, -500) is to
    // the right, dist 0.5, dot 1.
    let down = Listener {
        right: [0.0, 0.0, -1.0],
        ..Listener::default()
    };
    mixer.update(&mut table, down, OUT, 0.0, &mut out);
    let z = mixer
        .start_sound(&mut table, &mut rand, start(3, 1, beep, [0.0, 0.0, -500.0]))
        .unwrap();
    assert_eq!(
        (mixer.channels()[z].leftvol, mixer.channels()[z].rightvol),
        (0, 255)
    );
    // Off axis on every component: offset (300, 0, 400), length 500, dot 0.6 with the turned
    // listener's right applies to x only.
    mixer.update(&mut table, turned, OUT, 0.0, &mut out);
    let xyz = mixer
        .start_sound(
            &mut table,
            &mut rand,
            start(4, 1, beep, [300.0, 0.0, 400.0]),
        )
        .unwrap();
    // dot = 0.6 * 0.6 = 0.36: right (int) (127.5 * 1.36) = 173, left (int) (127.5 * 0.64) = 81.
    assert_eq!(
        (
            mixer.channels()[xyz].leftvol,
            mixer.channels()[xyz].rightvol
        ),
        (81, 173)
    );
}

// spec SND-5.6: endtime is the single-precision sum soundtime + mixahead * speed: past 2^24
// frames the sum rounds to an even frame, where int arithmetic would not
#[test]
fn the_end_time_is_a_float_sum() {
    let (mut mixer, mut table, _, _, _) = fixture();
    let mut out = Vec::new();
    let mut settings = *mixer.settings();
    settings.mixahead = 0.2;
    mixer.set_settings(settings);
    // elapsed * 11025 truncates to 16777217, which float rounds to 16777216; + 2205 is
    // 16779421, halfway between the floats 16779420 and 16779422, so to even: 16779420.
    mixer.update(
        &mut table,
        Listener::default(),
        OUT,
        16_777_217.5 / 11025.0,
        &mut out,
    );
    assert_eq!(mixer.sound_time(), 16_777_217);
    assert_eq!(mixer.painted(), 16_779_420);
    assert_eq!(out.len(), 2 * (16_779_420 - 16_777_217));
}

// spec SND-5.7: after a loop restart inside a block the frames go to their place in the
// block by default, and over the block's first frames under LoopPaint::Id
#[test]
fn a_loop_restart_inside_a_block_is_placed_by_the_setting() {
    for (loop_paint, first, ninth) in [
        (LoopPaint::Fixed, 12485, 3814),
        // Frame 0 holds sample 0 (72) plus sample 4 (22): 94 * 248 is 23312, times 179 over
        // 256 is 16300; frame 8 is left silent.
        (LoopPaint::Id, 16300, 0),
    ] {
        let (mut mixer, mut table, _, looped, _) = fixture();
        let mut settings = *mixer.settings();
        settings.loop_paint = loop_paint;
        mixer.set_settings(settings);
        let mut out = Vec::new();
        mixer
            .static_sound(&mut table, looped, [0.0; 3], 255.0, 1.0)
            .unwrap();
        mixer.paint_channels(&mut table, 10, &mut out);
        assert_eq!(out.len(), 20);
        assert_eq!((out[0], out[16]), (first, ninth), "{loop_paint:?}");
    }
    assert_eq!(LoopPaint::default(), LoopPaint::Fixed);
}

// spec SND-5.8: a loop start at or past the length, which hangs id's mixer, stops the
// channel
#[test]
fn a_loop_that_would_never_end_stops() {
    // Three samples at 22050 Hz looping from 2: resampled, length 1 and loopstart 1.
    let (mut mixer, mut table, _, _, _) =
        fixture_with(&[("tight.wav", wav(22050, 8, 1, &[200, 200, 200], Some(2)))]);
    let tight = table.precache("tight.wav").unwrap();
    assert_eq!(table.data(tight).unwrap().length(), 1);
    assert_eq!(table.data(tight).unwrap().loopstart(), Some(1));
    let mut out = Vec::new();
    let index = mixer
        .static_sound(&mut table, tight, [0.0; 3], 255.0, 1.0)
        .unwrap();
    mixer.paint_channels(&mut table, 4, &mut out);
    assert_eq!(mixer.channels()[index].sfx, None);
    assert_eq!(out.len(), 8);
    assert_ne!(out[0], 0, "the one sample played");
    assert_eq!(&out[2..], [0; 6]);
}

// spec SND-5.9: the headless clock is the elapsed seconds times the rate, truncated once, not
// a truncated step per frame
#[test]
fn the_headless_clock_does_not_drift() {
    let (mut mixer, mut table, _, _, _) = fixture();
    let mut out = Vec::new();
    for _ in 0..3 {
        mixer.update(&mut table, Listener::default(), OUT, 0.03, &mut out);
    }
    // 0.09 * 11025 is 992.25; three steps of (int) 330.75 would give 990.
    assert_eq!(mixer.sound_time(), 992);
    mixer.update(&mut table, Listener::default(), OUT, -1.0, &mut out);
    assert_eq!(mixer.sound_time(), 992, "a negative step is ignored");
}

// spec SND-5.10: a mono output gets one sample per frame, the left sum
#[test]
fn a_mono_output_takes_the_left_sum() {
    let (_, mut table, _, looped, _) = fixture();
    let mut mono = Mixer::new(MixerSpec {
        stereo: false,
        ..MixerSpec::default()
    });
    let mut out = Vec::new();
    mono.static_sound(&mut table, looped, [0.0; 3], 255.0, 1.0)
        .unwrap();
    mono.paint_channels(&mut table, 2, &mut out);
    assert_eq!(out, [12485, 0]);
}

// spec SND-2.4, SND-4.2, SND-5.2: a sound known but never loaded is loaded by the start that
// first plays it, and by the paint that first mixes it, through the table's reader
#[test]
fn a_sound_is_loaded_when_first_played() {
    let late_wav = || ("late.wav", wav(11025, 16, 1, &sixteen_bit(), None));
    let mut mixer = Mixer::new(MixerSpec::default());
    let mut table = files(&[late_wav()]).table();
    let late = table.find("late.wav").unwrap();
    assert_eq!(table.loaded(late), None);
    mixer
        .start_sound(&mut table, &mut Fixed(vec![]), start(2, 1, late, [0.0; 3]))
        .unwrap();
    assert_eq!(table.loaded(late), Some(&Loaded::Data));
    let looped = table.find("loop.wav").unwrap();
    assert_eq!(
        mixer.static_sound(&mut table, looped, [0.0; 3], 255.0, 1.0),
        Some(SND_FIRST_STATIC)
    );
    assert_eq!(table.loaded(looped), Some(&Loaded::Missing));
    // A channel started before its sound was tried is loaded when painted.
    let mut mixer = Mixer::new(MixerSpec::default());
    let mut table = files(&[late_wav()]).table();
    let late = table.find("late.wav").unwrap();
    mixer
        .start_sound(&mut table, &mut Fixed(vec![]), start(2, 1, late, [0.0; 3]))
        .unwrap();
    let mut out = Vec::new();
    mixer.paint_channels(&mut table, 2, &mut out);
    assert_eq!(&out[..2], [696, 696]);
}

// spec SND-4.3: an ambient_level of 0 silences the ambient channels even inside a leaf
#[test]
fn ambient_level_zero_silences() {
    let (mut mixer, mut table, _, _, _) = fixture_with(&ambient_files());
    let mut out = Vec::new();
    mixer.precache_ambients(&mut table);
    let listener = Listener::default();
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([255; 4]),
        0.1,
        &mut out,
    );
    assert!(mixer.channels()[0].sfx.is_some());
    let mut settings = *mixer.settings();
    settings.ambient_level = 0.0;
    mixer.set_settings(settings);
    mixer.update(
        &mut table,
        listener,
        ListenerLeaf::In([255; 4]),
        0.1,
        &mut out,
    );
    assert!(
        mixer.channels()[..SND_NUM_AMBIENTS]
            .iter()
            .all(|c| c.sfx.is_none())
    );
}

// spec SND-3.2: the offset is the sound's origin less the listener's on every axis
#[test]
fn the_offset_is_from_the_listener_on_z() {
    let (mut mixer, mut table, beep, _, _) = fixture();
    let mut rand = Fixed(vec![]);
    let mut out = Vec::new();
    // Looking straight down from z 100: right is (0, 0, -1); a sound 500 below is to the
    // right at dist 0.5 (its offset's z is -500, not -400).
    let down = Listener {
        origin: [0.0, 0.0, 100.0],
        right: [0.0, 0.0, -1.0],
        ..Listener::default()
    };
    mixer.update(&mut table, down, OUT, 0.0, &mut out);
    let z = mixer
        .start_sound(&mut table, &mut rand, start(3, 1, beep, [0.0, 0.0, -400.0]))
        .unwrap();
    assert_eq!(
        (mixer.channels()[z].leftvol, mixer.channels()[z].rightvol),
        (0, 255)
    );
}

// spec SND-5.2: a channel silent on the right alone is still painted on the left
#[test]
fn a_channel_silent_on_the_right_is_painted() {
    let (mut mixer, mut table, _, looped, _) = fixture();
    let mut out = Vec::new();
    mixer.update(&mut table, Listener::default(), OUT, 0.0, &mut out);
    // 500 units to the left at attenuation 64: left 255, right 0.
    let index = mixer
        .static_sound(&mut table, looped, [0.0, 500.0, 0.0], 255.0, 64.0)
        .unwrap();
    assert_eq!(
        (
            mixer.channels()[index].leftvol,
            mixer.channels()[index].rightvol
        ),
        (255, 0)
    );
    let from = mixer.painted();
    out.clear();
    mixer.paint_channels(&mut table, from + 1, &mut out);
    assert_eq!(out, [12485, 0]);
}

// No spec statement: P6-32, a sample's place reads as its index, a temporary entity's sound is
// entity -1, and each fixed-point scale agrees with its shift (SND-5.4, SND-1.6)
#[test]
fn sample_places_sound_entities_and_scales() {
    assert_eq!(SamplePos::new(7).get(), 7);
    assert_eq!(SoundEntity::TEMPORARY.get(), -1);
    assert_eq!(SoundEntity::new(3).get(), 3);
    mixer::check_scales();
    sfx::check_scales();
}
