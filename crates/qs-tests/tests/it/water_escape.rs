// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/v101qc/client.qc, CheckWaterJump, PlayerJump, PlayerPreThink

//! How high above a liquid's surface the edge of a pool may be for a swimming player to
//! climb out (L-74): id's `CheckWaterJump` and the swimming half of `PlayerJump`, run in
//! `qs-sim` on pools compiled by `qs-map-bsp` with their rims at each height. The generator
//! keeps every rim at or under [`qs_map_gen::CLIMB_OUT`].

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp_const,
    reason = "test code: a failed unwrap or index is a failed test, and the rims are whole"
)]

use std::path::PathBuf;
use std::rc::Rc;

use qc_codegen::compile_module;
use qc_progs::Progs;
use qc_sema::{Db, FileId, ModuleInput};
use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_sim::{LoadOptions, UserCmd, World};

use crate::shape_stairs::{EYE, FRAME, system_defs};

/// How deep the pool is under its rim, and how far it reaches.
pub(crate) const DEPTH: f64 = 128.0;
pub(crate) const POOL: [f64; 2] = [256.0, 128.0];
/// How long a swimmer has to get out: ten seconds of frames.
const FRAMES: usize = 720;
/// The heights of rim above the surface tried, every 2 units.
const RIMS: std::ops::RangeInclusive<u32> = 0..=24;
const RIM_STEP: f64 = 2.0;

/// A player who swims and climbs out of water as id's `client.qc` has it, put at the player
/// start. `PlayerJump`'s swimming sound is left out: it changes nothing the test measures.
const SWIMMER: &str = "\
void(vector ang) makevectors = #1;
void(entity e, vector o) setorigin = #2;
void(entity e, vector min, vector max) setsize = #4;
vector(vector v) normalize = #9;
void(vector v1, vector v2, float nomonsters, entity forent) traceline = #16;
entity(entity start, .string fld, string match) find = #18;
string(string s) precache_model = #20;
float TRUE = 1;
float FL_ONGROUND = 512;
float FL_WATERJUMP = 2048;
float FL_JUMPRELEASED = 4096;
float CONTENT_WATER = -3;
float CONTENT_SLIME = -4;
void() main = {};
void() StartFrame = {};
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
void() PlayerJump =
{
	if (self.flags & FL_WATERJUMP)
		return;
	if (self.waterlevel >= 2)
	{
		if (self.watertype == CONTENT_WATER)
			self.velocity_z = 100;
		else if (self.watertype == CONTENT_SLIME)
			self.velocity_z = 80;
		else
			self.velocity_z = 50;
		return;
	}
	if (!(self.flags & FL_ONGROUND))
		return;
	if ( !(self.flags & FL_JUMPRELEASED) )
		return;
	self.flags = self.flags - (self.flags & FL_JUMPRELEASED);
	self.flags = self.flags - FL_ONGROUND;
	self.button2 = 0;
	self.velocity_z = self.velocity_z + 270;
};
void() CheckWaterJump =
{
	local vector start, end;
	makevectors (self.angles);
	start = self.origin;
	start_z = start_z + 8;
	v_forward_z = 0;
	normalize(v_forward);
	end = start + v_forward*24;
	traceline (start, end, TRUE, self);
	if (trace_fraction < 1)
	{
		start_z = start_z + self.maxs_z - 8;
		end = start + v_forward*24;
		self.movedir = trace_plane_normal * -50;
		traceline (start, end, TRUE, self);
		if (trace_fraction == 1)
		{
			self.flags = self.flags | FL_WATERJUMP;
			self.velocity_z = 225;
			self.flags = self.flags - (self.flags & FL_JUMPRELEASED);
			self.teleport_time = time + 2;
			return;
		}
	}
};
void() PlayerPreThink =
{
	if (self.waterlevel == 2)
		CheckWaterJump ();
	if (self.button2)
		PlayerJump ();
	else
		self.flags = self.flags | FL_JUMPRELEASED;
};
";

/// The progs of [`SWIMMER`] after id's system definitions.
pub(crate) fn progs() -> Vec<u8> {
    let files = [
        ("defs.qc", system_defs()),
        ("player.qc", SWIMMER.to_owned()),
    ];
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

pub(crate) fn surface(texture: &str) -> Surface {
    Surface {
        texture: texture.as_bytes().to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    }
}

/// A pool `DEPTH` deep, its liquid of `liquid` reaching to `rim` under the ground around it,
/// the player start on its bottom in its middle.
pub(crate) fn pool(liquid: &str, rim: f64) -> Vec<u8> {
    let [px, py] = POOL;
    let start = format!("{} 0 {}", px / 2.0, -DEPTH + 25.0);
    let mut file = MapFile::parse(
        format!(
            "{{\n\"classname\" \"worldspawn\"\n}}\n\
             {{\n\"classname\" \"info_player_start\"\n\"origin\" \"{start}\"\n}}\n"
        )
        .as_bytes(),
    )
    .unwrap();
    let rock = surface("rock");
    let reach = px + px;
    let cuboid = |min: [f64; 3], max: [f64; 3]| Brush::cuboid(min, max, &rock).unwrap();
    let brushes = vec![
        cuboid([-reach, -reach, -DEPTH - 16.0], [reach, reach, -DEPTH]),
        cuboid([-reach, -reach, -DEPTH], [0.0, reach, 0.0]),
        cuboid([px, -reach, -DEPTH], [reach, reach, 0.0]),
        cuboid([0.0, py, -DEPTH], [px, reach, 0.0]),
        cuboid([0.0, -reach, -DEPTH], [px, -py, 0.0]),
        Brush::cuboid([0.0, -py, -DEPTH], [px, py, -rim], &surface(liquid)).unwrap(),
    ];
    file.entities[0]
        .items
        .extend(brushes.into_iter().map(Item::Brush));
    file.to_bytes()
}

/// Whether a player on the bottom of a pool of `liquid` whose rim is `rim` above its surface,
/// facing its west wall with forward and jump held, stands on the ground west of it within
/// [`FRAMES`].
fn climbs_out(liquid: &str, rim: f64) -> bool {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&pool(liquid, rim), None)
        .unwrap();
    let bytes: &'static [u8] = Box::leak(compiled.bsp.into_boxed_slice());
    let bsp = Bsp::read(bytes, BspLoader::default()).unwrap();
    let progs_bytes = progs();
    let progs = Progs::read(&progs_bytes).unwrap();
    let mut world = World::load(
        &progs,
        "pool",
        &bsp,
        Rc::new(|_: &str| None),
        LoadOptions::default(),
    )
    .unwrap();
    world.settle(&progs).unwrap();
    world.connect(&progs).unwrap();
    world.set_command(UserCmd {
        angles: [0.0, 180.0, 0.0],
        forward: 320.0,
        buttons: 2,
        ..UserCmd::default()
    });
    // On the ground the origin is 24 above it and the eye 22 above that.
    let standing = 24.0 + EYE - 1.0;
    for _ in 0..FRAMES {
        world.frame(&progs, FRAME).unwrap();
        let (eye, _) = world.player_view().unwrap();
        if eye[0] < -16.0 && eye[2] > standing {
            return true;
        }
    }
    false
}

/// The rims of [`RIMS`] a swimmer in `liquid` climbs out over.
fn climbed(liquid: &str) -> Vec<f64> {
    RIMS.map(|step| f64::from(step) * RIM_STEP)
        .filter(|&rim| climbs_out(liquid, rim))
        .collect()
}

// spec PHYS-9.5: a water jump, begun by CheckWaterJump, lifts a swimmer out over a rim up to
// CLIMB_OUT above the surface and not over one higher (PHYS-8.4's waterlevel 2): CheckWaterJump needs solid ahead at the waist (origin + 8) and open space at the eye
// trace (origin + 32), while waterlevel is 2.
#[test]
fn a_swimmer_climbs_out_over_a_low_rim_and_not_a_high_one() {
    let out = climbed("*water1");
    let highest = out.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert_eq!(
        highest,
        qs_map_gen::CLIMB_OUT,
        "rims climbed out over: {out:?}"
    );
    assert!(
        out.iter().all(|&rim| rim <= highest),
        "every lower rim is climbed too: {out:?}"
    );
    let lowest = out.iter().copied().fold(f64::INFINITY, f64::min);
    let below: Vec<f64> = RIMS
        .map(|step| f64::from(step) * RIM_STEP)
        .filter(|&rim| rim >= lowest && rim <= highest)
        .collect();
    assert_eq!(out, below, "no gap between the rims climbed");
}

// spec PHYS-9.5: lava, where PlayerJump swims up at 50 and not 100, is climbed out of over the
// same rims:
// CheckWaterJump does not read the liquid.
#[test]
fn a_swimmer_climbs_out_of_lava_over_the_same_rims() {
    assert_eq!(climbed("*lava1"), climbed("*water1"));
}
