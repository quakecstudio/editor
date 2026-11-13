// SPDX-License-Identifier: GPL-3.0-or-later

//! The stairs `qs_map::shapes` makes, compiled by `qs-map-bsp` and walked up by a player in
//! `qs-sim` (P6-18, MAP-10.10): the default step is one the player climbs.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, panic or index is a failed test"
)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use qc_codegen::compile_module;
use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_progs::Progs;
use qc_sema::{Db, FileId, ModuleInput};
use qc_types::Etype;
use qs_bsp::{Bsp, BspLoader};
use qs_map::shapes::{Facing, SHAPE_STEP_DEPTH, SHAPE_STEP_HEIGHT, Shape};
use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_sim::{LoadOptions, UserCmd, World};

/// The server's frame, 1/72 of a second.
pub(crate) const FRAME: f64 = 1.0 / 72.0;
/// The stairs' box: six steps of the default size.
const STAIRS: [[f64; 3]; 2] = [[0.0, 0.0, 0.0], [192.0, 64.0, 96.0]];
/// How high the player's eye is above its origin, and its origin above the floor.
pub(crate) const EYE: f32 = 22.0;
const STANDING: f32 = 24.0;

/// A player that walks, put at the player start.
const PLAYER: &str = "\
void(entity e, vector o) setorigin = #2;
void(entity e, vector min, vector max) setsize = #4;
entity(entity start, .string fld, string match) find = #18;
string(string s) precache_model = #20;
void() main = {};
void() StartFrame = {};
void() PlayerPreThink = {};
void() PlayerPostThink = {};
void() ClientKill = {};
void() ClientConnect = {};
void() ClientDisconnect = {};
void() SetNewParms = {};
void() SetChangeParms = {};
void() worldspawn = { precache_model(\"progs/player.mdl\"); };
void() info_player_start = {};
void() PutClientInServer = {
    local entity spot;
    spot = find(world, classname, \"info_player_start\");
    self.health = 100;
    self.solid = 3;
    self.movetype = 3;
    self.view_ofs = '0 0 22';
    setorigin(self, spot.origin);
    setsize(self, '-16 -16 -24', '16 16 32');
};
";

/// The type `defs.qc` declares a system variable with.
fn type_name(etype: Etype) -> &'static str {
    match etype {
        Etype::String => "string",
        Etype::Float => "float",
        Etype::Vector => "vector",
        Etype::Entity => "entity",
        Etype::Function => "void()",
        other => panic!("no system variable has type {other:?}"),
    }
}

/// The progs of [`PLAYER`] after id's system definitions.
pub(crate) fn progs() -> Vec<u8> {
    let mut defs = String::new();
    for var in SYSTEM_GLOBALS {
        writeln!(defs, "{} {};", type_name(var.etype), var.name).unwrap();
    }
    defs.push_str("void end_sys_globals;\n");
    for var in SYSTEM_FIELDS {
        writeln!(defs, ".{} {};", type_name(var.etype), var.name).unwrap();
    }
    defs.push_str("void end_sys_fields;\n");
    let files = [("defs.qc", defs), ("player.qc", PLAYER.to_owned())];
    let mut db = Db::new();
    let mut input = ModuleInput::default();
    for (i, (name, text)) in files.iter().enumerate() {
        db.set_file(
            FileId::new(i),
            PathBuf::from(name),
            text.as_bytes().to_vec(),
        );
        input.compiled.push(FileId::new(i));
    }
    let names: Vec<String> = files.iter().map(|(n, _)| (*n).to_owned()).collect();
    let compiled = compile_module(&mut db, &input, |id| names[id.index()].clone()).unwrap();
    qc_progs::write(&compiled.progs).unwrap()
}

/// A floor, the stairs climbing east and a landing at their top, the player start before them.
fn map() -> Vec<u8> {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"info_player_start\"\n\"origin\" \"-96 32 24\"\n}\n",
    )
    .unwrap();
    let surface = Surface {
        texture: b"floor".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    let mut brushes = vec![
        Brush::cuboid([-256.0, -64.0, -16.0], [640.0, 128.0, 0.0], &surface).unwrap(),
        Brush::cuboid([192.0, 0.0, 0.0], [640.0, 64.0, 96.0], &surface).unwrap(),
    ];
    let [low, high] = STAIRS;
    brushes.extend(
        Shape::Stairs {
            height: SHAPE_STEP_HEIGHT,
            depth: SHAPE_STEP_DEPTH,
            facing: Facing::East,
        }
        .brushes(low, high, &surface)
        .unwrap(),
    );
    file.entities[0]
        .items
        .extend(brushes.into_iter().map(Item::Brush));
    file.to_bytes()
}

// spec MAP-10.10: a player walking east climbs the default stairs to their top
#[test]
fn a_player_walks_up_the_default_stairs() {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&map(), None)
        .unwrap();
    let bytes: &'static [u8] = Box::leak(compiled.bsp.into_boxed_slice());
    let bsp = Bsp::read(bytes, BspLoader::default()).unwrap();
    let progs_bytes = progs();
    let progs = Progs::read(&progs_bytes).unwrap();
    let mut world = World::load(
        &progs,
        "stairs",
        &bsp,
        Rc::new(|_: &str| None),
        LoadOptions::default(),
    )
    .unwrap();
    world.settle(&progs).unwrap();
    world.connect(&progs).unwrap();
    world.frame(&progs, FRAME).unwrap();
    let (eye, _) = world.player_view().unwrap();
    assert!(
        (eye[2] - (STANDING + EYE)).abs() < 1.0,
        "starts on the floor: {eye:?}"
    );
    world.set_command(UserCmd {
        angles: [0.0; 3],
        forward: 320.0,
        ..UserCmd::default()
    });
    // At 320 units a second the player is past the stairs, on the landing, within two seconds;
    // the landing runs to x = 640.
    let top = 96.0 + STANDING + EYE;
    let mut landed = None;
    for _ in 0..144 {
        world.frame(&progs, FRAME).unwrap();
        let (eye, _) = world.player_view().unwrap();
        if eye[0] > 240.0 && eye[0] < 600.0 {
            landed = Some(eye);
            break;
        }
    }
    let eye = landed.unwrap_or_else(|| panic!("never reached the landing"));
    assert!((eye[2] - top).abs() < 1.0, "stands on the landing: {eye:?}");
}
