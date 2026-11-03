// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use crate::LoadOptions;
use crate::world::tests::player_with_options;
use qc_builtins::localization::Localization;
use qc_progs::Progs;
use qc_vm::convert::{double_to_float, float_to_int};

use super::{Message, NET_MAX_DATAGRAM, NET_MAX_MSGLEN, Vis};
use crate::UserCmd;
use crate::fixtures::{CONTENTS_EMPTY, CONTENTS_SOLID, MapSpec};
use crate::world::tests::{PLAYER_SIZE, Running, entity, ledge, ledge_over, moved, player, run_on};

/// Reads a message back, as the client will.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn byte(&mut self) -> u8 {
        let b = self.bytes[self.at];
        self.at += 1;
        b
    }

    fn char(&mut self) -> i8 {
        i8::from_le_bytes([self.byte()])
    }

    fn short(&mut self) -> i16 {
        i16::from_le_bytes([self.byte(), self.byte()])
    }

    fn long(&mut self) -> i32 {
        i32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()])
    }

    fn float(&mut self) -> f32 {
        f32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()])
    }

    fn string(&mut self) -> String {
        let end = self.bytes[self.at..].iter().position(|&b| b == 0).unwrap();
        let text = String::from_utf8_lossy(&self.bytes[self.at..self.at + end]).into_owned();
        self.at += end + 1;
        text
    }

    fn coord(&mut self) -> f32 {
        f32::from(self.short()) / 8.0
    }

    fn angle(&mut self) -> f32 {
        f32::from(self.char()) * (360.0 / 256.0)
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }

    fn rest(&self) -> &'a [u8] {
        &self.bytes[self.at..]
    }
}

/// A connected player on `spec` with the map's `entities`, as [`player`] but with entities.
fn player_with(spec: &MapSpec, more: &str, entities: &str) -> Running {
    let source = format!(
        "{more}void() PutClientInServer = {{ self.health = 100; self.solid = 3; \
         self.movetype = 3; self.view_ofs = '0 0 22'; {PLAYER_SIZE} }};\n"
    );
    let mut run = run_on(spec, &source, entities);
    run.settle();
    run.connect();
    run
}

impl Running {
    fn settle(&mut self) {
        let progs = Progs::read(&self.progs).unwrap();
        self.world.settle(&progs).unwrap();
    }

    fn packets(&mut self) -> Vec<Vec<u8>> {
        self.world.take_messages().packets
    }
}

/// The datagram's entity updates after the time and the client data, as `(number, bits)`
/// with the fields read past, and what follows them.
fn skip_clientdata(r: &mut Reader<'_>) {
    assert_eq!(r.byte(), 15, "svc_clientdata");
    let bits = r.short();
    for bit in [0, 1] {
        if bits & (1 << bit) != 0 {
            r.char();
        }
    }
    for i in 0..3 {
        if bits & (1 << (2 + i)) != 0 {
            r.char();
        }
        if bits & (1 << (5 + i)) != 0 {
            r.char();
        }
    }
    r.long();
    for bit in [12, 13, 14] {
        if bits & (1 << bit) != 0 {
            r.byte();
        }
    }
    r.short();
    for _ in 0..5 {
        r.byte();
    }
    r.byte();
}

/// One entity update: its number, bits and the bytes of its fields.
fn update(r: &mut Reader<'_>) -> (usize, u16, Vec<u8>) {
    let first = r.byte();
    assert!(first & 128 != 0, "an update starts with U_SIGNAL");
    let mut bits = u16::from(first & 127);
    if bits & 1 != 0 {
        bits |= u16::from(r.byte()) << 8;
    }
    let number = if bits & (1 << 14) != 0 {
        usize::try_from(r.short()).unwrap()
    } else {
        usize::from(r.byte())
    };
    let start = r.at;
    for bit in [10, 6, 11, 12, 13] {
        if bits & (1 << bit) != 0 {
            r.byte();
        }
    }
    for (i, angle_bit) in [8, 4, 9].into_iter().enumerate() {
        if bits & (1 << (1 + i)) != 0 {
            r.short();
        }
        if bits & (1 << angle_bit) != 0 {
            r.byte();
        }
    }
    (number, bits, r.bytes[start..r.at].to_vec())
}

/// One entity update as read back: its number, bits and field bytes.
type Update = (usize, u16, Vec<u8>);

/// The datagram of a frame: `svc_time`'s value, the updates and the bytes after them.
fn datagram(bytes: &[u8]) -> (f32, Vec<Update>, Vec<u8>) {
    let mut r = Reader::new(bytes);
    assert_eq!(r.byte(), 7, "svc_time");
    let time = r.float();
    skip_clientdata(&mut r);
    let mut updates = Vec::new();
    while !r.done() && r.bytes[r.at] & 128 != 0 {
        updates.push(update(&mut r));
    }
    (time, updates, r.rest().to_vec())
}

// spec NET-1.1: a byte is the low 8 bits, a short the low 16 little-endian, a long all 32, a
// float its bytes, a string its text and a NUL, a coord (int) (f * 8) and an angle
// ((int) f * 256 / 360) & 255
#[test]
fn the_writers_encode_as_id_does() {
    let mut m = Message::new(64, false);
    m.write_byte(0x1ff);
    m.write_byte(-1);
    m.write_short(-2);
    m.write_short(0x12345);
    m.write_long(-3);
    m.write_float(1.5);
    m.write_string(b"hi\0cut");
    m.write_string(b"");
    m.write_coord(1.5);
    m.write_coord(-0.9);
    m.write_angle(-90.0);
    m.write_angle(359.9);
    m.write_angle(360.0);
    assert_eq!(
        m.bytes(),
        [
            0xff, 0xff, 0xfe, 0xff, 0x45, 0x23, 0xfd, 0xff, 0xff, 0xff, 0, 0, 0xc0, 0x3f, b'h',
            b'i', 0, 0, 12, 0, 0xf9, 0xff, 192, 255, 0
        ]
    );
    assert_eq!(m.len(), 25);
    assert_eq!(m.max(), 64);
}

// spec NET-1.2: a buffer that may not overflow drops a write that does not fit; a client's
// message empties itself, marks the overflow and takes the write, unless the write alone is
// too big
#[test]
fn buffers_hold_their_size() {
    let mut strict = Message::new(4, false);
    strict.write_long(1);
    strict.write_byte(2);
    assert_eq!(strict.len(), 4);
    assert!(!strict.overflowed());
    let mut loose = Message::new(4, true);
    loose.write_long(1);
    loose.write_short(2);
    assert_eq!(loose.bytes(), [2, 0]);
    assert!(loose.overflowed());
    loose.write_bytes(&[0; 5]);
    assert_eq!(
        loose.bytes(),
        [2, 0],
        "a write larger than the buffer is dropped"
    );
    loose.clear();
    assert!(loose.is_empty());
    assert_eq!(Message::default().max(), 0);
}

/// A player on the ledge whose `StartFrame` runs `each` and `PlayerPreThink` runs `pre`.
fn framed(each: &str, pre: &str) -> Running {
    let more = format!("void() StartFrame = {{ {each} }};\n");
    player(&ledge(), "", pre, &more)
}

// spec NET-1.2: the datagram is cleared each frame before the moves and the physics
// spec NET-1.5: a datagram is svc_time, the client data, the entities and then sv.datagram
// spec NET-1.3: a spawned client is sent its datagram, then its reliable message when it
// holds anything
#[test]
fn a_frame_sends_the_datagram_then_the_reliable_message() {
    let mut run = framed("particle('1 2 3', '0 0 1', 7, 2); bprint(\"hello\");", "");
    run.packets();
    run.frames(1, 0.1).unwrap();
    let packets = run.packets();
    assert_eq!(packets.len(), 2, "{packets:?}");
    let (time, updates, rest) = datagram(&packets[0]);
    assert_eq!(time, double_to_float(run.world.time()));
    assert_eq!(updates.iter().map(|u| u.0).collect::<Vec<_>>(), [1]);
    // svc_particle, the origin as coords, the direction times 16, the count and colour.
    assert_eq!(rest, [18, 8, 0, 16, 0, 24, 0, 0, 0, 16, 2, 7]);
    let mut r = Reader::new(&packets[1]);
    assert_eq!(r.byte(), 8, "svc_print");
    assert_eq!(r.string(), "hello");
    assert!(r.done());
    run.frames(1, 0.1).unwrap();
    let packets = run.packets();
    let (_, _, rest) = datagram(&packets[0]);
    assert_eq!(rest.len(), 12, "the last frame's particle is gone");
}

// spec RRE-1.9: with a localization loaded, bprint's text is its localized format with the
// placeholders filled, and WriteString writes its string localized
// spec RRE-1.6: with none, the text and the string are written as they are; the file is the
// host's from before the map, so a spawn function's write is localized too
#[test]
fn printed_and_written_strings_are_localized_with_a_file_loaded() {
    let each = "bprint(\"$qc_hi\"); WriteString(2, \"$qc_hi\");";
    let more = format!("void() StartFrame = {{ {each} }};\n");
    let read = |run: &mut Running| {
        run.packets();
        run.frames(1, 0.1).unwrap();
        let packets = run.packets();
        let mut r = Reader::new(&packets[1]);
        assert_eq!(r.byte(), 8, "svc_print");
        let printed = r.string();
        (printed, r.string())
    };
    let mut plain = player_with_options(&ledge(), "", "", &more, LoadOptions::default());
    assert_eq!(read(&mut plain), ("$qc_hi".to_owned(), "$qc_hi".to_owned()));
    let options = LoadOptions {
        localization: Localization::loc_load_file(b"qc_hi = \"Hi {}!\"\n"),
        ..LoadOptions::default()
    };
    let mut localized = player_with_options(&ledge(), "", "", &more, options);
    assert_eq!(
        read(&mut localized),
        ("Hi !".to_owned(), "Hi {}!".to_owned())
    );
}

// spec NET-1.4: a change of frags sends svc_updatefrags to every active client, once, and
// the reliable datagram is appended to the client's message and cleared
#[test]
fn frags_and_the_reliable_datagram_reach_the_client() {
    let mut run = framed("WriteByte(2, 77); WriteShort(2, 300);", "self.frags = 3;");
    run.packets();
    run.frames(1, 0.1).unwrap();
    let packets = run.packets();
    let mut r = Reader::new(&packets[1]);
    assert_eq!(r.byte(), 14, "svc_updatefrags");
    assert_eq!(r.byte(), 0);
    assert_eq!(r.short(), 3);
    assert_eq!(r.byte(), 77);
    assert_eq!(r.short(), 300);
    assert!(r.done());
    run.frames(1, 0.1).unwrap();
    let packets = run.packets();
    assert_eq!(packets[1], [77, 44, 1], "the frags did not change again");
}

// spec NET-1.3: after the messages are sent every edict loses EF_MUZZLEFLASH, so the flash
// is sent once
#[test]
fn muzzle_flashes_are_sent_once() {
    let source = "void() gun = { self.effects = 2; };\n";
    let mut run = player(&ledge(), "self.effects = 2 + 4;", "", source);
    run.packets();
    run.frames(1, 0.1).unwrap();
    let (_, updates, _) = datagram(&run.packets()[0]);
    let (number, bits, fields) = &updates[0];
    assert_eq!(*number, 1);
    assert!(bits & (1 << 13) != 0, "U_EFFECTS");
    let before = [10, 6, 11, 12]
        .iter()
        .filter(|&&b| bits & (1 << b) != 0)
        .count();
    assert_eq!(fields[before], 6);
    assert_eq!(run.world.float(1, "effects"), 4.0);
}

// spec NET-2.1: after the settle frames every client slot and every edict with a model has
// a baseline in the signon buffer: the client's colormap is its number and its model the
// player's, another's its own model
// spec NET-2.3: prespawn sends the signon buffer, then signon 2
#[test]
fn baselines_describe_the_spawn_state() {
    let source = "void() worldspawn = { precache_model(\"progs/t.mdl\"); };\n\
        void() thing = { setmodel(self, \"progs/t.mdl\"); self.frame = 3; self.skin = 1; \
        self.angles = '0 90 0'; };\n\
        void() bare = { };\n";
    let entities = [entity("thing", "10 20 30", ""), entity("bare", "0 0 0", "")].concat();
    let mut run = run_on(&ledge(), source, &entities);
    run.settle();
    run.connect();
    let packets = run.packets();
    assert_eq!(
        packets.len(),
        3,
        "the serverinfo, the prespawn, the spawn and begin"
    );
    let all = packets.concat();
    let mut r = Reader::new(&all);
    // Past the serverinfo (NET-2.2) to the signon buffer.
    while r.byte() != 25 {}
    assert_eq!(r.byte(), 1, "signon 1");
    let mut baselines = Vec::new();
    while r.bytes[r.at] == 22 {
        r.byte();
        let number = r.short();
        let fields = [r.byte(), r.byte(), r.byte(), r.byte()];
        let mut origin = [0.0; 3];
        let mut angles = [0.0; 3];
        for i in 0..3 {
            origin[i] = r.coord();
            angles[i] = r.angle();
        }
        baselines.push((number, fields, origin, angles));
    }
    assert_eq!(r.byte(), 25);
    assert_eq!(r.byte(), 2, "signon 2 after the signon buffer");
    // The models: the world is 1, the player's 2 and t.mdl 3.
    assert_eq!(
        baselines,
        [
            (0, [1, 0, 0, 0], [0.0; 3], [0.0; 3]),
            (1, [2, 0, 1, 0], [0.0; 3], [0.0; 3]),
            (2, [3, 3, 0, 1], [10.0, 20.0, 30.0], [0.0, 90.0, 0.0]),
        ],
        "the world, the client and the thing; the bare entity has no baseline"
    );
}

// spec NET-2.2: the serverinfo: the version print, the protocol, maxclients, the game type,
// the world's message, the model and sound names, the cd track, the view entity, signon 1
#[test]
fn the_serverinfo_names_the_game() {
    let source = "void() worldspawn = { precache_sound(\"misc/a.wav\"); \
        self.message = \"Hello\"; self.sounds = 4; };\n";
    let mut run = run_on(&ledge(), source, "");
    run.connect();
    let packets = run.packets();
    let mut r = Reader::new(&packets[0]);
    assert_eq!(r.byte(), 8);
    let print = r.string();
    assert!(
        print.starts_with("\x02\nVERSION 1.09 SERVER ("),
        "{print:?}"
    );
    assert!(print.ends_with(" CRC)"));
    assert_eq!(r.byte(), 11, "svc_serverinfo");
    assert_eq!(r.long(), 15);
    assert_eq!(r.byte(), 1, "maxclients");
    assert_eq!(r.byte(), 0, "GAME_COOP");
    assert_eq!(r.string(), "Hello");
    assert_eq!(r.string(), "maps/test.bsp");
    assert_eq!(r.string(), "progs/player.mdl");
    assert_eq!(r.byte(), 0, "the end of the models");
    assert_eq!(r.string(), "misc/a.wav");
    assert_eq!(r.byte(), 0, "the end of the sounds");
    assert_eq!(r.byte(), 32, "svc_cdtrack");
    assert_eq!((r.byte(), r.byte()), (4, 4));
    assert_eq!(r.byte(), 5, "svc_setview");
    assert_eq!(r.short(), 1);
    assert_eq!(r.byte(), 25);
    assert_eq!(r.byte(), 1);
}

// spec NET-2.4: the spawn message: the time, each slot's name, frags and colours, the 64
// light styles, the four stats, the client's setangle, the client data, signon 3
// spec NET-2.5: datagrams start with the frame after the begin
#[test]
fn the_spawn_message_lists_the_client_state() {
    let more = "void() worldspawn = { lightstyle(0, \"m\"); lightstyle(63, \"az\"); \
        total_monsters = 5; found_secrets = 2; };\n";
    let mut run = player(&ledge(), "self.angles = '10 20 30';", "", more);
    let packets = run.packets();
    let _ = Progs::read(&run.progs).unwrap();
    assert_eq!(packets.len(), 3, "no datagram before the first frame");
    let all = packets.concat();
    let mut r = Reader::new(&all);
    while r.byte() != 25 {}
    r.byte();
    while r.byte() != 25 {}
    assert_eq!(r.byte(), 2);
    assert_eq!(r.byte(), 7, "svc_time");
    assert_eq!(r.float(), double_to_float(run.world.time()));
    assert_eq!((r.byte(), r.byte(), r.string().as_str()), (13, 0, "player"));
    assert_eq!((r.byte(), r.byte(), r.short()), (14, 0, 0));
    assert_eq!((r.byte(), r.byte(), r.byte()), (17, 0, 0));
    for i in 0..64_u8 {
        assert_eq!((r.byte(), r.byte()), (12, i));
        let style = r.string();
        match i {
            0 => assert_eq!(style, "m"),
            63 => assert_eq!(style, "az"),
            _ => assert_eq!(style, ""),
        }
    }
    for (stat, value) in [(11, 0), (12, 5), (13, 2), (14, 0)] {
        assert_eq!((r.byte(), r.byte(), r.long()), (3, stat, value));
    }
    assert_eq!(r.byte(), 10, "svc_setangle");
    assert_eq!([r.angle(), r.angle(), r.angle()], [9.84375, 19.6875, 0.0]);
    skip_clientdata(&mut r);
    assert_eq!((r.byte(), r.byte()), (25, 3));
    assert!(r.done());
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.packets().len(), 1, "a datagram, and nothing reliable");
}

// spec NET-3.1: the fat PVS is the OR of the PVS of every non-solid leaf within 8 units of
// the point: on one side only that side's leaf, within 8 of the plane both
#[test]
fn the_fat_pvs_reaches_the_leafs_near_the_eye() {
    // One plane at x = 0: front is leaf 1, back is leaf 2; leaf 0 is solid.
    let vis = Vis {
        nodes: vec![(0, [-2, -3])],
        planes: vec![([1.0, 0.0, 0.0], 0.0)],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY, CONTENTS_EMPTY],
        pvs: vec![vec![0xff], vec![0b01], vec![0b10]],
        row: 1,
    };
    assert_eq!(vis.fat_pvs([20.0, 0.0, 0.0]), [0b01]);
    assert_eq!(vis.fat_pvs([-20.0, 0.0, 0.0]), [0b10]);
    assert_eq!(vis.fat_pvs([5.0, 0.0, 0.0]), [0b11]);
    assert_eq!(vis.fat_pvs([-8.0, 0.0, 0.0]), [0b11], "exactly 8 is both");
    let solid = Vis {
        nodes: vec![(0, [-1, -3])],
        ..vis.clone()
    };
    assert_eq!(
        solid.fat_pvs([20.0, 0.0, 0.0]),
        [0],
        "a solid leaf adds nothing"
    );
    assert_eq!(Vis::default().fat_pvs([0.0; 3]), Vec::<u8>::new());
}

// spec NET-3.2: the client is always sent; an entity with no model, or one not linked into
// any leaf, is not; fewer than 16 bytes left end the entities
#[test]
fn entities_are_sent_when_they_are_visible() {
    let source = "void() worldspawn = { precache_model(\"progs/t.mdl\"); };\n\
        void() thing = { setmodel(self, \"progs/t.mdl\"); };\n\
        void() bare = { };\n\
        void() ghost = { self.modelindex = 2; self.model = \"progs/t.mdl\"; };\n";
    let entities = [
        entity("bare", "0 0 0", ""),
        entity("thing", "10 20 30", ""),
        entity("ghost", "0 0 0", ""),
    ]
    .concat();
    let mut run = player_with(&ledge(), source, &entities);
    run.packets();
    run.frames(1, 0.1).unwrap();
    let (_, updates, _) = datagram(&run.packets()[0]);
    assert_eq!(
        updates.iter().map(|u| u.0).collect::<Vec<_>>(),
        [1, 3],
        "the client and the thing; not the bare one nor the unlinked ghost"
    );
    // Many moving things: 300 updates of 8 bytes do not fit the datagram.
    let many = "void() worldspawn = { precache_model(\"progs/t.mdl\"); local float i; i = 0; \
        while (i < 300) { local entity e; e = spawn(); setmodel(e, \"progs/t.mdl\"); \
        setorigin(e, '0 0 0' + i * '1 0 0'); e.movetype = 8; e.velocity = '100 100 100'; \
        i = i + 1; } };\n";
    let mut crowded = player(&ledge(), "", "", many);
    crowded.packets();
    crowded.frames(1, 0.1).unwrap();
    let packets = crowded.packets();
    let (_, updates, rest) = datagram(&packets[0]);
    assert!(
        updates.len() < 300 && updates.len() > 100,
        "{}",
        updates.len()
    );
    assert!(packets[0].len() > NET_MAX_DATAGRAM - 16 - 16);
    assert!(rest.is_empty());
    assert_eq!(updates[0].0, 1, "the client first");
}

// spec NET-3.3: the bits are set for an origin more than 0.1 from the baseline, an angle
// that differs, MOVETYPE_STEP, and each field that differs; the number is a short past 255
// and the high bits need U_MOREBITS
#[test]
fn update_bits_follow_the_baseline() {
    let source = "void() worldspawn = { precache_model(\"progs/t.mdl\"); \
        precache_model(\"progs/u.mdl\"); };\n\
        void() thing = { setmodel(self, \"progs/t.mdl\"); self.frame = 3; };\n\
        void() nudge = { self.origin_x = self.origin_x + 0.05; self.nextthink = time + 0.1; \
        self.think = nudge; };\n\
        void() change = { self.frame = 4; self.skin = 2; self.colormap = 1; \
        self.effects = 8; setmodel(self, \"progs/u.mdl\"); self.angles = '0 45 0'; \
        self.movetype = 4; self.origin = '20 0 0'; };\n";
    let entities = [
        // The thinks run in the first frame after the settle frames and the baselines.
        entity(
            "thing",
            "0 0 0",
            "\"think\" \"nudge\"\n\"nextthink\" \"1.25\"\n",
        ),
        entity(
            "thing",
            "0 0 0",
            "\"think\" \"change\"\n\"nextthink\" \"1.25\"\n",
        ),
    ]
    .concat();
    let mut run = player_with(&ledge(), source, &entities);
    run.packets();
    run.frames(1, 0.1).unwrap();
    let (_, updates, _) = datagram(&run.packets()[0]);
    let nudged = &updates[1];
    assert_eq!(nudged.0, 2);
    assert_eq!(nudged.1, 0, "within 0.1 of the baseline on every count");
    let changed = &updates[2];
    assert_eq!(changed.0, 3);
    let bits = changed.1;
    for (bit, name) in [
        (0, "U_MOREBITS"),
        (1, "U_ORIGIN1"),
        (4, "U_ANGLE2"),
        (5, "U_NOLERP"),
        (6, "U_FRAME"),
        (10, "U_MODEL"),
        (11, "U_COLORMAP"),
        (12, "U_SKIN"),
        (13, "U_EFFECTS"),
    ] {
        assert!(bits & (1 << bit) != 0, "{name} in {bits:#x}");
    }
    for bit in [2, 3, 8, 9, 14] {
        assert!(bits & (1 << bit) == 0, "bit {bit} clear in {bits:#x}");
    }
    // modelindex 4 (u.mdl after the world, the player's and t), frame, colormap, skin,
    // effects, then the x coord 20 and the yaw 45.
    assert_eq!(changed.2, [4, 4, 1, 2, 8, 160, 0, 32]);
    // An entity numbered past 255 is a short with U_LONGENTITY and U_MOREBITS.
    let far = "void() worldspawn = { precache_model(\"progs/t.mdl\"); local float i; i = 0; \
        while (i < 255) { spawn(); i = i + 1; } };\n\
        void() thing = { setmodel(self, \"progs/t.mdl\"); };\n";
    let mut run = player_with(&ledge(), far, &entity("thing", "0 0 0", ""));
    run.packets();
    run.frames(1, 0.1).unwrap();
    let (_, updates, _) = datagram(&run.packets()[0]);
    let last = updates.last().unwrap();
    assert!(last.0 >= 256, "{}", last.0);
    assert!(last.1 & (1 << 14) != 0 && last.1 & 1 != 0);
}

// spec NET-4.1: damage taken sends svc_damage with the inflictor's centre and clears it
// spec NET-4.3: a fixangle sends svc_setangle once
// spec NET-4.4: the bits and the data of svc_clientdata
// spec NET-4.5: the items long carries items2 when the progs defines it, else serverflags
#[test]
fn the_client_data_carries_the_player() {
    let more = "void() worldspawn = { precache_model(\"progs/v_shot.mdl\"); serverflags = 3; };\n";
    let pre = "self.dmg_take = 7; self.dmg_save = 2; \
        self.dmg_inflictor = world; self.view_ofs = '0 0 30'; self.idealpitch = -5; \
        self.punchangle = '1 0 -2'; self.velocity = '32 -16 0'; self.weaponframe = 4; \
        self.armorvalue = 50; self.weaponmodel = \"progs/v_shot.mdl\"; self.health = 77; \
        self.currentammo = 9; self.ammo_shells = 1; self.ammo_nails = 2; \
        self.ammo_rockets = 3; self.ammo_cells = 4; self.weapon = 8; self.items = 5; \
        self.waterlevel = 2; self.fixangle = 1; self.angles = '0 90 0';";
    let mut run = player(&ledge(), "self.movetype = 8;", pre, more);
    run.packets();
    run.frames(1, 0.1).unwrap();
    let packets = run.packets();
    let mut r = Reader::new(&packets[0]);
    assert_eq!(r.byte(), 7);
    r.float();
    assert_eq!(r.byte(), 19, "svc_damage");
    assert_eq!((r.byte(), r.byte()), (2, 7));
    assert_eq!([r.coord(), r.coord(), r.coord()], [0.0; 3]);
    assert_eq!(r.byte(), 10, "svc_setangle");
    assert_eq!([r.angle(), r.angle(), r.angle()], [0.0, 90.0, 0.0]);
    assert_eq!(r.byte(), 15, "svc_clientdata");
    let bits = r.short();
    let want = 1 | 2 | 4 | 16 | 32 | 64 | 512 | 2048 | 4096 | 8192 | 16384;
    assert_eq!(bits, want, "{bits:#x}");
    assert_eq!(r.char(), 30, "viewheight");
    assert_eq!(r.char(), -5, "idealpitch");
    assert_eq!(r.char(), 1, "punch x");
    assert_eq!(r.char(), 2, "velocity x / 16");
    assert_eq!(r.char(), -1, "velocity y / 16");
    assert_eq!(r.char(), -2, "punch z");
    assert_eq!(r.long(), 5 | (3 << 28), "items with serverflags");
    assert_eq!(r.byte(), 4, "weaponframe");
    assert_eq!(r.byte(), 50, "armor");
    assert_eq!(r.byte(), 3, "the weapon model's index");
    assert_eq!(r.short(), 77);
    assert_eq!(
        [r.byte(), r.byte(), r.byte(), r.byte(), r.byte()],
        [9, 1, 2, 3, 4]
    );
    assert_eq!(r.byte(), 8, "the weapon in standard Quake");
    assert_eq!(run.world.float(1, "dmg_take"), 0.0);
    assert_eq!(run.world.float(1, "fixangle"), 0.0);
    // Rogue and Hipnotic send the weapon's bit; items2 replaces the serverflags.
    let more2 = format!(".float items2;\n{more}void() PlayerPostThink = {{ self.items2 = 6; }};\n");
    let mut run = player(&ledge(), "self.movetype = 8;", pre, &more2);
    run.world.set_standard_quake(false);
    run.packets();
    run.frames(1, 0.1).unwrap();
    let packets = run.packets();
    let mut r = Reader::new(&packets[0]);
    r.byte();
    r.float();
    while r.byte() != 15 {}
    r.short();
    for _ in 0..6 {
        r.char();
    }
    assert_eq!(r.long(), 5 | (6 << 23), "items with items2");
    r.byte();
    r.byte();
    r.byte();
    r.short();
    for _ in 0..5 {
        r.byte();
    }
    assert_eq!(r.byte(), 3, "weapon 8 is bit 3");
}

// spec NET-4.2: on flat ground the ideal pitch is 0; near a dropoff, or with a single step,
// it is left as it is
#[test]
fn the_ideal_pitch_follows_the_floor() {
    let put = "self.idealpitch = 5; self.angles = '0 0 0'; setorigin(self, '0 0 24');";
    let mut run = player(&ledge(), put, "", "");
    run.world.set_command(UserCmd::default());
    run.frames(2, 0.1).unwrap();
    assert_eq!(run.world.float(1, "idealpitch"), 0.0, "flat ground");
    let put_deep = "self.idealpitch = 5; self.angles = '0 0 0'; setorigin(self, '80 0 24');";
    let mut deep = player(&ledge_over(200.0), put_deep, "", "");
    let w = moved(&mut deep, UserCmd::default(), 2, 0.1);
    assert_eq!(
        w.float(1, "idealpitch"),
        5.0,
        "near a dropoff nothing changes"
    );
    let put_edge = "self.idealpitch = 5; self.angles = '0 0 0'; setorigin(self, '20 0 24');";
    let mut edge = player(&ledge(), put_edge, "", "");
    let w = moved(&mut edge, UserCmd::default(), 2, 0.1);
    assert!(
        float_to_int(w.float(1, "flags")) & 512 != 0,
        "on the ground"
    );
    assert_eq!(w.float(1, "idealpitch"), 5.0, "one step down is not enough");
    let mut airborne = player(&ledge(), "self.idealpitch = 5; self.movetype = 8;", "", "");
    let w = moved(&mut airborne, UserCmd::default(), 1, 0.1);
    assert_eq!(w.float(1, "idealpitch"), 5.0, "not on the ground");
}

// spec NET-5.1: the bytes the builtins write beside the mock's counts: sound, particle and
// a broadcast write to the datagram; ambientsound, makestatic and an init write to the
// signon; lightstyle, bprint, sprint, centerprint, stuffcmd and MSG_ONE writes to the
// client's message; an MSG_ALL write to the reliable datagram
// spec BUILTIN-12.3: svc_sound's flags, volume, attenuation, channel and centre
// spec BUILTIN-12.4: svc_spawnstaticsound's coords, index, volume and attenuation
// spec BUILTIN-12.8: svc_spawnstatic's model index, frame, colormap, skin, origin and angles
// spec BUILTIN-13.2: each Write builtin's encoding
#[test]
fn the_builtins_bytes_match_the_mocks_counts() {
    let more = format!(
        "void() worldspawn = {{ precache_model(\"progs/t.mdl\"); precache_sound(\"a.wav\"); \
         precache_sound(\"b.wav\"); ambientsound('1 2 3', \"b.wav\", 0.5, 2); \
         lightstyle(2, \"abc\"); }};\n\
         void() statue = {{ setmodel(self, \"progs/t.mdl\"); self.frame = 2; self.skin = 1; \
         self.angles = '0 180 0'; makestatic(self); }};\n\
         void() StartFrame = {{ sound(world, 3, \"a.wav\", 0.5, 2); \
         sound(world, 0, \"b.wav\", 1, 1); particle('0 0 0', '10 -10 0', 73, 5); \
         WriteByte(0, 200); WriteByte(2, 9); WriteByte(3, 1); lightstyle(1, \"z\"); \
         bprint(\"bc\"); }};\n\
         void() PlayerPreThink = {{ sprint(self, \"st\"); centerprint(self, \"c\"); \
         stuffcmd(self, \"cmd\\n\"); msg_entity = self; WriteByte(1, 7); \
         WriteChar(1, -1); WriteShort(1, 300); WriteLong(1, 70000); WriteCoord(1, 1.5); \
         WriteAngle(1, -90); WriteString(1, \"hi\"); WriteEntity(1, self); {PLAYER_SIZE} }};\n"
    );
    let mut run = run_on(&ledge(), &more, &entity("statue", "4 8 12", ""));
    {
        let m = run.world.messages();
        let e = run.world.engine();
        assert_eq!(m.signon.len(), e.buffers.signon);
        let mut r = Reader::new(m.signon.bytes());
        assert_eq!(r.byte(), 29, "svc_spawnstaticsound");
        assert_eq!([r.coord(), r.coord(), r.coord()], [1.0, 2.0, 3.0]);
        assert_eq!([r.byte(), r.byte(), r.byte()], [2, 127, 128]);
        assert_eq!(r.byte(), 20, "svc_spawnstatic");
        // The world, the player's model and then t.mdl: index 3.
        assert_eq!([r.byte(), r.byte(), r.byte(), r.byte()], [3, 2, 0, 1]);
        assert_eq!(r.coord(), 4.0);
        assert_eq!(r.angle(), 0.0);
        assert_eq!(r.coord(), 8.0);
        assert_eq!(r.angle(), -180.0, "byte 128 read back as a char");
        assert_eq!(r.coord(), 12.0);
        assert_eq!(r.angle(), 0.0);
        assert!(r.done());
        assert!(
            m.client.message.is_empty(),
            "lightstyle while loading writes to no client"
        );
    }
    run.connect();
    run.packets();
    run.frames(1, 0.1).unwrap();
    let m = run.world.messages();
    let e = run.world.engine();
    // The signon holds the baselines too (NET-2.1), 16 bytes each, which the mock has no
    // count for.
    let baselines = m.signon.bytes().iter().filter(|&&b| b == 22).count();
    assert_eq!(baselines, 2, "the world and the client");
    assert_eq!(m.signon.len(), e.buffers.signon + 16 * baselines);
    assert_eq!(m.datagram.len(), e.buffers.unreliable);
    assert_eq!(
        e.buffers.reliable, 0,
        "the MSG_ALL byte the mock counted went out with the reliable datagram (NET-1.4)"
    );
    let packets = m.outbox.clone();
    let (_, _, rest) = datagram(&packets[0]);
    let mut r = Reader::new(&rest);
    assert_eq!(r.byte(), 6, "svc_sound");
    assert_eq!(r.byte(), 3, "volume and attenuation flags");
    assert_eq!(r.byte(), 127);
    assert_eq!(r.byte(), 128);
    assert_eq!(r.short(), 3, "(world << 3) | channel 3");
    assert_eq!(r.byte(), 1, "a.wav");
    assert_eq!(
        [r.coord(), r.coord(), r.coord()],
        [0.0; 3],
        "the world has no mins or maxs, so its centre is its origin"
    );
    assert_eq!(r.byte(), 6);
    assert_eq!(r.byte(), 0, "no flags at full volume and attenuation 1");
    assert_eq!(r.short(), 0);
    assert_eq!(r.byte(), 2);
    for _ in 0..3 {
        r.coord();
    }
    assert_eq!(r.byte(), 18, "svc_particle");
    assert_eq!([r.coord(), r.coord(), r.coord()], [0.0; 3]);
    assert_eq!([r.char(), r.char(), r.char()], [127, -128, 0]);
    assert_eq!([r.byte(), r.byte()], [5, 73]);
    assert_eq!(r.byte(), 200, "WriteByte to MSG_BROADCAST");
    assert!(r.done());
    let mut r = Reader::new(&packets[1]);
    assert_eq!(r.byte(), 12, "svc_lightstyle");
    assert_eq!(r.byte(), 1);
    assert_eq!(r.string(), "z");
    assert_eq!(r.byte(), 8);
    assert_eq!(r.string(), "bc");
    assert_eq!(r.byte(), 8, "sprint");
    assert_eq!(r.string(), "st");
    assert_eq!(r.byte(), 26, "centerprint");
    assert_eq!(r.string(), "c");
    assert_eq!(r.byte(), 9, "stuffcmd");
    assert_eq!(r.string(), "cmd\n");
    assert_eq!(r.byte(), 7);
    assert_eq!(r.char(), -1);
    assert_eq!(r.short(), 300);
    assert_eq!(r.long(), 70000);
    assert_eq!(r.coord(), 1.5);
    assert_eq!(r.angle(), -90.0);
    assert_eq!(r.string(), "hi");
    assert_eq!(r.short(), 1, "WriteEntity");
    assert_eq!(r.byte(), 9, "the MSG_ALL write, appended last");
    assert!(r.done());
    assert_eq!(m.client.message.max(), NET_MAX_MSGLEN);
}

// spec NET-5.1: the mock's byte counts empty with the sim's buffers each frame, so a sound
// is written on every frame of a long game, not only while the mock's count is under its
// limit
#[test]
fn the_mocks_counts_follow_the_buffers_every_frame() {
    let more = "void() worldspawn = { precache_sound(\"a.wav\"); };\n";
    let mut run = player(&ledge(), "", "sound(self, 3, \"a.wav\", 1, 1);", more);
    run.packets();
    for _ in 0..3 {
        run.frames(1, 0.1).unwrap();
        let m = run.world.messages();
        let e = run.world.engine();
        assert_eq!(m.datagram.len(), e.buffers.unreliable);
        assert_eq!(e.buffers.reliable, 0);
        let (_, _, rest) = datagram(&run.packets()[0]);
        assert_eq!(rest.first(), Some(&6), "svc_sound every frame");
    }
}

// spec NET-2.4: the spawn message starts with a clear, so what PutClientInServer wrote to
// the client is dropped
#[test]
fn the_spawn_message_drops_what_put_client_in_server_wrote() {
    let mut run = player(&ledge(), "sprint(self, \"hello\");", "", "");
    let packets = run.packets();
    assert_eq!(packets.len(), 3);
    assert_eq!(packets[2][0], 7, "svc_time first");
    let all = packets.concat();
    assert!(
        !all.windows(5).any(|w| w == b"hello"),
        "the sprint was cleared"
    );
}

// spec NET-1.3: an overflowed message is cleared and its flag reset, so the client's later
// messages go out
#[test]
fn an_overflowed_message_starts_over() {
    let mut message = Message::new(4, true);
    message.write_bytes(&[1, 2, 3]);
    message.write_bytes(&[4, 5]);
    assert!(message.overflowed());
    assert_eq!(
        message.bytes(),
        [4, 5],
        "emptied, then the write that overflowed"
    );
    message.clear();
    assert!(message.overflowed(), "SZ_Clear keeps the flag");
    message.reset_overflow();
    assert!(!message.overflowed());
    message.write_byte(9);
    assert_eq!(message.bytes(), [9]);
}

// spec NET-1.3: SV_CleanupEnts stores `(int)effects & ~EF_MUZZLEFLASH` for every edict, so a
// fractional effects is truncated whether or not it flashes
#[test]
fn cleanup_truncates_every_edicts_effects() {
    let mut run = player(&ledge(), "self.effects = 7.5;", "", "");
    run.packets();
    run.frames(1, 0.1).unwrap();
    assert_eq!(
        run.world.float(1, "effects"),
        5.0,
        "7.5 as an int is 7, less the flash"
    );
}

/// The rerelease's builtins, as its progs gets them once patched (RRE-4.1).
const RERELEASE_BUILTINS: &str = "float() finaleFinished = #79;\n\
    void(entity e, string s) localsound = #80;\n\
    void() draw_point = #81;\n\
    void() draw_cylinder = #89;\n\
    float() CheckPlayerEXFlags = #90;\n\
    float() walkpathtogoal = #91;\n\
    void() bot_movetopoint = #92;\n\
    void() past = #93;\n";

/// A player whose progs has the rerelease's builtins, in the rerelease's table or not, and
/// precaches `misc/a.wav`; `PlayerPreThink` runs `pre`.
fn rerelease_player(pre: &str, rerelease_builtins: bool) -> Running {
    let more = format!(
        "{RERELEASE_BUILTINS}void() worldspawn = {{ precache_sound(\"misc/b.wav\"); \
         precache_sound(\"misc/a.wav\"); }};\n"
    );
    let options = LoadOptions {
        rerelease_builtins,
        ..LoadOptions::default()
    };
    player_with_options(&ledge(), "", pre, &more, options)
}

// spec RRE-2.1: localsound writes svc_localsound, a field byte and the sample's precache
// index, to the client; a non-client or a sample not precached sends nothing
#[test]
fn localsound_sends_the_sound_to_the_client() {
    let pre = "localsound(world, \"misc/a.wav\"); localsound(self, \"misc/nope.wav\"); \
        localsound(self, \"misc/a.wav\");";
    let mut run = rerelease_player(pre, true);
    run.packets();
    run.frames(1, 0.1).unwrap();
    let packets = run.packets();
    assert_eq!(packets[1], [56, 0, 2]);
}

// spec RRE-2.1: a client message with fewer than 4 bytes free drops the sound
#[test]
fn localsound_is_dropped_from_a_full_message() {
    let mut run = rerelease_player("localsound(self, \"misc/a.wav\");", true);
    run.packets();
    let message = &mut run.world.builtins.messages.client.message;
    let room = message.max() - message.len();
    for _ in 0..room - 3 {
        message.write_byte(1);
    }
    let full = message.len();
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.packets()[1].len(), full, "3 bytes free: dropped");
    let mut run = rerelease_player("localsound(self, \"misc/a.wav\");", true);
    run.packets();
    let message = &mut run.world.builtins.messages.client.message;
    let room = message.max() - message.len();
    for _ in 0..room - 4 {
        message.write_byte(1);
    }
    run.frames(1, 0.1).unwrap();
    assert!(
        run.packets()[1].ends_with(&[56, 0, 2]),
        "4 bytes free: sent"
    );
}

// spec RRE-4.2: in the rerelease's table finaleFinished, CheckPlayerEXFlags and
// walkpathtogoal return 0, and 81 to 89 and 92 are the run error unimplemented builtin
#[test]
fn the_rereleases_table_runs_to_92() {
    let pre = "self.frags = 5 + finaleFinished() + CheckPlayerEXFlags() + walkpathtogoal();";
    let mut run = rerelease_player(pre, true);
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.world.float(1, "frags"), 5.0);
    for call in ["draw_point();", "draw_cylinder();", "bot_movetopoint();"] {
        let mut run = rerelease_player(call, true);
        let error = run.frames(1, 0.1).unwrap_err().to_string();
        assert!(error.contains("unimplemented builtin"), "{call} {error}");
    }
}

// spec RRE-4.2: past 92 the rerelease's table has no builtin, and without it 79 is past id's
#[test]
fn the_rereleases_table_ends_at_92() {
    let mut run = rerelease_player("past();", true);
    let past = run.frames(1, 0.1).unwrap_err().to_string();
    assert!(!past.contains("unimplemented builtin"), "{past}");
    let mut run = rerelease_player("finaleFinished();", false);
    let id = run.frames(1, 0.1).unwrap_err().to_string();
    assert!(!id.contains("unimplemented builtin"), "{id}");
    assert_eq!(past.replace("93", "79"), id);
}
