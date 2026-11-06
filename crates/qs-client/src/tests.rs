// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::panic,
    clippy::too_many_lines,
    reason = "test code: a failed unwrap or index is a failed test; the numbers are the spec's"
)]

use qs_mdl::mdl::{MDL_MAGIC, MDL_VERSION, Mdl};
use qs_sim::Message;

use crate::{
    CLIENT_MAX_DLIGHTS, CLIENT_MAX_EDICTS, CLIENT_MAX_STATIC_ENTITIES, CLIENT_MAX_TEMP_ENTITIES,
    CLIENT_MAX_VISEDICTS, CLIENT_SIGNONS, Client, ClientLimits, Contents, Event, FrameInfo, Game,
    LerpSettings, ModelInfo, ModelKind, Models, PART_ABSOLUTE_MIN_PARTICLES, PART_RAMP1,
    PART_RAMP3, ParseError, ParticleType, Particles, Random, Reader, SoundRef, ViewInput,
    ViewSettings, ViewState, angledelta, calc_bob, calc_roll, flags, gamma_table, stat,
};

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

/// A `rand ()` whose values climb, so each draw differs.
struct Counting(i32);

impl Random for Counting {
    fn rand(&mut self) -> i32 {
        self.0 += 7;
        self.0 & 0x7fff
    }
}

/// The models the tests' maps name: an alias model with two frames (one a group of three
/// poses at 0.2 seconds), a rotating one with random sync, a brush model, a sprite and a
/// rocket.
struct TestModels;

fn alias(flags: i32, random_sync: bool) -> ModelInfo {
    ModelInfo {
        kind: ModelKind::Alias {
            frames: vec![
                FrameInfo {
                    first_pose: 0,
                    poses: 1,
                    interval: 0.0,
                },
                FrameInfo {
                    first_pose: 1,
                    poses: 3,
                    interval: 0.2,
                },
            ],
        },
        flags,
        random_sync,
        nolerp: false,
    }
}

impl Models for TestModels {
    fn describe(&mut self, name: &str) -> Option<ModelInfo> {
        match name {
            "maps/test.bsp" | "*1" => Some(ModelInfo::brush()),
            "progs/player.mdl" | "progs/t.mdl" | "progs/bolt.mdl" | "progs/bolt2.mdl"
            | "progs/bolt3.mdl" | "progs/beam.mdl" => Some(alias(0, false)),
            "progs/spin.mdl" => Some(alias(flags::ROTATE, true)),
            "progs/rocket.mdl" => Some(alias(flags::ROCKET, false)),
            "progs/gib.mdl" => Some(alias(flags::GIB, false)),
            "progs/s.spr" => Some(ModelInfo {
                kind: ModelKind::Sprite,
                flags: 0,
                random_sync: false,
                nolerp: false,
            }),
            _ => None,
        }
    }
}

/// The default model list: the world, the player, a thing, the beams, the spinner, a
/// rocket, a gib and a sprite.
const MODELS: [&str; 11] = [
    "maps/test.bsp",
    "progs/player.mdl",
    "progs/t.mdl",
    "progs/bolt.mdl",
    "progs/bolt2.mdl",
    "progs/bolt3.mdl",
    "progs/beam.mdl",
    "progs/spin.mdl",
    "progs/rocket.mdl",
    "progs/gib.mdl",
    "progs/s.spr",
];
const WORLD: usize = 1;
const PLAYER: usize = 2;
const THING: usize = 3;
const BOLT: usize = 4;
const SPIN: usize = 8;
const ROCKET: usize = 9;
const GIB: usize = 10;

fn msg() -> Message {
    Message::new(65536, false)
}

/// A serverinfo message for `models` and `sounds`, maxclients 1, then `svc_setview` 1.
fn serverinfo(models: &[&str], sounds: &[&str]) -> Vec<u8> {
    let mut m = msg();
    m.write_byte(11);
    m.write_long(15);
    m.write_byte(1);
    m.write_byte(0);
    m.write_string(b"The Test");
    for model in models {
        m.write_string(model.as_bytes());
    }
    m.write_byte(0);
    for sound in sounds {
        m.write_string(sound.as_bytes());
    }
    m.write_byte(0);
    m.write_byte(5);
    m.write_short(1);
    m.take()
}

/// `svc_spawnbaseline` for entity `n`.
fn baseline(n: i32, model: i32, origin: [f32; 3], angles: [f32; 3]) -> Vec<u8> {
    let mut m = msg();
    m.write_byte(22);
    m.write_short(n);
    m.write_byte(model);
    m.write_byte(0);
    m.write_byte(0);
    m.write_byte(0);
    for i in 0..3 {
        m.write_coord(origin[i]);
        m.write_angle(angles[i]);
    }
    m.take()
}

/// A datagram: `svc_time`, a minimal client data (health 100), then `updates`.
fn datagram(time: f32, updates: &[Vec<u8>]) -> Vec<u8> {
    let mut m = msg();
    m.write_byte(7);
    m.write_float(time);
    m.write_byte(15);
    m.write_short(1 << 9 | 1 << 14);
    m.write_long(0);
    m.write_byte(0);
    m.write_short(100);
    for _ in 0..5 {
        m.write_byte(0);
    }
    m.write_byte(0);
    for u in updates {
        m.write_bytes(u);
    }
    m.take()
}

/// An entity update of entity `n` with the fields given: the origin and angles under their
/// bits, and the model, frame, effects when `Some`.
fn update(
    n: i32,
    origin: Option<[f32; 3]>,
    angles: Option<[f32; 3]>,
    model: Option<i32>,
    frame: Option<i32>,
    effects: Option<i32>,
    step: bool,
) -> Vec<u8> {
    let mut bits = 0;
    if origin.is_some() {
        bits |= 2 | 4 | 8;
    }
    if angles.is_some() {
        bits |= 1 << 8 | 1 << 4 | 1 << 9;
    }
    if model.is_some() {
        bits |= 1 << 10;
    }
    if frame.is_some() {
        bits |= 1 << 6;
    }
    if effects.is_some() {
        bits |= 1 << 13;
    }
    if step {
        bits |= 1 << 5;
    }
    if n >= 256 {
        bits |= 1 << 14;
    }
    if bits >= 256 {
        bits |= 1;
    }
    let mut m = msg();
    m.write_byte(bits | 128);
    if bits & 1 != 0 {
        m.write_byte(bits >> 8);
    }
    if n >= 256 {
        m.write_short(n);
    } else {
        m.write_byte(n);
    }
    if let Some(model) = model {
        m.write_byte(model);
    }
    if let Some(frame) = frame {
        m.write_byte(frame);
    }
    if let Some(effects) = effects {
        m.write_byte(effects);
    }
    for i in 0..3 {
        if let Some(o) = origin {
            m.write_coord(o[i]);
        }
        if let Some(a) = angles {
            m.write_angle(a[i]);
        }
    }
    m.take()
}

/// A client past the serverinfo, with the default models, no sounds, and baselines for the
/// world, the player (model `PLAYER`) and a thing (model `THING` at (100, 0, 0)).
/// A client connected to a map in the WinQuake mode, whose limits are id's, as the
/// statements of CLIENT-1 to CLIENT-6 state them (CLIENT-8.2).
fn connected() -> Client {
    connected_with(ClientLimits::WINQUAKE)
}

/// A client connected to a map, keeping to `limits`.
fn connected_with(limits: ClientLimits) -> Client {
    let mut client = Client::with_limits(Game::Standard, limits);
    let mut packets = vec![serverinfo(&MODELS, &["a.wav", "b.wav"])];
    packets.push(baseline(0, WORLD as i32, [0.0; 3], [0.0; 3]));
    packets.push(baseline(1, PLAYER as i32, [0.0; 3], [0.0; 3]));
    packets.push(baseline(2, THING as i32, [100.0, 0.0, 0.0], [0.0; 3]));
    let mut signon = msg();
    signon.write_byte(25);
    signon.write_byte(3);
    packets.push(signon.take());
    client
        .frame(0.0, &packets, &mut TestModels, &mut Fixed(vec![]))
        .unwrap();
    client
}

fn frame(client: &mut Client, frametime: f32, packets: &[Vec<u8>]) -> Vec<Event> {
    client
        .frame(frametime, packets, &mut TestModels, &mut Fixed(vec![]))
        .unwrap()
}

// spec CLIENT-1.4: a message's bytes are read as id reads them: a coordinate is a short
// over 8 and an angle a byte over 256 of a turn; a string ends at its NUL
#[test]
fn the_reader_decodes_the_wire() {
    let mut m = msg();
    m.write_byte(200);
    m.write_short(-300);
    m.write_long(70000);
    m.write_float(1.5);
    m.write_string(b"hi");
    m.write_coord(-0.125);
    m.write_angle(90.0);
    let bytes = m.take();
    let mut r = Reader::new(&bytes);
    assert_eq!(r.byte(), 200);
    assert_eq!(r.short(), -300);
    assert_eq!(r.long(), 70000);
    assert_eq!(r.float(), 1.5);
    assert_eq!(r.string(), b"hi");
    assert_eq!(r.coord(), -0.125);
    assert_eq!(r.angle(), 90.0);
    assert!(r.done());
    assert!(!r.bad());
    assert_eq!(r.byte(), -1, "past the end");
    assert!(r.bad());
    let mut r = Reader::new(&[7, 0]);
    assert_eq!(r.short(), 7);
    assert_eq!(r.char(), -1);
    assert_eq!(r.long(), -1);
    assert_eq!(r.float(), -1.0);
    assert_eq!(
        Reader::new(b"abc").string(),
        b"abc",
        "the end ends a string too"
    );
}

// spec CLIENT-1.1: the serverinfo clears the client, checks the protocol and maxclients,
// reads the level name and the model and sound names, loads every model, and makes the
// world model 1
#[test]
fn the_serverinfo_starts_a_map() {
    let mut client = Client::new(Game::Standard);
    let events = frame(&mut client, 0.0, &[serverinfo(&MODELS, &["a.wav"])]);
    assert_eq!(client.maxclients(), 1);
    assert_eq!(client.levelname(), b"The Test");
    assert_eq!(client.viewentity(), 1);
    assert_eq!(client.entities()[0].model, 1, "the world");
    assert!(matches!(
        client.model(THING),
        Some(ModelInfo {
            kind: ModelKind::Alias { .. },
            ..
        })
    ));
    assert!(client.model(0).is_none());
    assert!(client.model(12).is_none());
    // The divider and the level's name come first (CLIENT-1.7).
    assert_eq!(
        events[2..],
        [Event::NewMap {
            level: b"The Test".to_vec(),
            models: MODELS.iter().map(|m| (*m).to_owned()).collect(),
            sounds: vec!["a.wav".to_owned()],
        }]
    );
    let mut bad = serverinfo(&MODELS, &[]);
    bad[1] = 14;
    assert_eq!(
        Client::new(Game::Standard).frame(0.0, &[bad], &mut TestModels, &mut Fixed(vec![])),
        Err(ParseError::Version(14))
    );
    let mut bad = serverinfo(&MODELS, &[]);
    bad[5] = 17;
    assert_eq!(
        Client::new(Game::Standard).frame(0.0, &[bad], &mut TestModels, &mut Fixed(vec![])),
        Err(ParseError::MaxClients(17))
    );
    assert_eq!(
        Client::new(Game::Standard).frame(
            0.0,
            &[serverinfo(&["maps/test.bsp", "progs/nope.mdl"], &[])],
            &mut TestModels,
            &mut Fixed(vec![])
        ),
        Err(ParseError::ModelNotFound("progs/nope.mdl".to_owned()))
    );
    let many: Vec<String> = (0..256).map(|i| format!("maps/test.bsp{i}")).collect();
    let names: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(
        Client::new(Game::Standard).frame(
            0.0,
            &[serverinfo(&names, &[])],
            &mut TestModels,
            &mut Fixed(vec![])
        ),
        Err(ParseError::TooManyPrecaches)
    );
    assert_eq!(
        ParseError::Version(1).to_string(),
        "Server returned version 1, not 15"
    );
}

// spec CLIENT-1.2: a slot past num_entities raises it; MAX_EDICTS is an error
// spec CLIENT-1.3: a baseline sets the slot's model, frame, colormap, skin, origin and
// angles; a static entity is drawn from its baseline, at most 128 of them
#[test]
fn baselines_and_statics() {
    let mut client = connected();
    assert_eq!(client.entities().len(), 3);
    let b = client.entities()[2].baseline;
    assert_eq!((b.modelindex, b.origin), (THING, [100.0, 0.0, 0.0]));
    frame(
        &mut client,
        0.0,
        &[baseline(9, THING as i32, [1.0; 3], [0.0; 3])],
    );
    assert_eq!(client.entities().len(), 10);
    let error = client.frame(
        0.0,
        &[baseline(CLIENT_MAX_EDICTS as i32, 1, [0.0; 3], [0.0; 3])],
        &mut TestModels,
        &mut Fixed(vec![]),
    );
    assert_eq!(error, Err(ParseError::EntityNumber(600)));
    let mut stat = baseline(0, THING as i32, [5.0, 6.0, 7.0], [0.0, 90.0, 0.0]);
    stat[0] = 20;
    stat.remove(1);
    stat.remove(1);
    frame(&mut client, 0.0, std::slice::from_ref(&stat));
    let s = client.statics()[0];
    assert_eq!(
        (s.model, s.origin, s.angles[1]),
        (THING, [5.0, 6.0, 7.0], 90.0)
    );
    let scene = client.scene();
    assert_eq!(scene.statics.len(), 1);
    assert_eq!(scene.statics[0].origin, [5.0, 6.0, 7.0]);
    for _ in 1..CLIENT_MAX_STATIC_ENTITIES {
        frame(&mut client, 0.0, std::slice::from_ref(&stat));
    }
    assert_eq!(
        client.frame(0.0, &[stat], &mut TestModels, &mut Fixed(vec![])),
        Err(ParseError::TooManyStatics)
    );
}

// spec CLIENT-8.1: by default a client keeps QuakeSpasm's limits: past id's 128 static
// entities up to 4096, the next one refused; a map reload keeps the limits
#[test]
fn quakespasm_limits_are_the_default() {
    assert_eq!(ClientLimits::default(), ClientLimits::QUAKESPASM);
    assert_eq!(
        (
            ClientLimits::QUAKESPASM.statics,
            ClientLimits::QUAKESPASM.visedicts,
            ClientLimits::QUAKESPASM.dlights,
            ClientLimits::QUAKESPASM.beams,
            ClientLimits::QUAKESPASM.temp_entities
        ),
        (4096, 4096, 64, 32, 256)
    );
    let mut client = connected_with(ClientLimits::default());
    let mut stat = baseline(0, THING as i32, [5.0, 6.0, 7.0], [0.0, 90.0, 0.0]);
    stat[0] = 20;
    stat.remove(1);
    stat.remove(1);
    for _ in 0..4096 {
        frame(&mut client, 0.0, std::slice::from_ref(&stat));
    }
    assert_eq!(client.statics().len(), 4096);
    assert_eq!(
        client.frame(0.0, &[stat], &mut TestModels, &mut Fixed(vec![])),
        Err(ParseError::TooManyStatics)
    );
    // spec CLIENT-8.2: the WinQuake mode keeps id's
    assert_eq!(ClientLimits::WINQUAKE.statics, CLIENT_MAX_STATIC_ENTITIES);
    assert_eq!(ClientLimits::WINQUAKE.dlights, CLIENT_MAX_DLIGHTS);
}

// spec CLIENT-1.4: each command of a server message; an unknown one and a short one are
// errors
// spec CLIENT-4.2: a static sound's origin, index, volume and attenuation go to the mixer
#[test]
fn every_command_is_read() {
    let mut client = connected();
    let mut m = msg();
    m.write_byte(1); // nop
    m.write_byte(4);
    m.write_long(15); // version
    m.write_byte(8);
    m.write_string(b"printed");
    m.write_byte(26);
    m.write_string(b"centred");
    m.write_byte(9);
    m.write_string(b"cmd\n");
    m.write_byte(10);
    m.write_angle(10.0);
    m.write_angle(20.0);
    m.write_angle(30.0);
    m.write_byte(5);
    m.write_short(2); // setview
    m.write_byte(12);
    m.write_byte(3);
    m.write_string(b"az"); // lightstyle
    m.write_byte(16);
    m.write_short((5 << 3) | 2); // stopsound
    m.write_byte(13);
    m.write_byte(0);
    m.write_string(b"Ranger"); // updatename
    m.write_byte(14);
    m.write_byte(0);
    m.write_short(-2); // updatefrags
    m.write_byte(17);
    m.write_byte(0);
    m.write_byte(0x4d); // updatecolors
    m.write_byte(24);
    m.write_byte(1); // setpause
    m.write_byte(27); // killedmonster
    m.write_byte(28); // foundsecret
    m.write_byte(3);
    m.write_byte(stat::TOTALSECRETS as i32);
    m.write_long(7); // updatestat
    m.write_byte(29);
    m.write_coord(1.0);
    m.write_coord(2.0);
    m.write_coord(3.0);
    m.write_byte(2);
    m.write_byte(128);
    m.write_byte(64); // spawnstaticsound
    m.write_byte(32);
    m.write_byte(4);
    m.write_byte(5); // cdtrack
    m.write_byte(33); // sellscreen
    m.write_byte(30); // intermission
    let events = frame(&mut client, 0.1, &[m.take()]);
    assert_eq!(
        events,
        [
            Event::Print(b"printed".to_vec()),
            Event::CenterPrint(b"centred".to_vec()),
            Event::StuffText(b"cmd\n".to_vec()),
            Event::StopSound {
                entity: 5,
                channel: 2
            },
            Event::Pause(true),
            Event::StaticSound {
                origin: [1.0, 2.0, 3.0],
                sound: 2,
                volume: 128,
                attenuation: 64
            },
            Event::CdTrack {
                track: 4,
                loop_track: 5
            },
            Event::SellScreen,
        ]
    );
    assert_eq!(client.viewangles(), [9.84375, 19.6875, 29.53125]);
    assert_eq!(client.viewentity(), 2);
    assert_eq!(client.lightstyles()[3], b"az");
    assert_eq!(client.scores()[0].name, b"Ranger");
    assert_eq!(client.scores()[0].frags, -2);
    assert_eq!(client.scores()[0].colors, 0x4d);
    assert_eq!(
        client.scene().colors[0],
        0x4d,
        "the scene carries each slot's colours"
    );
    assert!(client.paused());
    assert_eq!(client.stats()[stat::MONSTERS], 1);
    assert_eq!(client.stats()[stat::SECRETS], 1);
    assert_eq!(client.stats()[stat::TOTALSECRETS], 7);
    assert_eq!(client.intermission(), 1);
    assert_eq!(
        client.completed_time(),
        0.1,
        "the clock when the message was read"
    );
    let mut finale = msg();
    finale.write_byte(31);
    finale.write_string(b"The End");
    assert_eq!(
        frame(&mut client, 0.0, &[finale.take()]),
        [Event::CenterPrint(b"The End".to_vec())]
    );
    assert_eq!(client.intermission(), 2);
    let mut cutscene = msg();
    cutscene.write_byte(34);
    cutscene.write_string(b"...");
    frame(&mut client, 0.0, &[cutscene.take()]);
    assert_eq!(client.intermission(), 3);
    let mut bye = msg();
    bye.write_byte(2);
    bye.write_byte(99);
    assert_eq!(
        frame(&mut client, 0.0, &[bye.take()]),
        [Event::Disconnect],
        "nothing is read after a disconnect"
    );
    let errors: [(Vec<u8>, ParseError); 8] = [
        (vec![99], ParseError::Illegible(99)),
        (vec![4, 14, 0, 0, 0], ParseError::Version(14)),
        (vec![7, 0], ParseError::BadMessage),
        (vec![12, 64, 0], ParseError::LightStyle(64)),
        (vec![13, 1, 0], ParseError::Scoreboard(1)),
        (vec![25, 3], ParseError::Signon(3, 3)),
        (vec![3, 32, 0, 0, 0, 0], ParseError::Stat(32)),
        (vec![23, 14], ParseError::TempEntity(14)),
    ];
    for (bytes, want) in errors {
        let mut client = connected();
        assert_eq!(
            client.frame(
                0.0,
                std::slice::from_ref(&bytes),
                &mut TestModels,
                &mut Fixed(vec![])
            ),
            Err(want.clone()),
            "{bytes:?}"
        );
        assert!(!want.to_string().is_empty());
    }
}

// spec RRE-2.2: svc_localsound's index is a byte, a short under SND_LARGESOUND, and one at or
// past MAX_SOUNDS is the error
// spec RRE-2.3: svc_achievement's string is read and ignored
// spec RRE-2.5: the bound is id's 256, and a negative index is the same error
#[test]
fn the_rereleases_local_sound_and_achievement() {
    let mut client = connected();
    let mut m = msg();
    m.write_byte(52);
    m.write_string(b"ACH_BEAT_E1"); // achievement
    m.write_byte(56);
    m.write_byte(0);
    m.write_byte(255); // localsound
    m.write_byte(56);
    m.write_byte(1 << 4);
    m.write_short(7); // localsound, large
    m.write_byte(56);
    m.write_byte(0x0f);
    m.write_byte(3); // localsound, the other bits ignored
    assert_eq!(
        frame(&mut client, 0.1, &[m.take()]),
        [
            Event::LocalSound(255),
            Event::LocalSound(7),
            Event::LocalSound(3)
        ]
    );
    let errors: [(Vec<u8>, ParseError); 2] = [
        (vec![56, 1 << 4, 0, 1], ParseError::LocalSound(256)),
        (vec![56, 1 << 4, 0xff, 0xff], ParseError::LocalSound(-1)),
    ];
    for (bytes, want) in errors {
        let mut client = connected();
        assert_eq!(
            client.frame(
                0.0,
                std::slice::from_ref(&bytes),
                &mut TestModels,
                &mut Fixed(vec![])
            ),
            Err(want.clone()),
            "{bytes:?}"
        );
    }
    assert_eq!(
        ParseError::LocalSound(256).to_string(),
        "CL_ParseLocalSound: 256 > MAX_SOUNDS"
    );
}

// spec CLIENT-1.5: the signon stages climb and the first update after 3 completes them
#[test]
fn the_signon_completes_on_the_first_update() {
    let mut client = Client::new(Game::Standard);
    frame(&mut client, 0.0, &[serverinfo(&MODELS, &[])]);
    let mut s = msg();
    s.write_byte(25);
    s.write_byte(1);
    assert_eq!(frame(&mut client, 0.0, &[s.take()]), [Event::Signon(1)]);
    assert_eq!(client.signon(), 1);
    let mut s = msg();
    s.write_byte(25);
    s.write_byte(3);
    frame(&mut client, 0.0, &[s.take()]);
    assert_eq!(client.signon(), 3);
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.0,
            &[update(
                1,
                None,
                None,
                Some(PLAYER as i32),
                None,
                None,
                false,
            )],
        )],
    );
    assert_eq!(client.signon(), CLIENT_SIGNONS);
}

// spec CLIENT-2.1: the update's bits, a second byte under U_MOREBITS and a short entity
// under U_LONGENTITY
// spec CLIENT-2.3: each field is read under its bit and taken from the baseline otherwise
// spec CLIENT-1.6: a colormap is a client slot, and one past maxclients is an error
#[test]
fn updates_read_their_bits() {
    let mut client = connected();
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.0,
            &[
                update(2, None, None, None, Some(1), Some(8), false),
                update(
                    300,
                    Some([1.0, 2.0, 3.0]),
                    Some([0.0, 90.0, 0.0]),
                    Some(THING as i32),
                    None,
                    None,
                    false,
                ),
            ],
        )],
    );
    let thing = client.entities()[2];
    assert_eq!(thing.frame, 1);
    assert_eq!(thing.effects, 8);
    assert_eq!(thing.msg_origins[0], [100.0, 0.0, 0.0], "from the baseline");
    assert_eq!(thing.model, THING);
    let far = client.entities()[300];
    assert_eq!(far.msg_origins[0], [1.0, 2.0, 3.0]);
    assert_eq!(far.msg_angles[0], [0.0, 90.0, 0.0]);
    assert_eq!(far.model, THING);
    let mut m = msg();
    m.write_byte(128 | 1);
    m.write_byte(1 << 3); // U_COLORMAP in the high byte
    m.write_byte(2);
    m.write_byte(1);
    frame(&mut client, 0.0, &[m.take()]);
    assert_eq!(client.entities()[2].colormap, 1);
    let mut m = msg();
    m.write_byte(128 | 1);
    m.write_byte(1 << 3);
    m.write_byte(2);
    m.write_byte(2);
    assert_eq!(
        client.frame(0.0, &[m.take()], &mut TestModels, &mut Fixed(vec![])),
        Err(ParseError::BadColormap(2))
    );
    let mut m = msg();
    m.write_byte(128 | 1);
    m.write_byte(1 << 2); // U_MODEL
    m.write_byte(2);
    m.write_byte(255);
    assert!(
        client
            .frame(0.0, &[m.take()], &mut TestModels, &mut Fixed(vec![]))
            .is_ok(),
        "model 255 is below MAX_MODELS; it has no info"
    );
}

// spec CLIENT-2.2: a slot whose msgtime is not the previous message's is forced
// spec CLIENT-2.4: a model change draws a random sync base for ST_RAND, 0 otherwise; no
// model forces the update
// spec CLIENT-2.5: U_NOLERP forces the slot and a forced update copies the position
#[test]
fn forced_updates_and_sync_bases() {
    let mut client = connected();
    // A fresh slot's msgtime is 0, as mtime[1] is: id does not force the first update, so
    // the previous position is the zero the slot held.
    let first = datagram(
        1.0,
        &[update(
            2,
            Some([10.0, 0.0, 0.0]),
            None,
            None,
            None,
            None,
            false,
        )],
    );
    frame(&mut client, 1.0, &[first]);
    let thing = client.entities()[2];
    assert_eq!(thing.msg_origins, [[10.0, 0.0, 0.0], [0.0; 3]]);
    assert_eq!(thing.origin, [10.0, 0.0, 0.0], "at frac 1");
    // Left out of the next message, then back: forced.
    frame(&mut client, 0.1, &[datagram(1.1, &[])]);
    assert_eq!(client.entities()[2].model, 0);
    let back = datagram(
        1.2,
        &[update(
            2,
            Some([20.0, 0.0, 0.0]),
            None,
            Some(THING as i32),
            None,
            None,
            false,
        )],
    );
    frame(&mut client, 0.1, &[back]);
    let thing = client.entities()[2];
    assert_eq!(
        thing.msg_origins,
        [[20.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
        "forced: the position is copied"
    );
    assert_eq!(thing.origin, [20.0, 0.0, 0.0]);
    assert!(!thing.forcelink, "relinked and cleared");
    let stepped = datagram(
        1.3,
        &[update(
            2,
            Some([30.0, 0.0, 0.0]),
            None,
            None,
            None,
            None,
            true,
        )],
    );
    client
        .frame(0.1, &[stepped], &mut TestModels, &mut Fixed(vec![]))
        .unwrap();
    assert!(client.entities()[2].lerpflags.movestep);
    let spin = datagram(
        1.4,
        &[update(2, None, None, Some(SPIN as i32), None, None, false)],
    );
    client
        .frame(0.1, &[spin], &mut TestModels, &mut Fixed(vec![0x3fff]))
        .unwrap();
    let thing = client.entities()[2];
    assert_eq!(thing.model, SPIN);
    assert!((thing.syncbase - 0.5).abs() < 1e-3, "{}", thing.syncbase);
    let plain = datagram(
        1.5,
        &[update(2, None, None, Some(THING as i32), None, None, false)],
    );
    client
        .frame(0.1, &[plain], &mut TestModels, &mut Fixed(vec![0x3fff]))
        .unwrap();
    assert_eq!(client.entities()[2].syncbase, 0.0);
    let none = datagram(
        1.6,
        &[update(
            2,
            Some([40.0, 0.0, 0.0]),
            None,
            Some(0),
            None,
            None,
            false,
        )],
    );
    frame(&mut client, 0.1, &[none]);
    let thing = client.entities()[2];
    assert_eq!(thing.model, 0);
    assert_eq!(
        thing.msg_origins[1],
        [40.0, 0.0, 0.0],
        "no model forces the update"
    );
}

// spec CLIENT-3.1: the client data's view height, ideal pitch, punch angles and velocity
// under their bits, with defaults
// spec CLIENT-3.2: the items' flash times, the weapon frame, armour, weapon, health, ammo
// and the active weapon, as a bit index for the mission packs
// spec CLIENT-3.3: onground is cleared by every message and set by SU_ONGROUND
#[test]
fn the_client_data_is_read() {
    let mut client = connected();
    let mut m = msg();
    m.write_byte(15);
    m.write_short(1 | 2 | 4 | 64 | 512 | 1024 | 2048 | 4096 | 8192 | 16384);
    m.write_byte(30); // viewheight
    m.write_byte(-5_i32 & 255); // idealpitch
    m.write_byte(3); // punch x
    m.write_byte(-2_i32 & 255); // velocity y / 16
    m.write_long(5 | 32);
    m.write_byte(4); // weaponframe
    m.write_byte(50); // armor
    m.write_byte(PLAYER as i32); // weapon model
    m.write_short(77);
    m.write_byte(9);
    for i in 1..=4 {
        m.write_byte(i);
    }
    m.write_byte(3);
    let bytes = m.take();
    frame(&mut client, 0.25, std::slice::from_ref(&bytes));
    assert_eq!(client.idealpitch(), -5.0);
    assert!(client.onground() && client.inwater());
    assert_eq!(client.items(), 5 | 32);
    assert_eq!(client.item_gettime()[0], 0.25);
    assert_eq!(client.item_gettime()[5], 0.25);
    assert_eq!(client.item_gettime()[1], 0.0);
    let s = client.stats();
    assert_eq!(s[stat::WEAPONFRAME], 4);
    assert_eq!(s[stat::ARMOR], 50);
    assert_eq!(s[stat::WEAPON], PLAYER as i32);
    assert_eq!(s[stat::HEALTH], 77);
    assert_eq!(s[stat::AMMO], 9);
    assert_eq!(&s[stat::SHELLS..=stat::CELLS], [1, 2, 3, 4]);
    assert_eq!(s[stat::ACTIVEWEAPON], 3);
    let view = client.scene().view;
    assert_eq!(
        view.origin[2], 30.0,
        "the view height, no bob at rest, the nudge bounded at 30 above the origin"
    );
    frame(&mut client, 0.1, &[datagram(1.0, &[])]);
    assert!(!client.onground(), "cleared by the next message");
    assert_eq!(client.idealpitch(), 0.0);
    assert_eq!(client.stats()[stat::ARMOR], 0);
    let mut pack = Client::new(Game::MissionPack);
    frame(&mut pack, 0.0, &[serverinfo(&MODELS, &[])]);
    frame(&mut pack, 0.0, &[bytes]);
    assert_eq!(pack.stats()[stat::ACTIVEWEAPON], 8, "1 << byte");
}

// spec CLIENT-4.1: a sound's flags, volume, attenuation, entity and channel, index and
// origin; an entity past MAX_EDICTS is an error
#[test]
fn sounds_are_read() {
    let mut client = connected();
    let mut m = msg();
    m.write_byte(6);
    m.write_byte(3);
    m.write_byte(128);
    m.write_byte(128);
    m.write_short((7 << 3) | 5);
    m.write_byte(2);
    m.write_coord(1.0);
    m.write_coord(2.0);
    m.write_coord(3.0);
    m.write_byte(6);
    m.write_byte(0);
    m.write_short(2 << 3);
    m.write_byte(1);
    m.write_coord(0.0);
    m.write_coord(0.0);
    m.write_coord(0.0);
    let events = frame(&mut client, 0.0, &[m.take()]);
    let Event::Sound(first) = &events[0] else {
        panic!("{events:?}");
    };
    assert_eq!((first.entity, first.channel), (7, 5));
    assert_eq!(first.sound, SoundRef::Index(2));
    assert_eq!(first.origin, [1.0, 2.0, 3.0]);
    assert!((first.volume - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(first.attenuation, 2.0);
    let Event::Sound(second) = &events[1] else {
        panic!("{events:?}");
    };
    assert_eq!((second.volume, second.attenuation), (1.0, 1.0));
    assert_eq!(second.sound, SoundRef::Index(1));
    let mut m = msg();
    m.write_byte(6);
    m.write_byte(0);
    m.write_short(601 << 3);
    assert_eq!(
        client.frame(0.0, &[m.take()], &mut TestModels, &mut Fixed(vec![])),
        Err(ParseError::SoundEntity(601))
    );
}

// spec CLIENT-5.1: each frame the clock advances by frametime before the messages are read
// spec CLIENT-5.2: the lerp fraction between the two latest message times, a gap over 0.1
// treated as 0.1, the fraction clamped and the clock pulled in past 1 percent
// spec CLIENT-5.3: the velocity is interpolated and bobjrotate is 100 degrees a second
#[test]
fn the_clock_and_the_lerp_fraction() {
    let mut client = connected();
    frame(&mut client, 0.5, &[datagram(1.0, &[])]);
    assert_eq!(
        client.time(),
        0.9,
        "the gap from 0 to 1.0 counts as 0.1, and the clock more than 1 percent behind snaps to it"
    );
    frame(
        &mut client,
        0.05,
        &[datagram(
            1.1,
            &[update(
                2,
                Some([110.0, 0.0, 0.0]),
                None,
                None,
                None,
                None,
                false,
            )],
        )],
    );
    assert_eq!(
        client.time(),
        1.0,
        "0.95 is behind 1.0 by more than 1 percent"
    );
    frame(
        &mut client,
        0.05,
        &[datagram(
            1.2,
            &[update(
                2,
                Some([120.0, 0.0, 0.0]),
                None,
                None,
                None,
                None,
                false,
            )],
        )],
    );
    // frac = (1.1 - 1.1) / 0.1 = 0: the thing is drawn at the previous message's spot.
    assert_eq!(client.entities()[2].origin, [110.0, 0.0, 0.0]);
    frame(&mut client, 0.05, &[]);
    assert!(
        (client.entities()[2].origin[0] - 115.0).abs() < 1e-3,
        "{:?}",
        client.entities()[2].origin
    );
    frame(&mut client, 0.5, &[]);
    assert_eq!(
        client.time(),
        1.2,
        "past the latest message by more than 1 percent, the clock snaps to it"
    );
    assert_eq!(client.entities()[2].origin, [120.0, 0.0, 0.0]);
    // A gap of 0.5 between messages counts as 0.1.
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.7,
            &[update(
                2,
                Some([130.0, 0.0, 0.0]),
                None,
                None,
                None,
                None,
                false,
            )],
        )],
    );
    assert_eq!(client.message_time(), 1.7);
    assert_eq!(
        client.time(),
        1.6,
        "below the earlier message by more than 1 percent: the clock snaps to mtime[1]"
    );
    assert_eq!(client.entities()[2].origin, [120.0, 0.0, 0.0]);
    let spin = datagram(
        1.8,
        &[update(2, None, None, Some(SPIN as i32), None, None, false)],
    );
    frame(&mut client, 0.1, &[spin]);
    frame(
        &mut client,
        0.1,
        &[datagram(
            1.9,
            &[update(2, None, None, Some(SPIN as i32), None, None, false)],
        )],
    );
    let yaw = client.entities()[2].angles[1];
    assert!(
        (yaw - 180.0).abs() < 0.1,
        "bobjrotate at 1.8 seconds, the clock: {yaw}"
    );
}

// spec CLIENT-5.4: a slot not in the latest message loses its model; a forced slot takes
// its latest position; a move over 100 units is a teleport drawn at the new spot; angles
// are interpolated the short way round
// spec CLIENT-5.6: the view entity is not drawn, every other slot joins the visible list
#[test]
fn relinking_interpolates_and_drops() {
    let mut client = connected();
    frame(
        &mut client,
        1.0,
        &[datagram(
            1.0,
            &[
                update(
                    1,
                    Some([0.0; 3]),
                    None,
                    Some(PLAYER as i32),
                    None,
                    None,
                    false,
                ),
                update(
                    2,
                    Some([0.0; 3]),
                    Some([0.0, 350.0, 0.0]),
                    None,
                    None,
                    None,
                    false,
                ),
            ],
        )],
    );
    frame(
        &mut client,
        0.05,
        &[datagram(
            1.1,
            &[
                update(
                    1,
                    Some([0.0; 3]),
                    None,
                    Some(PLAYER as i32),
                    None,
                    None,
                    false,
                ),
                update(
                    2,
                    Some([0.0, 10.0, 0.0]),
                    Some([0.0, 10.0, 0.0]),
                    None,
                    None,
                    None,
                    false,
                ),
            ],
        )],
    );
    assert_eq!(client.time(), 1.05);
    let thing = client.entities()[2];
    assert!((thing.origin[1] - 5.0).abs() < 1e-3);
    assert!(
        (thing.angles[1] + 0.703).abs() < 0.01,
        "halfway from -11.25 to 9.84 the short way: {}",
        thing.angles[1]
    );
    let scene = client.scene();
    assert_eq!(
        scene.entities.len(),
        1,
        "the player, the view entity, is not drawn"
    );
    assert_eq!(scene.entities[0].model, THING);
    frame(
        &mut client,
        0.05,
        &[datagram(
            1.2,
            &[
                update(
                    1,
                    Some([0.0; 3]),
                    None,
                    Some(PLAYER as i32),
                    None,
                    None,
                    false,
                ),
                update(2, Some([0.0, 500.0, 0.0]), None, None, None, None, false),
            ],
        )],
    );
    frame(&mut client, 0.05, &[]);
    assert_eq!(
        client.entities()[2].origin,
        [0.0, 500.0, 0.0],
        "a teleport is not interpolated"
    );
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.3,
            &[update(
                1,
                Some([0.0; 3]),
                None,
                Some(PLAYER as i32),
                None,
                None,
                false,
            )],
        )],
    );
    assert_eq!(client.entities()[2].model, 0, "not in the latest message");
    assert!(client.scene().entities.is_empty());
}

// spec CLIENT-5.5: EF_ROTATE takes the rotation yaw; EF_BRIGHTFIELD makes the field;
// muzzle flash, bright and dim lights allocate lights keyed by the slot; model flags leave
// trails and a rocket lights
// spec CLIENT-5.7: a keyed light is reused, else a dead one, else light 0; lights decay
#[test]
fn effects_lights_and_trails() {
    let mut client = connected();
    let packets = [datagram(
        1.0,
        &[
            update(
                2,
                Some([0.0; 3]),
                None,
                Some(THING as i32),
                None,
                Some(1 | 2 | 4 | 8),
                false,
            ),
            update(
                3,
                Some([0.0; 3]),
                None,
                Some(ROCKET as i32),
                None,
                None,
                false,
            ),
            update(4, Some([0.0; 3]), None, Some(GIB as i32), None, None, false),
        ],
    )];
    client
        .frame(1.0, &packets, &mut TestModels, &mut Counting(0))
        .unwrap();
    let scene = client.scene();
    // The flash, the bright light and the dim light share the slot's keyed light, so the
    // dim light, last, is what remains; the rocket has its own.
    assert_eq!(scene.lights.len(), 2, "{:?}", scene.lights);
    assert!(
        scene
            .lights
            .iter()
            .any(|l| l.radius > 200.0 && l.radius < 232.0 && l.minlight == 0.0)
    );
    assert_eq!(
        scene.lights.iter().filter(|l| l.radius == 200.0).count(),
        1,
        "the rocket's"
    );
    assert_eq!(
        client.particles().active().len(),
        162,
        "the bright field; the trails have no length yet"
    );
    let packets = [datagram(
        1.1,
        &[
            update(
                3,
                Some([30.0, 0.0, 0.0]),
                None,
                Some(ROCKET as i32),
                None,
                None,
                false,
            ),
            update(
                4,
                Some([0.0, 9.0, 0.0]),
                None,
                Some(GIB as i32),
                None,
                None,
                false,
            ),
        ],
    )];
    client
        .frame(0.1, &packets, &mut TestModels, &mut Counting(0))
        .unwrap();
    let scene = client.scene();
    let fire = client
        .particles()
        .active()
        .iter()
        .filter(|p| p.kind == ParticleType::Fire)
        .count();
    let blood = client
        .particles()
        .active()
        .iter()
        .filter(|p| p.kind == ParticleType::Grav)
        .count();
    assert_eq!(fire, 10, "a rocket trail every 3 units over 30");
    assert_eq!(blood, 3, "a blood trail every 3 units over 9");
    assert!(
        scene.lights.len() <= 2,
        "the short-lived lights died: {:?}",
        scene.lights
    );
    // An explosion's light (key 0) decays by 300 a second.
    let mut te = msg();
    te.write_byte(23);
    te.write_byte(3);
    te.write_coord(0.0);
    te.write_coord(0.0);
    te.write_coord(0.0);
    frame(&mut client, 0.0, &[te.take()]);
    let before = client
        .scene()
        .lights
        .iter()
        .map(|l| l.radius)
        .fold(0.0_f32, f32::max);
    assert_eq!(before, 350.0);
    // The clock only moves with the server's: a tenth later by its message.
    let later = client.message_time() + 0.1;
    frame(&mut client, 0.1, &[datagram(later, &[])]);
    let after = client
        .scene()
        .lights
        .iter()
        .map(|l| l.radius)
        .fold(0.0_f32, f32::max);
    assert!((after - 320.0).abs() < 1e-3, "{after}");
    // Every light taken: the next explosion reuses light 0.
    for _ in 0..CLIENT_MAX_DLIGHTS {
        let mut te = msg();
        te.write_byte(23);
        te.write_byte(3);
        te.write_coord(1.0);
        te.write_coord(0.0);
        te.write_coord(0.0);
        frame(&mut client, 0.0, &[te.take()]);
    }
    assert!(client.scene().lights.len() <= CLIENT_MAX_DLIGHTS);
}

// spec CLIENT-6.1: each temporary entity's particles and sound
// spec CLIENT-6.2: a beam reuses its entity's slot, else a free one; 24 at most
// spec CLIENT-6.3: an unknown type is an error
// spec CLIENT-6.4: temporary entities every 30 units along a live beam, with its pitch and
// yaw and a random roll; one of the view entity starts at it
#[test]
fn temporary_entities_and_beams() {
    let mut client = connected();
    let tent = |kind: i32, extra: &[i32]| {
        let mut m = msg();
        m.write_byte(23);
        m.write_byte(kind);
        m.write_coord(1.0);
        m.write_coord(2.0);
        m.write_coord(3.0);
        for e in extra {
            m.write_byte(*e);
        }
        m.take()
    };
    let cases: [(i32, usize, Option<&str>); 9] = [
        (7, 30, Some("wizard/hit.wav")),
        (8, 20, Some("hknight/hit.wav")),
        (0, 10, Some("weapons/tink1.wav")),
        (1, 20, Some("weapons/tink1.wav")),
        (2, 20, None),
        (3, 1024, Some("weapons/r_exp3.wav")),
        (4, 1024, Some("weapons/r_exp3.wav")),
        (10, 1024, None),
        (11, 8 * 8 * 14, None),
    ];
    for (kind, count, sound) in cases {
        let mut client = connected();
        // The particles draw five values each before the sound draws its own.
        let mut draws = vec![0; count * 5];
        draws.push(1);
        let events = client
            .frame(0.0, &[tent(kind, &[])], &mut TestModels, &mut Fixed(draws))
            .unwrap();
        assert_eq!(client.particles().active().len(), count, "type {kind}");
        match sound {
            Some(name) => {
                let Some(Event::Sound(s)) = events.first() else {
                    panic!("type {kind}: {events:?}");
                };
                assert_eq!(s.sound, SoundRef::Name(name), "type {kind}");
                assert_eq!(
                    (s.entity, s.channel, s.volume, s.attenuation),
                    (-1, 0, 1.0, 1.0)
                );
            }
            None => assert!(events.is_empty(), "type {kind}: {events:?}"),
        }
    }
    // One spike in five ricochets: rand % 5 of 0, then rand & 3 picks the sound.
    let after = |first: i32, second: i32, count: usize| {
        let mut draws = vec![0; count * 5];
        draws.push(first);
        draws.push(second);
        Fixed(draws)
    };
    let events = client
        .frame(0.0, &[tent(0, &[])], &mut TestModels, &mut after(5, 1, 10))
        .unwrap();
    assert!(
        matches!(&events[0], Event::Sound(s) if s.sound == SoundRef::Name("weapons/ric1.wav")),
        "{events:?}"
    );
    let events = client
        .frame(0.0, &[tent(1, &[])], &mut TestModels, &mut after(10, 2, 20))
        .unwrap();
    assert!(matches!(&events[0], Event::Sound(s) if s.sound == SoundRef::Name("weapons/ric2.wav")));
    let events = client
        .frame(0.0, &[tent(1, &[])], &mut TestModels, &mut after(15, 3, 20))
        .unwrap();
    assert!(matches!(&events[0], Event::Sound(s) if s.sound == SoundRef::Name("weapons/ric3.wav")));
    frame(&mut client, 0.0, &[tent(12, &[4, 3])]);
    assert!(
        client
            .particles()
            .active()
            .iter()
            .any(|p| p.color == 4 && p.kind == ParticleType::Blob)
    );
    // A beam from entity 7, 90 units long along x.
    let beam = |ent: i32, kind: i32, end_x: f32| {
        let mut m = msg();
        m.write_byte(23);
        m.write_byte(kind);
        m.write_short(ent);
        m.write_coord(0.0);
        m.write_coord(0.0);
        m.write_coord(0.0);
        m.write_coord(end_x);
        m.write_coord(0.0);
        m.write_coord(0.0);
        m.take()
    };
    let mut client = connected();
    client
        .frame(
            0.0,
            &[beam(7, 5, 90.0)],
            &mut TestModels,
            &mut Fixed(vec![45]),
        )
        .unwrap();
    let scene = client.scene();
    let bolts: Vec<_> = scene.entities.iter().filter(|e| e.model == BOLT).collect();
    assert_eq!(bolts.len(), 3);
    assert_eq!(bolts[0].origin, [0.0; 3]);
    assert_eq!(bolts[1].origin, [30.0, 0.0, 0.0]);
    assert_eq!(bolts[0].angles, [0.0, 0.0, 45.0]);
    client
        .frame(
            0.0,
            &[beam(7, 5, 30.0)],
            &mut TestModels,
            &mut Fixed(vec![]),
        )
        .unwrap();
    assert_eq!(
        client
            .scene()
            .entities
            .iter()
            .filter(|e| e.model == BOLT)
            .count(),
        1,
        "the same entity's beam is replaced"
    );
    client
        .frame(0.0, &[beam(8, 6, 0.0)], &mut TestModels, &mut Fixed(vec![]))
        .unwrap();
    assert_eq!(
        client
            .scene()
            .entities
            .iter()
            .filter(|e| e.model == BOLT + 1)
            .count(),
        0,
        "a beam of no length places nothing"
    );
    let mut up = msg();
    up.write_byte(23);
    up.write_byte(13);
    up.write_short(9);
    for c in [0.0, 0.0, 0.0, 0.0, 0.0, 60.0] {
        up.write_coord(c);
    }
    frame(&mut client, 0.0, &[up.take()]);
    let beams: Vec<_> = client
        .scene()
        .entities
        .iter()
        .filter(|e| e.model == BOLT + 3)
        .cloned()
        .collect();
    assert_eq!(beams.len(), 2);
    assert_eq!(beams[0].angles[0], 90.0, "straight up");
    let mut down = msg();
    down.write_byte(23);
    down.write_byte(9);
    down.write_short(10);
    for c in [0.0, 0.0, 0.0, 30.0, 30.0, -30.0] {
        down.write_coord(c);
    }
    frame(&mut client, 0.0, &[down.take()]);
    let bolt3 = client
        .scene()
        .entities
        .iter()
        .find(|e| e.model == BOLT + 2)
        .copied()
        .unwrap();
    assert_eq!(bolt3.angles[1], 45.0, "yaw from atan2");
    assert_eq!(
        bolt3.angles[0], 325.0,
        "pitch -35.26 truncated and raised by 360"
    );
    let later = client.message_time() + 0.3;
    frame(&mut client, 0.3, &[datagram(later, &[])]);
    assert!(
        client
            .scene()
            .entities
            .iter()
            .all(|e| e.model < BOLT || e.model > BOLT + 3),
        "every beam ended"
    );
    for i in 0..CLIENT_MAX_TEMP_ENTITIES as i32 + 10 {
        frame(&mut client, 0.0, &[beam(100 + i, 5, 30.0)]);
    }
    assert_eq!(
        client
            .scene()
            .entities
            .iter()
            .filter(|e| e.model == BOLT)
            .count(),
        24,
        "24 beams, one segment each"
    );
    let mut client = connected();
    frame(
        &mut client,
        1.0,
        &[datagram(
            1.0,
            &[update(
                1,
                Some([50.0, 0.0, 0.0]),
                None,
                Some(PLAYER as i32),
                None,
                None,
                false,
            )],
        )],
    );
    frame(&mut client, 0.0, &[beam(1, 5, 80.0)]);
    let bolts: Vec<_> = client
        .scene()
        .entities
        .iter()
        .filter(|e| e.model == BOLT)
        .cloned()
        .collect();
    assert_eq!(
        bolts[0].origin,
        [50.0, 0.0, 0.0],
        "the view entity's beam starts at it"
    );
    assert_eq!(bolts.len(), 1);
    let mut client = connected();
    frame(&mut client, 0.0, &[beam(1, 5, 30.0 * 70.0)]);
    assert_eq!(
        client
            .scene()
            .entities
            .iter()
            .filter(|e| e.model == BOLT)
            .count(),
        CLIENT_MAX_TEMP_ENTITIES
    );
    let _ = CLIENT_MAX_VISEDICTS;
}

// spec CLIENT-7.1: a gap over 0.2 seconds, a model change, a dropped slot or a new one and
// a muzzle flash restart the animation lerp; U_STEP marks a stepping entity
// spec CLIENT-7.3: the pose of a frame group follows the clock; a pose change starts a
// lerp of the frame's interval, 0.1 for a single pose; the blend is clamped; r_lerpmodels 0
// draws the pose alone
#[test]
fn animation_lerps() {
    let mut client = connected();
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.0,
            &[update(2, Some([0.0; 3]), None, None, Some(0), None, false)],
        )],
    );
    let scene = client.scene();
    let poses = scene.entities[0].poses.unwrap();
    assert_eq!(
        (poses.previous, poses.current),
        (0, 0),
        "a new slot restarts the lerp"
    );
    frame(
        &mut client,
        0.05,
        &[datagram(
            1.05,
            &[update(2, Some([0.0; 3]), None, None, Some(1), None, false)],
        )],
    );
    let poses = client.scene().entities[0].poses.unwrap();
    // Frame 1 is a group of 3 poses at 0.2 seconds: at 1.05 its pose is 1 + (int)(1.05 / 0.2) % 3 = 1 + 5 % 3 = 3.
    assert_eq!((poses.previous, poses.current), (0, 3));
    assert!(poses.blend < 0.01, "the lerp just started: {}", poses.blend);
    frame(
        &mut client,
        0.1,
        &[datagram(
            1.15,
            &[update(2, Some([0.0; 3]), None, None, Some(1), None, false)],
        )],
    );
    let poses = client.scene().entities[0].poses.unwrap();
    assert!(
        (poses.blend - 0.5).abs() < 0.01,
        "halfway through 0.2 seconds: {}",
        poses.blend
    );
    // Back to the single frame 0 at 1.19: a lerp of 0.1 seconds from pose 3 to pose 0.
    frame(
        &mut client,
        0.04,
        &[datagram(
            1.19,
            &[update(2, Some([0.0; 3]), None, None, Some(0), None, false)],
        )],
    );
    let poses = client.scene().entities[0].poses.unwrap();
    assert_eq!((poses.previous, poses.current), (3, 0));
    frame(
        &mut client,
        0.1,
        &[datagram(
            1.29,
            &[update(2, Some([0.0; 3]), None, None, Some(0), None, false)],
        )],
    );
    let poses = client.scene().entities[0].poses.unwrap();
    assert_eq!(poses.blend, 1.0);
    assert_eq!(
        (poses.previous, poses.current),
        (0, 0),
        "a finished lerp makes the previous pose the current"
    );
    frame(
        &mut client,
        0.26,
        &[datagram(
            1.55,
            &[update(2, Some([0.0; 3]), None, None, Some(0), None, false)],
        )],
    );
    let poses = client.scene().entities[0].poses.unwrap();
    assert_eq!(
        (poses.previous, poses.current, poses.blend),
        (0, 0, 1.0),
        "a gap over 0.2 restarts"
    );
    frame(
        &mut client,
        0.1,
        &[datagram(
            1.65,
            &[update(
                2,
                Some([0.0; 3]),
                None,
                None,
                Some(0),
                Some(2),
                false,
            )],
        )],
    );
    frame(
        &mut client,
        0.1,
        &[datagram(
            1.75,
            &[update(2, Some([0.0; 3]), None, None, Some(1), None, false)],
        )],
    );
    let poses = client.scene().entities[0].poses.unwrap();
    assert_eq!(
        poses.previous, poses.current,
        "a muzzle flash restarts twice"
    );
    frame(
        &mut client,
        0.1,
        &[datagram(
            1.85,
            &[update(2, Some([0.0; 3]), None, None, Some(0), None, false)],
        )],
    );
    let poses = client.scene().entities[0].poses.unwrap();
    assert_ne!(
        poses.previous, poses.current,
        "the second restart was spent: it lerps again"
    );
    client.set_lerp(LerpSettings {
        models: false,
        moves: true,
    });
    frame(
        &mut client,
        0.05,
        &[datagram(
            1.9,
            &[update(2, Some([0.0; 3]), None, None, Some(1), None, false)],
        )],
    );
    let poses = client.scene().entities[0].poses.unwrap();
    assert_eq!(
        (poses.previous, poses.current, poses.blend),
        (poses.current, poses.current, 1.0)
    );
}

// spec CLIENT-7.2: a stepping entity is relinked at its latest position and moved by its
// own lerp
// spec CLIENT-7.4: the move lerp runs over 0.1 seconds from the previous to the current
// position, angles the short way round; a teleport resets it; r_lerpmove 0 draws the
// position itself
#[test]
fn move_lerps() {
    let mut client = connected();
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.0,
            &[update(
                2,
                Some([0.0; 3]),
                Some([0.0, 350.0, 0.0]),
                None,
                None,
                None,
                true,
            )],
        )],
    );
    frame(
        &mut client,
        0.1,
        &[datagram(
            1.1,
            &[update(
                2,
                Some([10.0, 0.0, 0.0]),
                Some([0.0, 10.0, 0.0]),
                None,
                None,
                None,
                true,
            )],
        )],
    );
    let drawn = client.scene().entities[0];
    assert_eq!(
        client.entities()[2].origin,
        [10.0, 0.0, 0.0],
        "relinked at the latest spot"
    );
    assert!(
        (drawn.origin[0] - 0.0).abs() < 1e-3,
        "the move lerp just started: {:?}",
        drawn.origin
    );
    frame(&mut client, 0.05, &[]);
    let drawn = client.scene().entities[0];
    assert!((drawn.origin[0] - 5.0).abs() < 1e-3, "{:?}", drawn.origin);
    // 350 and 10 degrees arrive as -11.25 and 9.84375: halfway is -0.703.
    assert!((drawn.angles[1] + 0.703).abs() < 0.01, "{:?}", drawn.angles);
    frame(&mut client, 0.1, &[]);
    assert_eq!(client.scene().entities[0].origin, [10.0, 0.0, 0.0]);
    // A stepping entity's update is always forced, so a teleport of one is lerped over 0.1
    // seconds, as QuakeSpasm does; the reset is for the others.
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.25,
            &[update(
                2,
                Some([500.0, 0.0, 0.0]),
                None,
                None,
                None,
                None,
                true,
            )],
        )],
    );
    assert_eq!(client.scene().entities[0].origin, [10.0, 0.0, 0.0]);
    frame(
        &mut client,
        0.1,
        &[datagram(
            1.35,
            &[update(
                2,
                Some([500.0, 0.0, 0.0]),
                None,
                None,
                None,
                None,
                true,
            )],
        )],
    );
    assert_eq!(client.scene().entities[0].origin, [500.0, 0.0, 0.0]);
    // A walking entity that teleports is drawn at its new spot at once.
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.4,
            &[update(2, Some([0.0; 3]), None, None, None, None, false)],
        )],
    );
    frame(
        &mut client,
        0.075,
        &[datagram(
            1.45,
            &[update(
                2,
                Some([1.0, 0.0, 0.0]),
                None,
                None,
                None,
                None,
                false,
            )],
        )],
    );
    let [x, _, _] = client.scene().entities[0].origin;
    assert!(
        (x - 0.5).abs() < 1e-5,
        "lerped by the frame's fraction: {x}"
    );
    client.set_lerp(LerpSettings {
        models: true,
        moves: false,
    });
    frame(
        &mut client,
        0.0,
        &[datagram(
            1.5,
            &[update(
                2,
                Some([520.0, 0.0, 0.0]),
                None,
                None,
                None,
                None,
                true,
            )],
        )],
    );
    assert_eq!(client.scene().entities[0].origin, [520.0, 0.0, 0.0]);
}

// spec PART-1.1: the pool holds 2048 particles, at least 512; clearing frees them all
// spec PART-1.2: an effect stops when the pool is empty
// spec PART-2.1: an explosion's 1024 particles, half explode and half explode2, with the
// scatter of (r % 32) - 16 and (r % 512) - 256
// spec PART-2.2: the colour-mapped explosion's 512 blobs cycle the colours
// spec PART-2.3: the blob explosion's colours and lives
// spec PART-2.4: the run effect's count, colour and velocity dir * 15; 1024 is an explosion
// spec PART-2.5: the lava splash's 1024 particles rise at 50 to 113
// spec PART-2.6: the teleport splash's particles leave along their offsets
// spec PART-2.7: a trail's particles by type: fire, smoke, blood, tracers, slight blood,
// voor, one per 3 units (1 past 128)
// spec PART-2.8: the entity field's 162 particles at 64 along each normal plus 16 forward
#[test]
fn every_effect_fills_the_pool_its_way() {
    let mut pool = Particles::new(10);
    assert_eq!(pool.capacity(), PART_ABSOLUTE_MIN_PARTICLES);
    assert_eq!(Particles::default().capacity(), 2048);
    pool.explosion(&mut Counting(0), 1.0, [0.0; 3]);
    assert_eq!(
        pool.active().len(),
        PART_ABSOLUTE_MIN_PARTICLES,
        "the pool ran out"
    );
    pool.clear();
    assert!(pool.active().is_empty());
    let mut pool = Particles::default();
    let mut rand = Fixed(vec![17, 300, 5, 100, 20, 600]);
    pool.explosion(&mut rand, 1.0, [10.0; 3]);
    let p = pool.active();
    assert_eq!(p.len(), 1024);
    let last = p[p.len() - 1];
    assert_eq!(last.kind, ParticleType::Explode2, "particle 0 is even");
    assert_eq!(last.ramp, 1.0, "17 & 3");
    assert_eq!(last.color, PART_RAMP1[0]);
    // The draws alternate per axis: origin, velocity, origin, velocity, ...
    assert_eq!(
        last.origin,
        [
            10.0 + 300.0 % 32.0 - 16.0,
            10.0 + 100.0 % 32.0 - 16.0,
            10.0 + 600.0 % 32.0 - 16.0
        ]
    );
    assert_eq!(last.velocity, [5.0 - 256.0, 20.0 - 256.0, 0.0 - 256.0]);
    assert_eq!(last.die, 6.0);
    assert_eq!(p[p.len() - 2].kind, ParticleType::Explode);
    let mut pool = Particles::default();
    pool.explosion2(&mut Counting(0), 1.0, [0.0; 3], 100, 3);
    let p = pool.active();
    assert_eq!(p.len(), 512);
    assert_eq!(p[511].color, 100);
    assert_eq!(p[510].color, 101);
    assert_eq!(p[508].color, 100);
    assert!(
        p.iter()
            .all(|q| q.kind == ParticleType::Blob && (q.die - 1.3).abs() < 1e-6)
    );
    let mut pool = Particles::default();
    pool.blob_explosion(&mut Fixed(vec![8, 5, 0, 0, 0, 0, 0, 0, 7]), 1.0, [0.0; 3]);
    let p = pool.active();
    assert_eq!(p.len(), 1024);
    assert_eq!(p[1023].kind, ParticleType::Blob2);
    assert_eq!(p[1023].die, 1.0 + 1.0 + 8.0 * 0.05);
    assert_eq!(p[1023].color, 155);
    assert_eq!(p[1022].kind, ParticleType::Blob);
    assert_eq!(p[1022].die, 2.0, "1 + 1 + (7 & 8) * 0.05");
    assert_eq!(p[1022].color, 66, "the draws ran out: 66 + 0");
    let mut pool = Particles::default();
    pool.run_effect(
        &mut Fixed(vec![3, 2, 16, 32, 48]),
        1.0,
        [0.0; 3],
        [1.0, 0.0, 0.0],
        73,
        2,
    );
    let p = pool.active();
    assert_eq!(p.len(), 2);
    assert_eq!(p[1].die, 1.3);
    assert_eq!(p[1].color, 72 + 2);
    assert_eq!(p[1].kind, ParticleType::SlowGrav);
    assert_eq!(p[1].origin, [-8.0, -8.0, -8.0]);
    assert_eq!(p[1].velocity, [15.0, 0.0, 0.0]);
    pool.run_effect(&mut Counting(0), 1.0, [0.0; 3], [0.0; 3], 0, 1024);
    assert_eq!(pool.active().len(), 1026, "1024 is an explosion");
    let mut pool = Particles::default();
    pool.lava_splash(&mut Fixed(vec![]), 1.0, [0.0; 3]);
    let p = pool.active();
    assert_eq!(p.len(), 1024);
    assert!(
        p.iter()
            .all(|q| q.color == 224 && q.die == 3.0 && q.kind == ParticleType::SlowGrav)
    );
    let last = p[1023];
    assert_eq!(last.origin, [-128.0, -128.0, 0.0]);
    assert!(
        (last.velocity[2] - 50.0 * 256.0 / (128.0_f32 * 128.0 * 2.0 + 256.0 * 256.0).sqrt()).abs()
            < 0.01
    );
    let mut pool = Particles::default();
    pool.teleport_splash(&mut Fixed(vec![]), 1.0, [0.0; 3]);
    let p = pool.active();
    assert_eq!(p.len(), 8 * 8 * 14);
    let last = p[p.len() - 1];
    assert_eq!(last.origin, [-16.0, -16.0, -24.0]);
    assert_eq!(last.color, 7);
    assert!((last.die - 1.2).abs() < 1e-6);
    let mut pool = Particles::default();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [9.0, 0.0, 0.0], 0);
    let p = pool.active();
    assert_eq!(p.len(), 3);
    assert_eq!(p[2].origin, [-3.0, -3.0, -3.0]);
    assert_eq!(p[2].color, PART_RAMP3[0]);
    assert_eq!(p[2].kind, ParticleType::Fire);
    assert_eq!(p[1].origin, [1.0 - 3.0, -3.0, -3.0]);
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [3.0, 0.0, 0.0], 1);
    assert_eq!(
        pool.active()[0].color,
        PART_RAMP3[2],
        "smoke starts two steps up the ramp"
    );
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [3.0, 0.0, 0.0], 2);
    assert_eq!(
        (pool.active()[0].kind, pool.active()[0].color),
        (ParticleType::Grav, 67)
    );
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [6.0, 0.0, 0.0], 3);
    let p = pool.active();
    assert_eq!(p.len(), 2);
    assert_eq!(p[1].die, 1.5);
    assert_eq!(p[1].color, 52);
    assert_eq!(
        p[1].velocity,
        [0.0, -30.0, 0.0],
        "the first tracer, an odd count, turns right"
    );
    assert_eq!(p[0].velocity, [0.0, 30.0, 0.0]);
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [3.0, 0.0, 0.0], 5);
    assert_eq!(pool.active()[0].color, 230);
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [12.0, 0.0, 0.0], 4);
    assert_eq!(
        pool.active().len(),
        2,
        "slight blood takes 6 units a particle"
    );
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [3.0, 0.0, 0.0], 6);
    assert_eq!((pool.active()[0].color, pool.active()[0].die), (152, 1.3));
    assert_eq!(pool.active()[0].origin, [-8.0; 3]);
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [3.0, 0.0, 0.0], 128);
    assert_eq!(pool.active().len(), 3, "128 and up step by 1");
    pool.clear();
    pool.entity_field(&mut Fixed(vec![]), 0.0, [1.0, 2.0, 3.0]);
    let p = pool.active();
    assert_eq!(p.len(), 162);
    assert!(
        p.iter()
            .all(|q| q.color == 0x6f && q.kind == ParticleType::Explode && q.velocity == [0.0; 3])
    );
    let first = p[161];
    assert!(
        (first.origin[0] - (1.0 + 16.0 + (-0.525731 * 64.0))).abs() < 0.01,
        "{:?}",
        first.origin
    );
}

// spec PART-3.1: the dead are freed; each type moves and recolours: fire climbs its ramp
// and rises, explode falls and speeds up, explode2 slows, blobs, gravity
#[test]
fn particles_move_by_their_type() {
    let mut pool = Particles::default();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [3.0, 0.0, 0.0], 0);
    pool.explosion(&mut Fixed(vec![]), 1.0, [0.0; 3]);
    pool.blob_explosion(&mut Fixed(vec![]), 1.0, [0.0; 3]);
    pool.update(0.0, 0.0, 800.0);
    let blobs = pool.active().to_vec();
    pool.clear();
    for p in blobs.iter().take(4) {
        pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [0.0; 3], 0);
        let _ = p;
    }
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [3.0, 0.0, 0.0], 0);
    pool.explosion(&mut Fixed(vec![]), 1.0, [0.0; 3]);
    pool.blob_explosion(&mut Fixed(vec![]), 1.0, [0.0; 3]);
    pool.clear();
    pool.rocket_trail(&mut Fixed(vec![]), 1.0, [0.0; 3], [3.0, 0.0, 0.0], 0);
    pool.explosion(&mut Fixed(vec![]), 1.0, [0.0; 3]);
    pool.explosion2(&mut Fixed(vec![]), 1.0, [0.0; 3], 66, 1);
    pool.teleport_splash(&mut Fixed(vec![]), 1.0, [0.0; 3]);
    let before = pool.active().len();
    pool.update(1.0, 0.1, 800.0);
    assert_eq!(pool.active().len(), before, "nothing dies at its own time");
    let grav = 0.1 * 800.0 * 0.05;
    for p in pool.active() {
        match p.kind {
            ParticleType::Fire => {
                assert_eq!(p.ramp, 0.5);
                assert_eq!(p.color, PART_RAMP3[0]);
                assert_eq!(p.velocity[2], grav);
            }
            ParticleType::Explode => {
                assert_eq!(p.ramp, 1.0);
                assert_eq!(p.color, PART_RAMP1[1]);
                assert!((p.velocity[0] - (-256.0 * 1.4)).abs() < 1e-3);
                assert!((p.velocity[2] - (-256.0 * 1.4 - grav)).abs() < 1e-3);
            }
            ParticleType::Explode2 => {
                assert_eq!(p.ramp, 1.5);
                assert!((p.velocity[0] - (-256.0 * 0.9)).abs() < 1e-3);
            }
            ParticleType::Blob => assert!((p.velocity[0] - (-256.0 * 1.4)).abs() < 1e-3),
            ParticleType::Blob2 => {
                assert!((p.velocity[0] - (-256.0 * 0.6)).abs() < 1e-3);
                assert!((p.velocity[2] - (-256.0 - grav)).abs() < 1e-3);
            }
            ParticleType::SlowGrav | ParticleType::Grav | ParticleType::Static => {}
        }
    }
    let slow = pool
        .active()
        .iter()
        .find(|p| p.kind == ParticleType::SlowGrav)
        .unwrap();
    assert!(
        slow.velocity[2] < 0.0 || slow.origin[2] != -24.0,
        "moved and pulled down"
    );
    for _ in 0..20 {
        pool.update(2.0, 0.1, 800.0);
    }
    assert!(
        pool.active().iter().all(|p| p.kind != ParticleType::Fire),
        "the fire ran its ramp and died"
    );
    pool.update(10.0, 0.1, 800.0);
    assert!(pool.active().is_empty(), "everything died by 10 seconds");
}

// spec VIEW-1.1: the roll is the sideways speed over 200 times 2 degrees, capped at 2
// spec VIEW-1.2: the bob follows the cycle and the speed, between -7 and 4
#[test]
fn roll_and_bob() {
    assert_eq!(
        calc_roll([0.0; 3], [0.0, -100.0, 0.0]),
        1.0,
        "half the roll speed to the right"
    );
    assert_eq!(
        calc_roll([0.0; 3], [0.0, 400.0, 0.0]),
        -2.0,
        "capped, to the left"
    );
    assert_eq!(calc_roll([0.0; 3], [0.0; 3]), 0.0);
    assert_eq!(calc_bob(0.0, [0.0; 3]), 0.0);
    // At speed 320 the bob is 6.4 * 0.3 + 6.4 * 0.7 * sin (cycle): at cycle 0 that is 1.92.
    assert!((calc_bob(0.0, [320.0, 0.0, 0.0]) - 1.92).abs() < 1e-3);
    // A quarter into the cycle, sin (pi / 2) = 1: 6.4, capped at 4.
    assert_eq!(calc_bob(0.15, [320.0, 0.0, 0.0]), 4.0);
    // Three quarters in: sin (3 pi / 2) = -1: 1.92 - 4.48 = -2.56.
    assert!((calc_bob(0.45, [320.0, 0.0, 0.0]) + 2.56).abs() < 1e-3);
    assert!(calc_bob(0.45, [2000.0, 0.0, 0.0]) == -7.0, "floored at -7");
}

// spec VIEW-2.1: the drift starts with the centre speed unless the drift was stopped this
// very time; stopping records the time
// spec VIEW-2.2: not on the ground nothing drifts; with nodrift, running forward long
// enough starts the drift
// spec VIEW-2.3: the pitch moves toward the ideal, faster each frame, landing exactly
#[test]
fn pitch_drift() {
    let mut view = ViewState::default();
    let mut angles = [20.0, 0.0, 0.0];
    view.drift_pitch(1.0, 0.1, true, 0.0, 0.0, &mut angles);
    assert_eq!(angles[0], 20.0, "nodrift by default");
    view.drift_pitch(1.0, 0.1, true, 200.0, 0.0, &mut angles);
    assert!(view.nodrift, "0.1 of running is short of 0.15");
    view.drift_pitch(1.0, 0.1, true, 200.0, 0.0, &mut angles);
    assert!(
        !view.nodrift,
        "0.2 of running is past 0.15: the drift starts"
    );
    assert_eq!(angles[0], 20.0, "nothing moves the frame it starts");
    view.drift_pitch(1.0, 0.01, true, 0.0, 0.0, &mut angles);
    assert!(
        (angles[0] - 15.0).abs() < 1e-3,
        "move 0.01 * 500: {}",
        angles[0]
    );
    view.drift_pitch(1.0, 0.01, true, 0.0, 0.0, &mut angles);
    assert!(
        (angles[0] - 9.95).abs() < 1e-3,
        "the velocity grew by 5 to 505: {}",
        angles[0]
    );
    view.drift_pitch(1.0, 0.01, true, 0.0, 0.0, &mut angles);
    assert!((angles[0] - 4.85).abs() < 1e-3, "{}", angles[0]);
    view.drift_pitch(1.0, 0.01, true, 0.0, 0.0, &mut angles);
    assert_eq!(
        angles[0], 0.0,
        "5.15 would overshoot: it lands on the ideal"
    );
    view.drift_pitch(1.0, 0.01, true, 0.0, 0.0, &mut angles);
    assert_eq!(angles[0], 0.0);
    let mut angles = [-10.0, 0.0, 0.0];
    view.drift_pitch(1.0, 0.01, true, 0.0, 0.0, &mut angles);
    assert_eq!(angles[0], -10.0, "the velocity starts from 0 again");
    view.drift_pitch(1.0, 0.01, true, 0.0, 0.0, &mut angles);
    assert!(
        angles[0] > -10.0 && angles[0] <= 0.0,
        "upward too: {}",
        angles[0]
    );
    view.drift_pitch(1.0, 0.1, false, 0.0, 0.0, &mut angles);
    let held = angles[0];
    view.drift_pitch(1.0, 0.1, false, 0.0, 0.0, &mut angles);
    assert_eq!(angles[0], held, "not on the ground");
    view.stop_pitch_drift(5.0);
    assert!(view.nodrift);
    view.start_pitch_drift(5.0);
    assert!(view.nodrift, "stopped at this very time");
    view.start_pitch_drift(5.1);
    assert!(!view.nodrift);
    let mut angles = [0.0; 3];
    view.drift_pitch(1.0, 0.1, true, 0.0, 0.0, &mut angles);
    assert_eq!(angles[0], 0.0, "no delta");
}

// spec VIEW-3.1: the eye is the origin plus the view height and the bob, nudged by 1/32
// spec VIEW-3.2: the movement roll, the damage kick while it lasts, and the dead roll
// spec VIEW-3.3: the idle sway, 0 by default
// spec VIEW-3.4: the offsets along the entity's axes, bounded about the origin
// spec VIEW-3.5: the weapon's angles follow the view, with the gun lag swinging back at 20
// degrees a second
// spec VIEW-3.6: the weapon sits at the view height, bobbing, lifted by the view size
// spec VIEW-3.7: a step up is smoothed at 80 units a second, at most 12 behind
// spec VIEW-3.8: the intermission looks from the entity with the idle sway at 1
// spec VIEW-3.9: no view before the map's entities arrive
#[test]
fn the_eye_and_the_weapon() {
    let mut view = ViewState::default();
    let settings = ViewSettings::default();
    let still = ViewInput {
        time: 1.0,
        frametime: 0.1,
        origin: [10.0, 20.0, 30.0],
        entity_angles: [0.0, 0.0, 0.0],
        viewangles: [5.0, 90.0, 0.0],
        velocity: [0.0; 3],
        viewheight: 22.0,
        punchangle: [0.0, 0.0, 1.0],
        onground: true,
        health: 100,
        intermission: false,
        weapon: (3, 2),
    };
    // The first frame on the ground starts the step smoothing from oldz 0, as id's static
    // does: prime it in the air.
    view.render(
        &ViewInput {
            onground: false,
            ..still
        },
        &settings,
    );
    let r = view.render(&still, &settings);
    assert_eq!(
        r.origin,
        [10.0 + 1.0 / 32.0, 20.0 + 1.0 / 32.0, 52.0 + 1.0 / 32.0]
    );
    assert_eq!(r.angles, [5.0, 90.0, 1.0], "the punch angle added");
    assert_eq!(r.entity_angles, [-5.0, 90.0, 0.0]);
    assert_eq!(r.weapon.model, 3);
    assert_eq!(r.weapon.frame, 2);
    assert_eq!(
        r.weapon.origin,
        [10.0, 20.0, 52.0 + 2.0],
        "lifted 2 at view size 100"
    );
    assert_eq!(r.weapon.angles, [-5.0, 90.0, 0.0]);
    let moving = ViewInput {
        velocity: [0.0, -100.0, 0.0],
        ..still
    };
    let r = view.render(&moving, &settings);
    assert!(
        (r.angles[2] - 1.0).abs() < 1e-3,
        "rolling right at half the roll speed, 100 along -y, the right of yaw 90: {}",
        r.angles[2]
    );
    let dead = ViewInput { health: 0, ..still };
    assert_eq!(
        view.render(&dead, &settings).angles[2],
        81.0,
        "the dead roll, then the punch"
    );
    // A damage kick from straight ahead pitches the view by 0.6 * 10 at its start.
    view.damage(
        1.0,
        0,
        5,
        [100.0, 20.0, 30.0],
        still.origin,
        [0.0, 0.0, 0.0],
    );
    let r = view.render(&still, &settings);
    assert!((r.angles[0] - (5.0 + 6.0)).abs() < 1e-3, "{}", r.angles[0]);
    assert!((view.dmg_time - 0.4).abs() < 1e-6);
    let swayed = ViewSettings {
        idlescale: 1.0,
        ..settings
    };
    let mut fresh = ViewState::default();
    fresh.render(
        &ViewInput {
            onground: false,
            ..still
        },
        &settings,
    );
    let r = fresh.render(&still, &swayed);
    assert!(
        (r.angles[2] - (1.0 + (2.0_f32).sin() * 0.1)).abs() < 1e-3,
        "{}",
        r.angles[2]
    );
    let offset = ViewSettings {
        offsets: [100.0, 0.0, 0.0],
        ..settings
    };
    let r = fresh.render(&still, &offset);
    assert_eq!(
        r.origin[1],
        20.0 + 14.0,
        "bounded at 14 along the yaw's forward"
    );
    // The weapon lag: a view that turned 30 degrees since the gun last looked swings back.
    let mut lagged = ViewState::default();
    let r = lagged.render(&still, &settings);
    assert_eq!(r.weapon.angles[1], 90.0);
    // Stepping up 10 units: the old z climbs 8 a frame.
    let mut stepping = ViewState::default();
    stepping.render(
        &ViewInput {
            onground: false,
            ..still
        },
        &settings,
    );
    let up = ViewInput {
        origin: [10.0, 20.0, 40.0],
        ..still
    };
    let r = stepping.render(&up, &settings);
    assert!(
        (r.origin[2] - (62.0 + 1.0 / 32.0 - 2.0)).abs() < 1e-3,
        "8 of the 10 caught up: {}",
        r.origin[2]
    );
    let high = ViewInput {
        origin: [10.0, 20.0, 100.0],
        ..still
    };
    let r = stepping.render(&high, &settings);
    assert!(
        (r.origin[2] - (122.0 + 1.0 / 32.0 - 12.0)).abs() < 1e-3,
        "at most 12 behind: {}",
        r.origin[2]
    );
    let inter = ViewInput {
        intermission: true,
        entity_angles: [1.0, 2.0, 3.0],
        ..still
    };
    let r = view.render(&inter, &settings);
    assert_eq!(r.origin, still.origin);
    assert_eq!(r.weapon.model, 0);
    assert!((r.angles[1] - (2.0 + (1.0_f32).sin() * 0.3)).abs() < 1e-3);
    let mut client = Client::new(Game::Standard);
    assert_eq!(client.scene().view.origin, [0.0; 3], "no map yet");
}

// spec VIEW-4.1: damage shifts the colour by the blood and armour, at least 10, up to 150
// spec VIEW-4.2: the bonus flash
// spec VIEW-4.3: the contents shift
// spec VIEW-4.4: the powerup shift by the first item set
// spec VIEW-4.5: the damage and bonus percents fade
// spec VIEW-4.6: the blend folds the shifts in order
// spec VIEW-4.7: the gamma table is the identity at 1
#[test]
fn colour_shifts_and_the_blend() {
    let mut view = ViewState::default();
    view.damage(1.0, 10, 2, [1.0, 0.0, 0.0], [0.0; 3], [0.0; 3]);
    assert_eq!(view.shifts[1].color, [200, 100, 100]);
    assert_eq!(view.shifts[1].percent, 30, "count 6 raised to 10, times 3");
    assert_eq!(view.faceanimtime, 1.2);
    view.damage(1.0, 2, 3, [1.0, 0.0, 0.0], [0.0; 3], [0.0; 3]);
    assert_eq!(
        view.shifts[1].color,
        [220, 50, 50],
        "some armour, less than the blood"
    );
    view.damage(1.0, 0, 100, [1.0, 0.0, 0.0], [0.0; 3], [0.0; 3]);
    assert_eq!(view.shifts[1].color, [255, 0, 0]);
    assert_eq!(view.shifts[1].percent, 150, "capped");
    view.bonus_flash();
    assert_eq!(
        (view.shifts[2].color, view.shifts[2].percent),
        ([215, 186, 69], 50)
    );
    view.set_contents(Contents::Lava);
    assert_eq!(view.shifts[0].percent, 150);
    view.set_contents(Contents::Slime);
    assert_eq!(view.shifts[0].color, [0, 25, 5]);
    view.set_contents(Contents::Water);
    assert_eq!(view.shifts[0].percent, 128);
    view.set_contents(Contents::Empty);
    assert_eq!(view.shifts[0].percent, 0);
    view.powerup_shift(4_194_304 | 2_097_152);
    assert_eq!(
        (view.shifts[3].color, view.shifts[3].percent),
        ([0, 0, 255], 30),
        "quad first"
    );
    view.powerup_shift(2_097_152);
    assert_eq!(view.shifts[3].color, [0, 255, 0]);
    view.powerup_shift(524_288);
    assert_eq!(view.shifts[3].percent, 100);
    view.powerup_shift(1_048_576);
    assert_eq!(view.shifts[3].color, [255, 255, 0]);
    view.powerup_shift(0);
    assert_eq!(view.shifts[3].percent, 0);
    view.fade(0.5);
    assert_eq!(view.shifts[1].percent, 75);
    assert_eq!(view.shifts[2].percent, 0);
    view.fade(1.0);
    assert_eq!(view.shifts[1].percent, 0);
    let mut only = ViewState::default();
    only.bonus_flash();
    let [r, g, b, a] = only.blend(100.0);
    assert!((a - 50.0 / 255.0).abs() < 1e-6);
    assert!(
        (r - 215.0 / 255.0).abs() < 1e-6
            && (g - 186.0 / 255.0).abs() < 1e-6
            && (b - 69.0 / 255.0).abs() < 1e-6
    );
    only.set_contents(Contents::Lava);
    let [r, _, _, a] = only.blend(100.0);
    // a = 150/255; then the bonus: a = a + 50/255 * (1 - a).
    let a1 = 150.0 / 255.0;
    let want = a1 + 50.0 / 255.0 * (1.0 - a1);
    assert!((a - want).abs() < 1e-6, "{a} {want}");
    assert!(r < 1.0 && r > 215.0 / 255.0 * 0.1);
    assert_eq!(only.blend(0.0), [0.0; 4], "gl_cshiftpercent 0");
    let table = gamma_table(1.0);
    assert!(table.iter().enumerate().all(|(i, &v)| usize::from(v) == i));
    let dark = gamma_table(2.0);
    assert_eq!(dark[0], 0);
    assert_eq!(dark[255], 255);
    assert!(dark[128] < 128);
    assert_eq!(angledelta(270.0), -90.0);
    assert!(
        (angledelta(-10.0) + 10.0).abs() < 0.01,
        "{}",
        angledelta(-10.0)
    );
}

use crate::{
    Anchor, CenterPrint, HudInput, HudItem, Placed, Score, intermission_overlay, sort_frags,
    status_bar,
};

fn hud_input<'a>(
    stats: &'a [i32; 32],
    gettime: &'a [f32; 32],
    scores: &'a [Score],
) -> HudInput<'a> {
    HudInput {
        time: 10.0,
        items: 0,
        item_gettime: gettime,
        stats,
        faceanimtime: 0.0,
        maxclients: 1,
        gametype: 0,
        scores,
        viewentity: 1,
        levelname: b"Test",
        intermission: 0,
        completed_time: 0.0,
        showscores: false,
        lines: 48,
    }
}

fn pics(items: &[Placed]) -> Vec<(String, i32, i32)> {
    items
        .iter()
        .filter_map(|p| match &p.item {
            HudItem::Pic { name, x, y } => Some((name.clone(), *x, *y)),
            _ => None,
        })
        .collect()
}

fn chars(items: &[Placed]) -> Vec<(i32, i32, u8)> {
    items
        .iter()
        .filter_map(|p| match &p.item {
            HudItem::Char { x, y, ch } => Some((*x, *y, *ch)),
            _ => None,
        })
        .collect()
}

// spec HUD-1.1: nothing with no lines; the bar alone at 24; the inventory above 24 and the
// frag list with more than one client; the scoreboard with the scores shown or dead
// spec HUD-1.2: the armour number and picture, 666 and the disc when invulnerable, the health
// and ammo numbers, red when low, and the ammo icon by the first ammo item
// spec HUD-1.6: a number keeps its last digits and right-aligns short ones by 24 a digit
#[test]
fn the_status_bar_shows_the_player() {
    let mut stats = [0; 32];
    stats[stat::HEALTH] = 100;
    stats[stat::ARMOR] = 7;
    stats[stat::AMMO] = 1234;
    let gettime = [0.0; 32];
    let scores = [Score {
        name: b"me".to_vec(),
        frags: 0,
        colors: 0,
    }];
    let mut input = hud_input(&stats, &gettime, &scores);
    input.items = 8192 | 256;
    input.lines = 0;
    assert!(status_bar(&input, 200).is_empty());
    input.lines = 24;
    let bar = status_bar(&input, 200);
    let p = pics(&bar);
    assert_eq!(p[0], ("sbar".to_owned(), 0, 0));
    assert!(
        p.contains(&("anum_7".to_owned(), 24 + 48, 0)),
        "7 armour, red, right-aligned: {p:?}"
    );
    assert!(p.contains(&("sb_armor1".to_owned(), 0, 0)));
    assert!(p.contains(&("face1".to_owned(), 112, 0)));
    assert!(p.contains(&("num_1".to_owned(), 136, 0)) && p.contains(&("num_0".to_owned(), 160, 0)));
    assert!(p.contains(&("sb_shells".to_owned(), 224, 0)));
    assert!(
        p.contains(&("num_2".to_owned(), 248, 0)) && p.contains(&("num_4".to_owned(), 296, 0)),
        "1234 keeps 234"
    );
    assert!(
        !p.iter().any(|(n, _, _)| n == "ibar"),
        "no inventory at 24 lines"
    );
    assert!(bar.iter().all(|p| p.anchor == Anchor::Bar));
    input.lines = 48;
    input.items = 32768 | 2048 | 1_048_576;
    let mut stats2 = stats;
    stats2[stat::AMMO] = 9;
    stats2[stat::ARMOR] = -5;
    let input2 = HudInput {
        stats: &stats2,
        ..input.clone()
    };
    let p = pics(&status_bar(&input2, 200));
    assert!(p.iter().any(|(n, _, _)| n == "ibar"));
    assert!(
        p.contains(&("anum_6".to_owned(), 24, 0)) && p.contains(&("disc".to_owned(), 0, 0)),
        "invulnerable: 666 in red"
    );
    assert!(p.contains(&("sb_cells".to_owned(), 224, 0)));
    assert!(
        p.contains(&("anum_9".to_owned(), 248 + 48, 0)),
        "9 ammo is low"
    );
    let input4 = HudInput {
        items: 32768 | 16384,
        ..input2.clone()
    };
    let p = pics(&status_bar(&input4, 200));
    assert!(
        p.contains(&("sb_armor3".to_owned(), 0, 0)),
        "the armour picture by the first bit set"
    );
    let input3 = HudInput {
        items: 16384,
        ..input2.clone()
    };
    let p = pics(&status_bar(&input3, 200));
    assert!(p.contains(&("sb_armor2".to_owned(), 0, 0)));
    assert!(
        p.contains(&("anum_minus".to_owned(), 24 + 24, 0))
            && p.contains(&("anum_5".to_owned(), 24 + 48, 0)),
        "-5: {p:?}"
    );
    let mut stats3 = stats;
    stats3[stat::HEALTH] = 0;
    let dead = HudInput {
        stats: &stats3,
        ..input.clone()
    };
    let p = pics(&status_bar(&dead, 200));
    assert!(p.contains(&("scorebar".to_owned(), 0, 0)));
    assert!(!p.iter().any(|(n, _, _)| n == "sbar"));
    stats3[stat::HEALTH] = 50;
    let shown = HudInput {
        stats: &stats3,
        showscores: true,
        ..input.clone()
    };
    assert!(
        pics(&status_bar(&shown, 200))
            .iter()
            .any(|(n, _, _)| n == "scorebar")
    );
}

// spec HUD-1.3: the face by the powerups, else by the health in fifths, the pain face while
// the damage animation runs
#[test]
fn the_face_follows_health_and_powerups() {
    let mut stats = [0; 32];
    let gettime = [0.0; 32];
    let scores = [];
    let mut face = |health: i32, items: i32, pain: bool| {
        stats[stat::HEALTH] = health;
        let mut input = hud_input(&stats, &gettime, &scores);
        input.items = items;
        input.faceanimtime = if pain { 11.0 } else { 0.0 };
        pics(&status_bar(&input, 200))
            .into_iter()
            .find(|(n, x, _)| n.starts_with("face") && *x == 112)
            .unwrap()
            .0
    };
    assert_eq!(face(100, 0, false), "face1");
    assert_eq!(face(99, 0, false), "face1", "99 / 20 is 4");
    assert_eq!(face(79, 0, false), "face2");
    assert_eq!(face(45, 0, false), "face3");
    assert_eq!(face(1, 0, false), "face5");
    assert_eq!(face(45, 0, true), "face_p3");
    assert_eq!(face(50, 524_288 | 1_048_576, false), "face_inv2");
    assert_eq!(face(50, 4_194_304, false), "face_quad");
    assert_eq!(face(50, 524_288, false), "face_invis");
    assert_eq!(face(50, 1_048_576, false), "face_invul2");
}

// spec HUD-1.4: each owned weapon's picture, flashing for a second after it is taken and
// highlighted when active; the ammo counts as small yellow digits; the items and sigils
#[test]
fn the_inventory_lists_weapons_ammo_and_items() {
    let mut stats = [0; 32];
    stats[stat::SHELLS] = 25;
    stats[stat::NAILS] = 100;
    stats[stat::CELLS] = 7;
    stats[stat::ACTIVEWEAPON] = 4;
    let mut gettime = [0.0; 32];
    gettime[0] = 10.0;
    gettime[2] = 9.0;
    let scores = [];
    let mut input = hud_input(&stats, &gettime, &scores);
    input.items = 1 | 4 | 64 | (1 << 17) | (1 << 22) | (1 << 28) | (1 << 31);
    let bar = status_bar(&input, 200);
    let p = pics(&bar);
    assert!(p.contains(&("ibar".to_owned(), 0, -24)));
    assert!(
        p.contains(&("inva1_shotgun".to_owned(), 0, -16)),
        "just taken: frame (0 % 5) + 2: {p:?}"
    );
    assert!(
        p.contains(&("inv2_nailgun".to_owned(), 48, -16)),
        "the active weapon, a second on"
    );
    assert!(p.contains(&("inv_lightng".to_owned(), 144, -16)));
    assert!(!p.iter().any(|(n, _, _)| n.contains("sshotgun")));
    assert!(p.contains(&("sb_key1".to_owned(), 192, -16)));
    assert!(p.contains(&("sb_quad".to_owned(), 192 + 80, -16)));
    assert!(
        p.contains(&("sb_sigil1".to_owned(), 288, -16))
            && p.contains(&("sb_sigil4".to_owned(), 288 + 24, -16))
    );
    let c = chars(&bar);
    // " 25": the 2 at column 2 and the 5 at column 3 of the first group, as characters 20 and 23.
    assert!(c.contains(&((2 * 8 - 2) + 4, -24, 20)), "{c:?}");
    assert!(c.contains(&((3 * 8 - 2) + 4, -24, 23)));
    assert!(
        c.contains(&((7 * 8 - 2) + 4, -24, 19)),
        "100 fills all three"
    );
    assert!(
        !c.iter().any(|&(x, _, _)| x == 8 - 2 + 4),
        "a leading space is not drawn"
    );
    input.time = 10.35;
    let p = pics(&status_bar(&input, 200));
    assert!(
        p.contains(&("inva4_shotgun".to_owned(), 0, -16)),
        "frame (3 % 5) + 2: {p:?}"
    );
    input.time = 10.72;
    let p = pics(&status_bar(&input, 200));
    assert!(
        p.contains(&("inva3_shotgun".to_owned(), 0, -16)),
        "frame (7 % 5) + 2"
    );
}

// spec HUD-1.5: up to four clients by frags, colour blocks and the frags, the local one
// bracketed
#[test]
fn the_frag_list_ranks_the_clients() {
    let stats = [0; 32];
    let gettime = [0.0; 32];
    let scores = [
        Score {
            name: b"a".to_vec(),
            frags: 3,
            colors: 0x4d,
        },
        Score {
            name: b"b".to_vec(),
            frags: 7,
            colors: 0,
        },
        Score {
            name: Vec::new(),
            frags: 99,
            colors: 0,
        },
        Score {
            name: b"d".to_vec(),
            frags: 7,
            colors: 0,
        },
    ];
    assert_eq!(
        sort_frags(&scores),
        [1, 3, 0],
        "ties keep slot order; the unnamed is left out"
    );
    let mut input = hud_input(&stats, &gettime, &scores);
    input.maxclients = 4;
    input.viewentity = 1;
    let bar = status_bar(&input, 400);
    let fills: Vec<_> = bar
        .iter()
        .filter_map(|p| match p.item {
            HudItem::Fill { x, y, w, h, color } => Some((x, y, w, h, color)),
            _ => None,
        })
        .collect();
    assert_eq!(
        fills[0],
        (23 * 8 + 10, -23, 28, 4, 8),
        "b's shirt row 0 plus 8"
    );
    assert_eq!(fills[1], (23 * 8 + 10, -19, 28, 3, 8));
    assert_eq!(fills[4], (31 * 8 + 10, -23, 28, 4, 0x40 + 8), "a's shirt");
    assert_eq!(fills[5].4, 0xd0 + 8, "a's trousers");
    let c = chars(&bar);
    assert!(c.contains(&((24 * 8) + 4, -24, b' ')) && c.contains(&((26 * 8) + 4, -24, b'7')));
    assert!(
        c.contains(&((31 * 8 + 2) + 4, -24, 16)) && c.contains(&((35 * 8 - 4) + 4, -24, 17)),
        "a, the local client, is bracketed: {c:?}"
    );
}

// spec HUD-2.1: the solo scoreboard's monsters, secrets, time and level name
// spec HUD-2.2: the deathmatch overlay: the ranking picture, each named client's blocks,
// frags and name, the local one marked
#[test]
fn the_scoreboards() {
    let mut stats = [0; 32];
    stats[stat::MONSTERS] = 3;
    stats[stat::TOTALMONSTERS] = 10;
    stats[stat::SECRETS] = 1;
    stats[stat::TOTALSECRETS] = 2;
    let gettime = [0.0; 32];
    let scores = [
        Score {
            name: b"me".to_vec(),
            frags: 2,
            colors: 0x30,
        },
        Score {
            name: b"you".to_vec(),
            frags: 5,
            colors: 0x01,
        },
    ];
    let mut input = hud_input(&stats, &gettime, &scores);
    input.showscores = true;
    input.time = 65.0;
    input.levelname = b"the Slipgate";
    let bar = status_bar(&input, 200);
    let c = chars(&bar);
    let text = |y: i32, x0: i32| -> String {
        let mut s: Vec<(i32, u8)> = c
            .iter()
            .filter(|&&(_, cy, _)| cy == y)
            .map(|&(x, _, ch)| (x, ch))
            .collect();
        s.sort_unstable();
        s.iter()
            .filter(|&&(x, _)| x >= x0)
            .map(|&(_, ch)| char::from(ch))
            .collect()
    };
    assert_eq!(text(4, 8), "Monsters:  3 / 10Time :  1:05");
    assert_eq!(text(12, 8), "Secrets :  1 /  2the Slipgate");
    assert!(
        c.contains(&(232 - 12 * 4, 12, b't')),
        "the level name centred on 232"
    );
    assert!(
        !bar.iter().any(|p| matches!(p.item, HudItem::Lmp { .. })),
        "no overlay in single player"
    );
    input.gametype = 1;
    input.maxclients = 2;
    input.viewentity = 2;
    let bar = status_bar(&input, 200);
    let lmp = bar
        .iter()
        .find(|p| matches!(p.item, HudItem::Lmp { .. }))
        .unwrap();
    assert_eq!(
        lmp,
        &Placed {
            anchor: Anchor::Screen,
            item: HudItem::Lmp {
                path: "gfx/ranking.lmp".to_owned(),
                x: None,
                y: 8
            }
        }
    );
    let centered: Vec<_> = bar
        .iter()
        .filter(|p| p.anchor == Anchor::Centered)
        .collect();
    assert!(
        matches!(
            centered[0].item,
            HudItem::Fill {
                x: 80,
                y: 40,
                w: 40,
                h: 4,
                color: 8
            }
        ),
        "you first: {:?}",
        centered[0]
    );
    assert!(
        matches!(
            centered[1].item,
            HudItem::Fill {
                x: 80,
                y: 44,
                w: 40,
                h: 4,
                color: 24
            }
        ),
        "{:?}",
        centered[1]
    );
    assert!(
        centered.iter().any(|p| matches!(
            p.item,
            HudItem::Char {
                x: 72,
                y: 40,
                ch: 12
            }
        )),
        "the local client, slot 1, is marked"
    );
    assert!(centered.iter().any(|p| matches!(
        p.item,
        HudItem::Char {
            x: 144,
            y: 40,
            ch: b'y'
        }
    )));
    assert!(
        centered.iter().any(|p| matches!(
            p.item,
            HudItem::Fill {
                x: 80,
                y: 50,
                w: 40,
                h: 4,
                color: 0x38
            }
        )),
        "me second, shirt 3"
    );
}

// spec HUD-2.3: the intermission overlay's pictures and numbers, the deathmatch overlay in
// deathmatch, the finale picture centred
#[test]
fn the_intermission_overlay() {
    let mut stats = [0; 32];
    stats[stat::MONSTERS] = 12;
    stats[stat::TOTALMONSTERS] = 100;
    stats[stat::SECRETS] = 1;
    stats[stat::TOTALSECRETS] = 3;
    let gettime = [0.0; 32];
    let scores = [Score {
        name: b"me".to_vec(),
        frags: 0,
        colors: 0,
    }];
    let mut input = hud_input(&stats, &gettime, &scores);
    input.intermission = 1;
    input.completed_time = 125.0;
    let overlay = intermission_overlay(&input);
    assert!(overlay.iter().all(|p| p.anchor == Anchor::Screen));
    let lmps: Vec<_> = overlay
        .iter()
        .filter_map(|p| match &p.item {
            HudItem::Lmp { path, x, y } => Some((path.clone(), *x, *y)),
            _ => None,
        })
        .collect();
    assert_eq!(
        lmps,
        [
            ("gfx/complete.lmp".to_owned(), Some(64), 24),
            ("gfx/inter.lmp".to_owned(), Some(0), 56)
        ]
    );
    let p = pics(&overlay);
    assert!(
        p.contains(&("num_2".to_owned(), 160 + 48, 64)),
        "2 minutes: {p:?}"
    );
    assert!(p.contains(&("num_colon".to_owned(), 234, 64)));
    assert!(
        p.contains(&("num_0".to_owned(), 246, 64)) && p.contains(&("num_5".to_owned(), 266, 64))
    );
    assert!(
        p.contains(&("num_1".to_owned(), 160 + 48, 104))
            && p.contains(&("num_slash".to_owned(), 232, 104))
            && p.contains(&("num_3".to_owned(), 240 + 48, 104))
    );
    assert!(
        p.contains(&("num_1".to_owned(), 160 + 24, 144))
            && p.contains(&("num_2".to_owned(), 160 + 48, 144))
    );
    assert!(
        p.contains(&("num_1".to_owned(), 240, 144)) && p.contains(&("num_0".to_owned(), 264, 144))
    );
    input.gametype = 1;
    let overlay = intermission_overlay(&input);
    assert!(matches!(&overlay[0].item, HudItem::Lmp { path, .. } if path == "gfx/ranking.lmp"));
    input.intermission = 2;
    let overlay = intermission_overlay(&input);
    assert_eq!(overlay.len(), 1);
    assert!(
        matches!(&overlay[0].item, HudItem::Lmp { path, x: None, y: 16 } if path == "gfx/finale.lmp")
    );
    assert_eq!(crate::color_for_map(0x90), 0x98);
}

// spec HUD-3.1: a centre print keeps at most 1023 bytes, resets its time and counts lines
// spec HUD-3.2: it is drawn while its time lasts or in an intermission, each line centred
// and cut at 40 characters, from 0.35 of the height for up to four lines and 48 for more
// spec HUD-3.3: in an intermission the characters appear at 8 a second, the first at once
#[test]
fn the_centre_print() {
    let mut center = CenterPrint::default();
    assert!(
        center.draw(0.1, 1.0, false, 640, 480).is_empty(),
        "nothing set"
    );
    center.set(b"hello\nworld", 1.0);
    assert_eq!(center.lines(), 2);
    let items = center.draw(0.1, 1.1, false, 640, 480);
    assert_eq!(items.len(), 10);
    assert_eq!(
        items[0],
        HudItem::Char {
            x: 300,
            y: 168,
            ch: b'h'
        }
    );
    assert_eq!(
        items[5],
        HudItem::Char {
            x: 300,
            y: 176,
            ch: b'w'
        }
    );
    center.draw(1.0, 2.1, false, 640, 480);
    assert!(
        center.draw(1.0, 3.1, false, 640, 480).is_empty(),
        "two seconds are up"
    );
    center.set(b"a\nb\nc\nd\ne", 3.1);
    assert_eq!(
        center.draw(0.0, 3.1, false, 640, 480)[0],
        HudItem::Char {
            x: 316,
            y: 48,
            ch: b'a'
        },
        "five lines start at 48"
    );
    let long = [b'x'; 50];
    center.set(&long, 4.0);
    assert_eq!(
        center.draw(0.0, 4.0, false, 640, 480).len(),
        40,
        "cut at 40"
    );
    let huge = vec![b'y'; 2000];
    center.set(&huge, 5.0);
    assert_eq!(center.draw(0.0, 5.0, false, 640, 480).len(), 40);
    center.set(b"the end", 10.0);
    assert_eq!(
        center.draw(0.0, 10.0, true, 320, 200).len(),
        1,
        "the first character at once"
    );
    assert_eq!(
        center.draw(0.0, 10.5, true, 320, 200).len(),
        5,
        "8 a second: 4 more"
    );
    assert_eq!(
        center.draw(5.0, 20.0, true, 320, 200).len(),
        7,
        "all of it, and the time no longer matters"
    );
}

// spec HUD-1.1, HUD-3.2: the client's hud: the bar, the overlay in an intermission and the
// centre print the messages set
#[test]
fn the_client_builds_its_hud() {
    let mut client = connected();
    client.set_screen(640, 480);
    let hud = client.hud();
    assert!(!hud.bar.is_empty() && hud.overlay.is_empty() && hud.center.is_empty());
    let mut m = msg();
    m.write_byte(26);
    m.write_string(b"Hi");
    frame(&mut client, 0.0, &[m.take()]);
    assert_eq!(client.hud().center.len(), 2);
    client.set_bar_lines(0);
    client.set_showscores(true);
    assert!(
        pics(&client.hud().bar)
            .iter()
            .any(|(n, _, _)| n == "scorebar")
    );
    let mut m = msg();
    m.write_byte(30);
    frame(&mut client, 0.0, &[m.take()]);
    let hud = client.hud();
    assert!(hud.bar.is_empty() && !hud.overlay.is_empty());
}

/// An `.mdl` with one vertex, one triangle, a 1 by 1 skin and one frame group of
/// `intervals.len()` poses ending at `intervals`.
fn group_mdl(intervals: &[f32]) -> Vec<u8> {
    let mut out = Vec::new();
    let int = |out: &mut Vec<u8>, v: i32| out.extend(v.to_le_bytes());
    let float = |out: &mut Vec<u8>, v: f32| out.extend(v.to_le_bytes());
    out.extend(MDL_MAGIC);
    int(&mut out, MDL_VERSION);
    for v in [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
        float(&mut out, v);
    }
    // One 4 by 4 skin (the width a multiple of 4), one vertex, one triangle, one frame.
    for v in [1, 4, 4, 1, 1, 1, 0, 0] {
        int(&mut out, v);
    }
    float(&mut out, 1.0);
    int(&mut out, 0);
    out.extend([0; 16]);
    for v in [0, 0, 0] {
        int(&mut out, v);
    }
    for v in [1, 0, 0, 0] {
        int(&mut out, v);
    }
    int(&mut out, 1);
    int(&mut out, i32::try_from(intervals.len()).unwrap());
    out.extend([0, 0, 0, 0, 255, 255, 255, 0]);
    for interval in intervals {
        float(&mut out, *interval);
    }
    for _ in intervals {
        out.extend([0, 0, 0, 0, 255, 255, 255, 0]);
        out.extend([b'f'; 16]);
        out.extend([1, 2, 3, 0]);
    }
    out
}

// spec REND-4.4: a frame group's interval is the group's first interval
#[test]
fn a_frame_groups_interval_is_its_first() {
    let bytes = group_mdl(&[0.1, 0.3, 0.6]);
    let mdl = Mdl::read(&bytes).unwrap();
    let info = ModelInfo::alias(&mdl);
    let ModelKind::Alias { frames } = info.kind else {
        panic!("an alias model");
    };
    assert_eq!(
        frames[0],
        FrameInfo {
            first_pose: 0,
            poses: 3,
            interval: 0.1,
        }
    );
}

// spec VIEW-3.5: the weapon's angles come from the view angles as the roll, the damage kick
// and the idle have left them, before the punch
#[test]
fn the_weapon_follows_the_kicked_view() {
    let mut view = ViewState::default();
    let settings = ViewSettings::default();
    let input = ViewInput {
        time: 1.0,
        frametime: 0.01,
        origin: [0.0; 3],
        entity_angles: [0.0; 3],
        viewangles: [5.0, 90.0, 0.0],
        velocity: [0.0; 3],
        viewheight: 22.0,
        punchangle: [0.0; 3],
        onground: true,
        health: 100,
        intermission: false,
        weapon: (3, 2),
    };
    // Hit from behind: a pitch kick.
    view.damage(1.0, 0, 20, [-100.0, 0.0, 0.0], [0.0; 3], [0.0, 0.0, 0.0]);
    let r = view.render(&input, &settings);
    assert_ne!(r.angles[0], 5.0, "the kick moved the view's pitch");
    assert_eq!(r.weapon.angles[0], -r.angles[0]);
    assert_eq!(r.weapon.angles[1], r.angles[1]);
}

// spec VIEW-4.8: the shift percents are ints: the damage's `3*count` and the fades truncate
// toward zero
#[test]
fn shift_percents_are_ints() {
    let mut view = ViewState::default();
    // blood 25: count 12.5, times 3 is 37.5, stored as 37.
    view.damage(1.0, 0, 25, [1.0, 0.0, 0.0], [0.0; 3], [0.0; 3]);
    assert_eq!(view.shifts[1].percent, 37);
    view.damage(1.0, 0, 100, [1.0, 0.0, 0.0], [0.0; 3], [0.0; 3]);
    assert_eq!(view.shifts[1].percent, 150);
    // At 72 Hz: 150 - 2.083 is 147.9, stored as 147.
    view.fade(1.0 / 72.0);
    assert_eq!(view.shifts[1].percent, 147);
    view.bonus_flash();
    view.fade(0.499);
    assert_eq!(view.shifts[2].percent, 0, "50 - 49.9 is 0.1, stored as 0");
}

// spec VIEW-2.1: the drift does not start on the frame the drift was stopped, from
// V_CalcRefdef's own start as well
#[test]
fn the_drift_waits_for_a_frame_after_a_stop() {
    let mut view = ViewState::default();
    let mut angles = [20.0, 0.0, 0.0];
    view.stop_pitch_drift(5.0);
    view.drift_pitch(5.0, 0.2, true, 200.0, 0.0, &mut angles);
    assert!(view.nodrift, "stopped at this very time");
    view.drift_pitch(5.1, 0.2, true, 200.0, 0.0, &mut angles);
    assert!(!view.nodrift);
}

// spec CLIENT-1.7: a new server prints the divider, then the level's name in the other colour
#[test]
fn a_new_server_prints_its_level_name() {
    let mut client = Client::with_limits(Game::Standard, ClientLimits::WINQUAKE);
    let packets = vec![serverinfo(&MODELS, &["a.wav", "b.wav"])];
    let events = client
        .frame(0.0, &packets, &mut TestModels, &mut Fixed(vec![]))
        .unwrap();
    let printed: Vec<&Vec<u8>> = events
        .iter()
        .filter_map(|event| match event {
            Event::Print(text) => Some(text),
            _ => None,
        })
        .collect();
    let divider = printed.iter().position(|text| text.starts_with(b"\n\n\x1d")).unwrap();
    assert_eq!(printed[divider].len(), 2 + 1 + 35 + 1 + 2);
    assert_eq!(printed[divider + 1][0], 2);
    assert!(printed[divider + 1].ends_with(b"\n"));
}
