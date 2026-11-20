// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or match is a failed test; the numbers are the spec's"
)]

use qs_sim::protocol::{EntAlpha, EntScale, Protocol, ProtocolFlags, ProtocolVersion};
use qs_sim::{Message, Overflow};

use crate::{
    Client, ClientLimits, Event, FrameInfo, Game, ModelInfo, ModelKind, Models, ParseError, Random,
    Reader, SoundEntity, SoundRef, stat,
};

struct NoRandom;

impl Random for NoRandom {
    fn rand(&mut self) -> i32 {
        0
    }
}

struct TestModels;

impl Models for TestModels {
    fn describe(&mut self, name: &str) -> Option<ModelInfo> {
        match name {
            "maps/test.bsp" => Some(ModelInfo::brush()),
            "progs/t.mdl" | "progs/u.mdl" => Some(ModelInfo {
                kind: ModelKind::Alias {
                    frames: vec![FrameInfo {
                        first_pose: 0,
                        poses: 1,
                        interval: 0.0,
                    }],
                },
                flags: 0,
                random_sync: false,
                nolerp: false,
            }),
            _ => None,
        }
    }
}

const THING: usize = 2;

fn msg(protocol: Protocol) -> Message {
    Message::new(65536, Overflow::Fatal).with_protocol(protocol)
}

/// A serverinfo naming `protocol`, the world and a thing, sounds `a.wav` and `b.wav`, then
/// `svc_setview` 1.
fn serverinfo(protocol: Protocol) -> Vec<u8> {
    let mut m = msg(protocol);
    m.write_byte(11);
    m.write_long(protocol.version().number());
    if protocol.version() == ProtocolVersion::Rmq {
        m.write_long(i32::from_le_bytes(protocol.flags().bits().to_le_bytes()));
    }
    m.write_byte(1);
    m.write_byte(0);
    m.write_string(b"The Test");
    for model in ["maps/test.bsp", "progs/t.mdl", "progs/u.mdl"] {
        m.write_string(model.as_bytes());
    }
    m.write_byte(0);
    for sound in ["a.wav", "b.wav"] {
        m.write_string(sound.as_bytes());
    }
    m.write_byte(0);
    m.write_byte(5);
    m.write_short(1);
    m.take()
}

/// A client connected in `protocol`, with `packets` after the serverinfo.
fn connected(protocol: Protocol, packets: &[Vec<u8>]) -> Result<Client, ParseError> {
    let mut client = Client::with_limits(Game::Standard, ClientLimits::default());
    let mut all = vec![serverinfo(protocol)];
    all.extend_from_slice(packets);
    client.frame(0.0, &all, &mut TestModels, &mut NoRandom)?;
    Ok(client)
}

/// A datagram at `time` with a minimal client data, then `updates`.
fn datagram(protocol: Protocol, time: f32, updates: &[Vec<u8>]) -> Vec<u8> {
    let mut m = msg(protocol);
    m.write_byte(7);
    m.write_float(time);
    m.write_byte(15);
    m.write_short(1 << 9 | 1 << 14);
    m.write_long(0);
    m.write_byte(0);
    m.write_short(100);
    for _ in 0..6 {
        m.write_byte(0);
    }
    for u in updates {
        m.write_bytes(u);
    }
    m.take()
}

/// `svc_spawnbaseline` for the thing, model 2 at (8, 0, 0).
fn thing_baseline(protocol: Protocol) -> Vec<u8> {
    let mut m = msg(protocol);
    m.write_byte(22);
    m.write_short(i32::try_from(THING).unwrap());
    for byte in [2, 0, 0, 0] {
        m.write_byte(byte);
    }
    for o in [8.0, 0.0, 0.0] {
        m.write_coord(o);
        m.write_angle(0.0);
    }
    m.take()
}

/// An update of the thing with `bits` (`U_SIGNAL` and the more bits added, and under 666 and
/// 999 the extend bits) and the bytes after its number.
fn thing_update(protocol: Protocol, bits: i32, tail: &[u8]) -> Vec<u8> {
    let fitz = !protocol.is_netquake();
    let mut bits = bits;
    if fitz && bits >= 1 << 16 {
        bits |= 1 << 15;
    }
    if bits >= 1 << 8 {
        bits |= 1;
    }
    let mut m = msg(protocol);
    m.write_byte(bits | 128);
    if bits & 1 != 0 {
        m.write_byte(bits >> 8);
    }
    if fitz && bits & (1 << 15) != 0 {
        m.write_byte(bits >> 16);
    }
    m.write_byte(i32::try_from(THING).unwrap());
    m.write_bytes(tail);
    m.take()
}

// spec CLIENT-9.1: coordinates and angles are read as the protocol's flags say
#[test]
fn the_reader_reads_each_protocol() {
    let mut m = msg(Protocol::RMQ);
    m.write_coord(-2.5);
    m.write_angle(90.0);
    m.write_angle16(-90.0);
    let bytes = m.take();
    let mut r = Reader::new(&bytes).with_protocol(Protocol::RMQ);
    assert_eq!(r.coord(), -2.5);
    assert_eq!(r.angle(), 90.0);
    assert_eq!(r.angle16(), -90.0);
    assert!(r.done());
    let float = Protocol::new(
        ProtocolVersion::Rmq,
        ProtocolFlags::FLOAT_COORD.with(ProtocolFlags::FLOAT_ANGLE),
    );
    let mut m = msg(float);
    m.write_coord(1.3);
    m.write_angle(7.7);
    m.write_angle16(8.8);
    let bytes = m.take();
    let mut r = Reader::new(&bytes).with_protocol(float);
    assert_eq!((r.coord(), r.angle(), r.angle16()), (1.3, 7.7, 8.8));
    let c24 = Protocol::new(ProtocolVersion::Rmq, ProtocolFlags::COORD_24);
    let mut r = Reader::new(&[2, 0, 51]).with_protocol(c24);
    assert_eq!(r.coord(), 2.2);
    let mut r = Reader::new(&[16, 0, 192]);
    assert_eq!((r.coord(), r.angle()), (2.0, -90.0), "id's by default");
    let mut r = Reader::new(&[16, 0, 192]).with_protocol(Protocol::FITZQUAKE);
    assert_eq!((r.coord(), r.angle()), (2.0, -90.0), "666 reads as id's");
}

// spec CLIENT-9.2: the serverinfo takes 15, 666 and 999, 999 with its flags, and the rest of the
// message is read in it; any other version is refused
#[test]
fn the_serverinfo_sets_the_protocol() {
    for protocol in [Protocol::NETQUAKE, Protocol::FITZQUAKE, Protocol::RMQ] {
        let mut baseline = thing_baseline(protocol);
        let mut packet = serverinfo(protocol);
        packet.append(&mut baseline);
        let mut client = Client::with_limits(Game::Standard, ClientLimits::default());
        client
            .frame(0.0, &[packet], &mut TestModels, &mut NoRandom)
            .unwrap();
        assert_eq!(client.entities()[THING].baseline.origin, [8.0, 0.0, 0.0]);
    }
    let mut m = msg(Protocol::NETQUAKE);
    m.write_byte(11);
    m.write_long(16);
    let mut client = Client::default();
    assert_eq!(
        client.frame(0.0, &[m.take()], &mut TestModels, &mut NoRandom),
        Err(ParseError::Version(16))
    );
    assert_eq!(
        ParseError::Version(16).to_string(),
        "Server returned version 16, not 15 or 666 or 999"
    );
    let mut version = msg(Protocol::NETQUAKE);
    version.write_byte(4);
    version.write_long(666);
    assert!(connected(Protocol::NETQUAKE, &[version.take()]).is_ok());
}

// spec CLIENT-9.3: under 666 the extend bytes, alpha, scale and the frame's and model's high
// bytes are read after the angles, the model taken after them
#[test]
fn fitzquake_updates_are_read() {
    let p = Protocol::FITZQUAKE;
    let bits = (1 << 6) | (1 << 10) | (1 << 16) | (1 << 20) | (1 << 17) | (1 << 18);
    // model 3, frame 44, then alpha 128, scale 32, frame high 1, model high 0.
    let update = thing_update(p, bits, &[3, 44, 128, 32, 1, 0]);
    let client = connected(p, &[thing_baseline(p), datagram(p, 0.1, &[update])]).unwrap();
    let thing = &client.entities()[THING];
    assert_eq!(thing.frame, 300);
    assert_eq!(thing.model, 3);
    assert_eq!(thing.alpha, EntAlpha::from_byte(128));
    assert_eq!(thing.scale, EntScale::from_byte(32));
    // Without the bits, the baseline's.
    let plain = thing_update(p, 0, &[]);
    let client = connected(p, &[thing_baseline(p), datagram(p, 0.1, &[plain])]).unwrap();
    let thing = &client.entities()[THING];
    assert_eq!(
        (thing.alpha, thing.scale),
        (EntAlpha::DEFAULT, EntScale::DEFAULT)
    );
}

// spec CLIENT-9.3: under 15 bit 15 is Nehahra's alpha: two floats, and a third when the first
// is 2
#[test]
fn nehahra_alpha_is_read_under_15() {
    let p = Protocol::NETQUAKE;
    let floats = |values: &[f32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let one = thing_update(p, 1 << 15, &floats(&[1.0, 0.5]));
    let client = connected(p, &[thing_baseline(p), datagram(p, 0.1, &[one])]).unwrap();
    assert_eq!(client.entities()[THING].alpha, EntAlpha::encode(0.5));
    let two = thing_update(p, 1 << 15, &floats(&[2.0, 0.25, 9.0]));
    let client = connected(p, &[thing_baseline(p), datagram(p, 0.1, &[two])]).unwrap();
    assert_eq!(client.entities()[THING].alpha, EntAlpha::encode(0.25));
}

// spec CLIENT-9.4: svc_spawnbaseline2 and svc_spawnstatic2 read their bits, a large model and
// frame as shorts, and the alpha and scale last
#[test]
fn second_baselines_and_statics() {
    let p = Protocol::FITZQUAKE;
    let mut m = msg(p);
    m.write_byte(42);
    m.write_short(i32::try_from(THING).unwrap());
    m.write_byte(0b1110);
    m.write_byte(2);
    m.write_short(300);
    m.write_byte(0);
    m.write_byte(0);
    for _ in 0..3 {
        m.write_coord(0.0);
        m.write_angle(0.0);
    }
    m.write_byte(64);
    m.write_byte(8);
    m.write_byte(43);
    m.write_byte(1 << 2);
    m.write_byte(3);
    m.write_byte(1);
    m.write_byte(0);
    m.write_byte(0);
    for _ in 0..3 {
        m.write_coord(4.0);
        m.write_angle(0.0);
    }
    m.write_byte(200);
    let client = connected(p, &[m.take()]).unwrap();
    let baseline = client.entities()[THING].baseline;
    assert_eq!((baseline.modelindex, baseline.frame), (2, 300));
    assert_eq!(baseline.alpha, EntAlpha::from_byte(64));
    assert_eq!(baseline.scale, EntScale::from_byte(8));
    let fixed = client.statics()[0];
    assert_eq!((fixed.model, fixed.frame), (3, 1));
    assert_eq!(fixed.alpha, EntAlpha::from_byte(200));
    assert_eq!(fixed.scale, EntScale::DEFAULT);
    assert_eq!(fixed.origin, [4.0; 3]);
    // A large model past the client's models is refused.
    let mut m = msg(p);
    m.write_byte(42);
    m.write_short(2);
    m.write_byte(1);
    m.write_short(5000);
    assert!(matches!(
        connected(p, &[m.take()]),
        Err(ParseError::BadModel(5000))
    ));
}

// spec CLIENT-9.5: U_LERPFINISH sets lerpfinish to msgtime plus its byte over 255; an update
// without it clears LERP_FINISH
#[test]
fn lerpfinish_is_read() {
    let p = Protocol::FITZQUAKE;
    let finish = thing_update(p, 1 << 19, &[51]);
    let mut client = connected(p, &[thing_baseline(p), datagram(p, 1.0, &[finish])]).unwrap();
    let at = client.entities()[THING].lerpflags.finish.unwrap();
    assert!((at - 1.2).abs() < 1e-6, "{at}");
    client
        .frame(
            0.1,
            &[datagram(p, 1.1, &[thing_update(p, 0, &[])])],
            &mut TestModels,
            &mut NoRandom,
        )
        .unwrap();
    assert_eq!(client.entities()[THING].lerpflags.finish, None);
}

// spec CLIENT-9.6: svc_clientdata's extend byte and the high bytes after the weapon, then the
// weapon's alpha
#[test]
fn client_data_high_bytes() {
    let p = Protocol::FITZQUAKE;
    let mut m = msg(p);
    m.write_byte(15);
    let bits = (1 << 9) | (1 << 14) | (1 << 13) | (1 << 15) | (1 << 20) | (1 << 17);
    m.write_short(bits);
    m.write_byte(bits >> 16);
    m.write_long(0);
    m.write_byte(44);
    m.write_byte(2);
    m.write_short(100);
    for value in [0, 0, 44, 0, 0, 1] {
        m.write_byte(value);
    }
    m.write_byte(1);
    m.write_byte(1);
    let client = connected(p, &[m.take()]).unwrap();
    assert_eq!(client.stats()[stat::ARMOR], 300);
    assert_eq!(client.stats()[stat::NAILS], 300);
    assert_eq!(client.stats()[stat::ACTIVEWEAPON], 1);
}

// spec CLIENT-9.7: SND_LARGEENTITY and SND_LARGESOUND read shorts; a sound past MAX_SOUNDS is
// refused; svc_spawnstaticsound2's sound is a short
#[test]
fn large_sounds_are_read() {
    let p = Protocol::FITZQUAKE;
    let mut m = msg(p);
    m.write_byte(6);
    m.write_byte((1 << 3) | (1 << 4));
    m.write_short(500);
    m.write_byte(6);
    m.write_short(2);
    for c in [1.0, 2.0, 3.0] {
        m.write_coord(c);
    }
    let mut client = connected(p, &[]).unwrap();
    let events = client
        .frame(0.0, &[m.take()], &mut TestModels, &mut NoRandom)
        .unwrap();
    let Some(Event::Sound(sound)) = events.first() else {
        panic!("{events:?}");
    };
    assert_eq!((sound.entity, sound.channel), (SoundEntity::new(500), 6));
    assert_eq!(sound.sound, SoundRef::Index(2));
    let mut m = msg(p);
    m.write_byte(6);
    m.write_byte(1 << 4);
    m.write_short(8);
    m.write_short(256);
    assert_eq!(
        client.frame(0.0, &[m.take()], &mut TestModels, &mut NoRandom),
        Err(ParseError::SoundNumber(256))
    );
    let mut m = msg(p);
    m.write_byte(44);
    for c in [1.0, 2.0, 3.0] {
        m.write_coord(c);
    }
    m.write_short(2);
    m.write_byte(255);
    m.write_byte(64);
    let mut client = connected(p, &[]).unwrap();
    let events = client
        .frame(0.0, &[m.take()], &mut TestModels, &mut NoRandom)
        .unwrap();
    assert!(
        matches!(events.first(), Some(Event::StaticSound { sound: 2, .. })),
        "{events:?}"
    );
}
