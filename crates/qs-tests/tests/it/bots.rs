// SPDX-License-Identifier: GPL-3.0-or-later

//! Bots driven in `qs-sim` on a room compiled by `qs-map-bsp` (L-87): the host's minds of
//! `qs-bots` set each bot's move before each server frame, and the room's own QuakeC plays it.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, panic or index is a failed test"
)]

use std::rc::Rc;

use qc_progs::Progs;
use qs_bots::Bots;
use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_sim::scenario::Driver;
use qs_sim::spawn::Deathmatch;
use qs_sim::{ClientId, LoadOptions, MaxPlayers, QcValue, World};

use crate::shape_stairs::{FRAME, progs_of};

/// A player who takes a rocket launcher by touching it, killed once a second has passed when
/// it is the bot `Ash`, and put back in the game by `PlayerDeathThink`'s rule: every button
/// released, then one pressed.
const GAME: &str = "\
void(entity e, vector o) setorigin = #2;
void(entity e, string m) setmodel = #3;
void(entity e, vector min, vector max) setsize = #4;
entity(entity start, .string fld, string match) find = #18;
string(string s) precache_model = #20;
float pickups; float killed; float respawns; float shots;
void() main = {};
void() PlayerPostThink = {
    if (self.button0 && !killed && self.netname == \"Ash\")
        shots = shots + 1;
};
void() ClientKill = {};
void() ClientConnect = {};
void() ClientDisconnect = {};
void() SetNewParms = {};
void() SetChangeParms = {};
void() worldspawn = { precache_model(\"progs/player.mdl\"); };
void() info_player_start = {};
void() info_player_deathmatch = {};
void() rocket_touch = {
    if (other.classname != \"player\")
        return;
    other.items = other.items | 32;
    other.ammo_rockets = 10;
    self.solid = 0;
    self.modelindex = 0;
    pickups = pickups + 1;
};
void() weapon_rocketlauncher = {
    self.solid = 1;
    self.touch = rocket_touch;
    setmodel(self, \"progs/player.mdl\");
    setsize(self, '-16 -16 0', '16 16 56');
    setorigin(self, self.origin);
};
void() PutClientInServer = {
    local entity spot;
    if (self.colormap == 1)
        spot = find(world, classname, \"info_player_start\");
    else
        spot = find(world, classname, \"info_player_deathmatch\");
    self.classname = \"player\";
    self.health = 100;
    self.deadflag = 0;
    self.takedamage = 2;
    self.flags = 8;
    self.solid = 3;
    self.movetype = 3;
    self.items = 4097;
    self.weapon = 1;
    self.ammo_shells = 25;
    self.view_ofs = '0 0 22';
    setmodel(self, \"progs/player.mdl\");
    setsize(self, '-16 -16 -24', '16 16 32');
    setorigin(self, spot.origin);
};
void() PlayerPreThink = {
    if (self.deadflag == 2) {
        if (!self.button0 && !self.button2)
            self.deadflag = 3;
        return;
    }
    if (self.deadflag == 3) {
        if (self.button0) {
            respawns = respawns + 1;
            PutClientInServer();
        }
        return;
    }
};
void() StartFrame = {
    local entity bot;
    if (killed || time < 1)
        return;
    bot = find(world, netname, \"Ash\");
    if (bot.health > 0 && bot.items & 32) {
        bot.health = -10;
        bot.deadflag = 2;
        bot.movetype = 0;
        killed = 1;
    }
};
";

fn rock() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    }
}

/// A sealed room 1024 by 256, the player start at its west end, the bots' start at its east
/// end and a rocket launcher in its middle.
fn room() -> Vec<u8> {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"info_player_start\"\n\"origin\" \"-400 0 24\"\n}\n\
          {\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"400 0 24\"\n}\n\
          {\n\"classname\" \"weapon_rocketlauncher\"\n\"origin\" \"0 64 0\"\n}\n",
    )
    .unwrap();
    let rock = rock();
    let cuboid = |min: [f64; 3], max: [f64; 3]| Brush::cuboid(min, max, &rock).unwrap();
    let brushes = [
        cuboid([-528.0, -144.0, -16.0], [528.0, 144.0, 0.0]),
        cuboid([-528.0, -144.0, 256.0], [528.0, 144.0, 272.0]),
        cuboid([-528.0, -144.0, 0.0], [-512.0, 144.0, 256.0]),
        cuboid([512.0, -144.0, 0.0], [528.0, 144.0, 256.0]),
        cuboid([-512.0, -144.0, 0.0], [512.0, -128.0, 256.0]),
        cuboid([-512.0, 128.0, 0.0], [512.0, 144.0, 256.0]),
    ];
    file.entities[0]
        .items
        .extend(brushes.into_iter().map(Item::Brush));
    file.to_bytes()
}

fn global(world: &World, progs: &Progs<'_>, name: &str) -> f32 {
    match world
        .named_globals(progs)
        .into_iter()
        .find(|g| g.name == name)
        .map(|g| g.value)
    {
        Some(QcValue::Float(value)) => value,
        other => panic!("{name} is {other:?}"),
    }
}

/// The room's world with two slots at `deathmatch`, the player in it and a bot brought in by
/// `addbot 2`, played for six seconds, the bots driven before each frame.
fn played(progs: &Progs<'_>, deathmatch: &str) -> (World, Bots) {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&room(), None)
        .unwrap();
    let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).unwrap();
    let options = LoadOptions {
        maxplayers: MaxPlayers::new(2),
        settings: vec![("deathmatch".to_owned(), deathmatch.to_owned())],
        ..LoadOptions::default()
    };
    let mut world = World::load(progs, "room", &bsp, Rc::new(|_: &str| None), options).unwrap();
    world.settle(progs).unwrap();
    world.connect(progs).unwrap();
    let graph = Bots::graph_of(&compiled.bsp, Deathmatch::On).unwrap();
    let mut bots = Bots::new(Some(graph));
    assert!(bots.is_empty());
    world.console("addbot 2\n");
    for _ in 0..(72 * 6) {
        // A scenario's driver is the same drive.
        Driver::before_frame(&mut bots, &mut world, progs, FRAME).unwrap();
        world.frame(progs, FRAME).unwrap();
        drop(world.take_messages(ClientId::FIRST));
    }
    (world, bots)
}

// spec BOT-9.1, BOT-3.3, BOT-1.6: before each frame the host gives the bot addbot brought in its
// mind, whose moves take it to the rocket launcher it needs, firing at the player it sees down
// the room; killed, it presses fire in turn until the mod's own death rule puts it back
#[test]
fn a_bot_takes_a_weapon_and_respawns() {
    let progs_bytes = progs_of(GAME);
    let progs = Progs::read(&progs_bytes).unwrap();
    let (mut world, mut bots) = played(&progs, "1");
    assert!(
        global(&world, &progs, "shots") > 0.0,
        "in deathmatch the bot shot at the player it saw"
    );
    assert_eq!(bots.len(), 1, "the bot has a mind");
    assert!(!bots.is_empty());
    assert_eq!(
        bots.skill(ClientId::new(1).unwrap()),
        Some(qs_sim::BotSkill::Hard),
        "addbot 2"
    );
    assert_eq!(
        global(&world, &progs, "pickups"),
        1.0,
        "the bot took the weapon"
    );
    assert_eq!(global(&world, &progs, "killed"), 1.0);
    assert_eq!(
        global(&world, &progs, "respawns"),
        1.0,
        "and was put back in the game"
    );
    let progs_again = Progs::read(&progs_bytes).unwrap();
    world.console("removebot\n");
    world.frame(&progs_again, FRAME).unwrap();
    bots.drive(&mut world, &progs_again, FRAME).unwrap();
    assert!(bots.is_empty(), "the dropped bot's mind is gone");
    world.console("addbot\nkick Ash\n");
    bots.drive(&mut world, &progs_again, FRAME).unwrap();
    world.frame(&progs_again, FRAME).unwrap();
    bots.drive(&mut world, &progs_again, FRAME).unwrap();
    assert!(bots.is_empty(), "a bot kicked has no mind");
}

// spec BOT-3.1, BOT-9.1: out of deathmatch the player is a friend, never shot at, and the bot
// still takes the weapon it needs
#[test]
fn a_bot_out_of_deathmatch_shoots_no_player() {
    let progs_bytes = progs_of(GAME);
    let progs = Progs::read(&progs_bytes).unwrap();
    let (world, _) = played(&progs, "0");
    assert_eq!(global(&world, &progs, "shots"), 0.0);
    assert_eq!(global(&world, &progs, "pickups"), 1.0);
}
