// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use qc_builtins::mock::Rng;
use qc_codegen::{Options, compile_module};
use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_progs::Progs;
use qc_sema::{Db, FileId, ModuleInput};
use qc_types::{Etype, Slot};
use qs_bsp::{Bsp, BspLoader, ClipNode};

use super::{World, WorldError, field_slot};
use crate::UserCmd;
use crate::collision::CONTENTS_WATER;
use crate::fixtures::{CONTENTS_EMPTY, CONTENTS_SOLID, MapSpec, bsp, node, plane};

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

/// The builtins the tests call.
const BUILTINS: &str = "\
void(entity e, vector o) setorigin = #2;
void(entity e, string m) setmodel = #3;
void(entity e, vector min, vector max) setsize = #4;
void(entity e) remove = #15;
void(vector v1, vector v2, float nomonsters, entity forent) traceline = #16;
string(string s) precache_model = #20;
float(float yaw, float dist) walkmove = #32;
float() droptofloor = #34;
float(entity e) checkbottom = #40;
float(vector v) pointcontents = #41;
void(float step) movetogoal = #67;
void(entity e) makestatic = #69;
float() random = #7;
entity() spawn = #14;
.float gravity;
.float count;
.float dmg;
.float speed;
entity marker;
";

/// The engine's entry points (EDICT-9.1).
const ENTRY_POINTS: &str = "void() main = {};\nvoid() StartFrame = {};\n\
    void() PlayerPreThink = {};\nvoid() PlayerPostThink = {};\nvoid() ClientKill = {};\n\
    void() ClientConnect = {};\nvoid() PutClientInServer = {};\n\
    void() ClientDisconnect = {};\nvoid() SetNewParms = {};\nvoid() SetChangeParms = {};\n\
    void() worldspawn = {};\n";

/// The progs `qc-codegen` writes for `source` after the system defs, the builtins and the
/// entry points.
fn compile(source: &str) -> Vec<u8> {
    let mut defs = String::new();
    for var in SYSTEM_GLOBALS {
        writeln!(defs, "{} {};", type_name(var.etype), var.name).unwrap();
    }
    defs.push_str("void end_sys_globals;\n");
    for var in SYSTEM_FIELDS {
        writeln!(defs, ".{} {};", type_name(var.etype), var.name).unwrap();
    }
    defs.push_str("void end_sys_fields;\n");
    defs.push_str(BUILTINS);
    let entry: String = ENTRY_POINTS
        .lines()
        .filter(|line| !source.contains(line.split('=').next().unwrap_or_default().trim_end()))
        .map(|line| format!("{line}\n"))
        .collect();
    let files = [
        ("defs.qc", defs),
        ("entry.qc", entry),
        ("t.qc", format!("{source}vector padding;\n")),
    ];
    let mut db = Db::new();
    let mut input = ModuleInput::default();
    for (i, (name, text)) in files.iter().enumerate() {
        db.set_file(FileId(i), PathBuf::from(name), text.as_bytes().to_vec());
        input.compiled.push(FileId(i));
    }
    let names: Vec<String> = files.iter().map(|(n, _)| (*n).to_owned()).collect();
    let compiled = compile_module(
        &mut db,
        &input,
        |id| names[id.0].clone(),
        Options::default(),
    )
    .unwrap_or_else(|e| panic!("{source:?} did not compile: {e:?}"));
    qc_progs::write(&compiled.progs).unwrap()
}

/// A floor at z = 0 for x below 100, and a pit floor at z = -30 past it; for the player's
/// hull, 24 and -6, with the edge at x = 116.
fn ledge() -> MapSpec {
    ledge_over(30.0)
}

/// The ledge with its pit floor `pit` below the floor.
fn ledge_over(pit: f32) -> MapSpec {
    MapSpec {
        planes: vec![
            plane([1.0, 0.0, 0.0], 100.0, 0),
            plane([0.0, 0.0, 1.0], 0.0, 2),
            plane([0.0, 0.0, 1.0], -pit, 2),
            plane([1.0, 0.0, 0.0], 116.0, 0),
            plane([0.0, 0.0, 1.0], 24.0, 2),
            plane([0.0, 0.0, 1.0], 24.0 - pit, 2),
        ],
        nodes: vec![node(0, [2, 1]), node(1, [-2, -1]), node(2, [-2, -1])],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY],
        clipnodes: vec![
            ClipNode {
                planenum: 3,
                children: [2, 1],
            },
            ClipNode {
                planenum: 4,
                children: [CONTENTS_EMPTY, CONTENTS_SOLID],
            },
            ClipNode {
                planenum: 5,
                children: [CONTENTS_EMPTY, CONTENTS_SOLID],
            },
        ],
    }
}

/// The world of `source`'s progs on the ledge, with `entities` after the worldspawn, reading
/// model files from `files`.
fn world_with(source: &str, entities: &str, files: Vec<(String, Vec<u8>)>) -> World {
    world_on(&ledge(), source, entities, files)
}

/// The world of `source`'s progs on the map `spec`.
fn world_on(spec: &MapSpec, source: &str, entities: &str, files: Vec<(String, Vec<u8>)>) -> World {
    running(spec, source, entities, files).world
}

/// A world and the progs it runs, so it can run frames.
struct Running {
    world: World,
    progs: Vec<u8>,
}

impl Running {
    /// Runs `count` frames of `frametime`.
    fn frames(&mut self, count: usize, frametime: f64) -> Result<(), WorldError> {
        let progs = Progs::read(&self.progs).map_err(|e| WorldError::Progs(e.to_string()))?;
        for _ in 0..count {
            self.world.frame(&progs, frametime)?;
        }
        Ok(())
    }
}

fn running(spec: &MapSpec, source: &str, entities: &str, files: Vec<(String, Vec<u8>)>) -> Running {
    let progs_bytes = compile(source);
    let progs = Progs::read(&progs_bytes).unwrap();
    let lump = format!("{{\n\"classname\" \"worldspawn\"\n}}\n{entities}");
    let mut map = bsp(spec);
    map.entity_lump = Box::leak(lump.into_bytes().into_boxed_slice());
    let read = Box::new(move |name: &str| {
        files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, bytes)| bytes.clone())
    });
    let world = World::load(&progs, "test", &map, read).expect("the world loads");
    Running {
        world,
        progs: progs_bytes,
    }
}

fn world(source: &str, entities: &str) -> World {
    world_with(source, entities, Vec::new())
}

/// An entity of `classname` at `origin` with `keys`. The map's entities start at edict 2,
/// after the client's (EDICT-4).
fn entity(classname: &str, origin: &str, keys: &str) -> String {
    format!("{{\n\"classname\" \"{classname}\"\n\"origin\" \"{origin}\"\n{keys}}}\n")
}

impl World {
    fn float(&self, n: usize, name: &str) -> f32 {
        self.machine
            .edicts()
            .field(n, field_slot(name))
            .map_or(f32::NAN, Slot::as_f32)
    }

    fn vector(&self, n: usize, name: &str) -> [f32; 3] {
        let first = field_slot(name);
        [0, 1, 2].map(|i| {
            self.machine
                .edicts()
                .field(n, first + i)
                .map_or(f32::NAN, Slot::as_f32)
        })
    }

    fn entity_field(&self, n: usize, name: &str) -> i32 {
        self.machine
            .edicts()
            .field(n, field_slot(name))
            .map_or(-1, Slot::as_i32)
    }

    fn linked(&self, n: usize) -> bool {
        self.builtins.collision.area().link_of(n).is_some()
    }
}

const PLAYER_SIZE: &str = "setsize(self, '-16 -16 -24', '16 16 32');";

// spec WORLD-8.5: droptofloor moves the box down to the floor it lands on, links it, sets
// FL_ONGROUND and groundentity and returns 1; all solid or no floor returns 0, changing
// nothing
// spec BUILTIN-15.3: droptofloor traces self's box 256 down and lands it
#[test]
fn droptofloor_lands_on_the_floor() {
    let source = format!(
        "void() dropper = {{ self.solid = 2; {PLAYER_SIZE} self.frags = droptofloor(); }};\n"
    );
    let entities = [
        entity("dropper", "50 0 100", ""),
        entity("dropper", "50 0 -50", ""),
        entity("dropper", "50 0 1000", ""),
    ]
    .concat();
    let world = world(&source, &entities);
    assert_eq!(world.float(2, "frags"), 1.0);
    let z = world.vector(2, "origin")[2];
    assert!((z - 24.031_25).abs() < 1e-3, "{z}");
    assert_eq!(world.float(2, "flags"), 512.0);
    assert_eq!(world.entity_field(2, "groundentity"), 0);
    assert!(world.linked(2));
    for (n, z) in [(3, -50.0), (4, 1000.0)] {
        assert_eq!(world.float(n, "frags"), 0.0, "{n}");
        assert_eq!(world.vector(n, "origin")[2], z, "{n}");
        assert_eq!(world.float(n, "flags"), 0.0, "{n}");
    }
}

// spec WORLD-8.4: traceline writes the trace to the trace_ globals, trace_ent the world when
// nothing is hit
// spec BUILTIN-14.2: traceline writes all nine trace_ globals
#[test]
fn traceline_writes_the_trace_globals() {
    let source = "void() tracer = { traceline('50 0 100', '50 0 -100', 0, self); \
        self.frags = trace_fraction; self.oldorigin = trace_endpos; \
        self.movedir = trace_plane_normal; self.health = trace_inopen; \
        self.armorvalue = trace_allsolid; self.ammo_shells = trace_plane_dist; \
        self.enemy = trace_ent; };\n";
    let world = world(source, &entity("tracer", "0 0 0", ""));
    assert!((world.float(2, "frags") - 0.499_843_75).abs() < 1e-6);
    assert_eq!(world.vector(2, "oldorigin"), [50.0, 0.0, 0.031_25]);
    assert_eq!(world.vector(2, "movedir"), [0.0, 0.0, 1.0]);
    assert_eq!(world.float(2, "health"), 1.0);
    assert_eq!(world.float(2, "armorvalue"), 0.0);
    assert_eq!(world.float(2, "ammo_shells"), 0.0);
    assert_eq!(world.entity_field(2, "enemy"), 0);
}

// spec BUILTIN-14.1: traceline hits linked entities, and nomonsters 1 passes all but brush
// models
#[test]
fn traceline_hits_entities_unless_nomonsters() {
    let source = format!(
        "void() blocker = {{ self.solid = 2; {PLAYER_SIZE} setorigin(self, self.origin); }};\n\
         void() tracer = {{ traceline('0 0 50', '100 0 50', self.count, self); \
         self.frags = trace_fraction; self.enemy = trace_ent; }};\n"
    );
    let entities = [
        entity("blocker", "50 0 50", ""),
        entity("tracer", "0 0 0", ""),
        entity("tracer", "0 0 0", "\"count\" \"1\"\n"),
    ]
    .concat();
    let world = world(&source, &entities);
    assert!(world.linked(2));
    assert!(world.float(3, "frags") < 1.0);
    let blocker = world.entity_value(2);
    assert_eq!(world.entity_field(3, "enemy"), blocker);
    assert_eq!(world.float(4, "frags"), 1.0);
    assert_eq!(world.entity_field(4, "enemy"), 0);
}

// spec WORLD-8.6: checkbottom is MOVE-1 and pointcontents WORLD-2.2
// spec MOVE-1.1: solid under all four corners is ground
// spec MOVE-1.2: a centre trace that finds no floor within 36 is no ground
// spec MOVE-1.3: a corner more than 18 below the centre is no ground
// spec BUILTIN-14.4: pointcontents is the world's contents at the point
// spec BUILTIN-14.5: checkbottom is 1 when the box is supported
#[test]
fn checkbottom_and_pointcontents() {
    let source = format!(
        "void() prober = {{ {PLAYER_SIZE} setorigin(self, self.origin); \
         self.frags = pointcontents(self.origin - '0 0 30'); self.health = checkbottom(self); }};\n"
    );
    let entities = [
        entity("prober", "50 0 24", ""),
        entity("prober", "90 0 24", ""),
        entity("prober", "200 0 -6", ""),
        entity("prober", "50 0 100", ""),
        entity("prober", "100 0 24", ""),
    ]
    .concat();
    let world = world(&source, &entities);
    assert_eq!(world.float(2, "frags"), -2.0);
    assert_eq!(world.float(2, "health"), 1.0);
    // Over the edge: the corner past it finds the pit floor 30 below the centre's.
    assert_eq!(world.float(3, "health"), 0.0);
    assert_eq!(world.float(4, "health"), 1.0);
    assert_eq!(world.float(5, "frags"), -1.0);
    assert_eq!(world.float(5, "health"), 0.0);
    // The centre on the pit side lands on the pit floor; the corner on the upper floor is
    // above it, which MOVE-1.3 does not refuse: supported.
    assert_eq!(world.float(6, "health"), 1.0);
}

/// A walker: dropped to the floor, then `walkmove (0, speed)` into `frags`.
fn walker_source() -> String {
    format!(
        "void() walker = {{ {PLAYER_SIZE} setorigin(self, self.origin); droptofloor(); \
         self.frags = walkmove(self.angles_y, self.speed); }};\n\
         void() floater = {{ {PLAYER_SIZE} setorigin(self, self.origin); \
         self.frags = walkmove(0, self.speed); }};\n\
         void() mark = {{ marker = self; }};\n\
         void() flyer = {{ {PLAYER_SIZE} self.enemy = marker; setorigin(self, self.origin); \
         self.frags = walkmove(0, self.speed); }};\n"
    )
}

fn walker(origin: &str, speed: &str, flags: &str) -> String {
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!("\"flags\" \"{flags}\"\n")
    };
    entity("walker", origin, &format!("\"speed\" \"{speed}\"\n{flags}"))
}

// spec WORLD-8.6: walkmove steps self when it is on the ground, flying or swimming
// spec MOVE-2.2: the box is pushed down from a step above the wished position
// spec MOVE-2.5: a good step clears FL_PARTIALGROUND and sets groundentity
// spec BUILTIN-15.1: walkmove returns 1 when it moved and 0 when not
#[test]
fn walkmove_steps_along_the_floor() {
    let entities = [
        walker("50 0 30", "10", ""),
        walker("50 0 30", "10", "1024"),
        entity("floater", "50 0 30", "\"speed\" \"10\"\n"),
    ]
    .concat();
    let world = world(&walker_source(), &entities);
    assert_eq!(world.float(2, "frags"), 1.0);
    let [x, _, z] = world.vector(2, "origin");
    assert_eq!(x, 60.0);
    assert!((z - 24.031_25).abs() < 1e-3, "{z}");
    assert_eq!(world.entity_field(2, "groundentity"), 0);
    assert_eq!(world.float(3, "frags"), 1.0);
    assert_eq!(world.float(3, "flags"), 512.0);
    // Not on the ground: no step.
    assert_eq!(world.float(4, "frags"), 0.0);
    assert_eq!(world.vector(4, "origin"), [50.0, 0.0, 30.0]);
}

// spec MOVE-2.3: a step off an edge fails, unless the ground was pulled out, when it falls
// spec MOVE-2.4: a step whose corners hang over the edge fails and puts the box back, unless
// the ground was pulled out
#[test]
fn walkmove_stops_at_edges() {
    let entities = [
        walker("50 0 30", "80", ""),
        walker("50 0 30", "45", ""),
        walker("50 0 30", "80", "1024"),
        walker("50 0 30", "45", "1024"),
    ]
    .concat();
    let world = world(&walker_source(), &entities);
    for n in [2, 3] {
        assert_eq!(world.float(n, "frags"), 0.0, "{n}");
        assert_eq!(world.vector(n, "origin")[0], 50.0, "{n}");
    }
    // Falls: moved by the step, FL_ONGROUND cleared.
    assert_eq!(world.float(4, "frags"), 1.0);
    assert_eq!(world.vector(4, "origin")[0], 130.0);
    assert_eq!(world.float(4, "flags"), 1024.0);
    // Hangs over the edge but keeps the step.
    assert_eq!(world.float(5, "frags"), 1.0);
    assert_eq!(world.vector(5, "origin")[0], 95.0);
}

// spec MOVE-2.1: a flyer moves without stepping, toward its enemy's height; a swimmer may not
// leave the water
#[test]
fn flyers_and_swimmers() {
    let entities = [
        entity("mark", "0 0 0", ""),
        entity("flyer", "50 0 100", "\"speed\" \"10\"\n\"flags\" \"1\"\n"),
        entity("flyer", "50 0 100", "\"speed\" \"10\"\n\"flags\" \"2\"\n"),
        entity("flyer", "50 0 35", "\"speed\" \"10\"\n\"flags\" \"1\"\n"),
        entity("flyer", "50 0 25", "\"speed\" \"10\"\n\"flags\" \"1\"\n"),
    ]
    .concat();
    let world = world(&walker_source(), &entities);
    // 100 above its enemy: 8 lower.
    assert_eq!(world.float(3, "frags"), 1.0);
    assert_eq!(world.vector(3, "origin"), [60.0, 0.0, 92.0]);
    // A swimmer in the open: "swim monster left water".
    assert_eq!(world.float(4, "frags"), 0.0);
    assert_eq!(world.vector(4, "origin"), [50.0, 0.0, 100.0]);
    // 35 above, neither above 40 nor below 30: level.
    assert_eq!(world.vector(5, "origin"), [60.0, 0.0, 35.0]);
    // 25 above: 8 higher.
    assert_eq!(world.vector(6, "origin"), [60.0, 0.0, 33.0]);
}

// spec WORLD-8.1: setorigin, setsize and setmodel link the entity
// spec WORLD-8.3: remove and makestatic unlink it
#[test]
fn placing_links_and_removing_unlinks() {
    let source = format!(
        "void() placed = {{ self.solid = 2; {PLAYER_SIZE} }};\n\
         void() moved = {{ self.solid = 2; setorigin(self, '300 0 50'); }};\n\
         void() gone = {{ self.solid = 2; {PLAYER_SIZE} remove(self); }};\n\
         void() still = {{ self.solid = 2; {PLAYER_SIZE} makestatic(self); }};\n"
    );
    let entities = [
        entity("placed", "50 0 50", ""),
        entity("moved", "0 0 0", ""),
        entity("gone", "50 0 50", ""),
        entity("still", "50 0 50", ""),
    ]
    .concat();
    let world = world(&source, &entities);
    assert!(world.linked(2));
    assert_eq!(world.vector(2, "absmin"), [33.0, -17.0, 25.0]);
    assert!(world.linked(3));
    assert_eq!(world.vector(3, "absmin"), [299.0, -1.0, 49.0]);
    assert!(!world.linked(4));
    assert!(!world.linked(5));
}

// spec WORLD-8.3: an entity spawned with no spawn function is freed and unlinked
#[test]
fn spawning_frees_unknown_classes() {
    let world = world("", &entity("nothing_here", "0 0 0", ""));
    assert!(!world.linked(2));
    assert_eq!(world.machine.edicts().is_free(2), Some(true));
}

/// A BSP29 file of `spec`'s planes, nodes, leaves and clipnodes and one model over them.
fn bsp_file(spec: &MapSpec, mins: [f32; 3], maxs: [f32; 3]) -> Vec<u8> {
    let mut lumps: Vec<Vec<u8>> = vec![Vec::new(); 15];
    for p in &spec.planes {
        let l = &mut lumps[1];
        for v in p.normal {
            l.extend(v.to_le_bytes());
        }
        l.extend(p.dist.to_le_bytes());
        l.extend(p.plane_type.to_le_bytes());
    }
    for n in &spec.nodes {
        let l = &mut lumps[5];
        l.extend(i32::try_from(n.planenum).unwrap().to_le_bytes());
        for c in n.children {
            l.extend(i16::try_from(c).unwrap().to_le_bytes());
        }
        l.extend([0; 16]);
    }
    for c in &spec.clipnodes {
        let l = &mut lumps[9];
        l.extend(c.planenum.to_le_bytes());
        for child in c.children {
            l.extend(i16::try_from(child).unwrap().to_le_bytes());
        }
    }
    for &contents in &spec.leafs {
        let l = &mut lumps[10];
        l.extend(contents.to_le_bytes());
        l.extend((-1_i32).to_le_bytes());
        l.extend([0; 20]);
    }
    let model = &mut lumps[14];
    for v in mins.iter().chain(&maxs).chain(&[0.0; 3]) {
        model.extend(v.to_le_bytes());
    }
    model.extend([0; 16]);
    model.extend([0; 12]);
    let mut out = Vec::new();
    out.extend(29_i32.to_le_bytes());
    let mut offset = 4 + 15 * 8;
    for lump in &lumps {
        out.extend(i32::try_from(offset).unwrap().to_le_bytes());
        out.extend(i32::try_from(lump.len()).unwrap().to_le_bytes());
        offset += lump.len();
    }
    for lump in lumps {
        out.extend(lump);
    }
    out
}

/// A one-frame sprite `width` by `height`.
fn sprite_file(width: i32, height: i32) -> Vec<u8> {
    let mut out = b"IDSP".to_vec();
    for v in [1, 2] {
        out.extend(i32::to_le_bytes(v));
    }
    out.extend(10.0_f32.to_le_bytes());
    for v in [width, height, 1] {
        out.extend(v.to_le_bytes());
    }
    out.extend(0.0_f32.to_le_bytes());
    out.extend(0_i32.to_le_bytes());
    for v in [0, 0, 0, 1, 1] {
        out.extend(i32::to_le_bytes(v));
    }
    out.push(0);
    out
}

// spec WORLD-8.2: setmodel takes a precached BSP's bounds and makes it a brush model the world
// clips against; a sprite's bounds are its own; an alias model's are the mock's 16
#[test]
fn precached_models_bring_their_bounds_and_hulls() {
    let crate_box = MapSpec {
        planes: vec![
            plane([0.0, 0.0, 1.0], 10.0, 2),
            plane([0.0, 0.0, 1.0], 34.0, 2),
        ],
        nodes: vec![node(0, [-2, -1])],
        ..MapSpec::default()
    };
    let bytes = bsp_file(&crate_box, [-8.0; 3], [8.0; 3]);
    if let Err(e) = Bsp::read(&bytes, BspLoader::default()) {
        panic!("{}", e.message("b_box"));
    }
    let source = "void() worldspawn = { precache_model(\"maps/b_box.bsp\"); \
        precache_model(\"progs/s.spr\"); precache_model(\"progs/t.mdl\"); };\n\
        void() boxed = { self.solid = 4; self.movetype = 7; setmodel(self, \"maps/b_box.bsp\"); \
        setorigin(self, self.origin); };\n\
        void() sprite = { setmodel(self, \"progs/s.spr\"); };\n\
        void() thing = { setmodel(self, \"progs/t.mdl\"); };\n\
        void() tracer = { traceline('300 0 100', '300 0 -100', 0, self); \
        self.frags = trace_fraction; self.enemy = trace_ent; };\n\
        void() rider = { self.solid = 2; setsize(self, '-16 -16 -24', '16 16 32'); \
        setorigin(self, self.origin); droptofloor(); };\n";
    let entities = [
        entity("boxed", "300 0 50", ""),
        entity("sprite", "0 0 0", ""),
        entity("thing", "0 0 0", ""),
        entity("tracer", "0 0 0", ""),
        entity("rider", "300 0 200", ""),
    ]
    .concat();
    let files = vec![
        ("maps/b_box.bsp".to_owned(), bytes),
        ("progs/s.spr".to_owned(), sprite_file(20, 40)),
    ];
    let world = world_with(source, &entities, files);
    // BSP-15.4 widens the stored -8 to 8 by 1.
    assert_eq!(world.vector(2, "mins"), [-9.0; 3]);
    assert_eq!(world.vector(2, "size"), [18.0; 3]);
    assert_eq!(world.vector(3, "mins"), [-10.0, -10.0, -20.0]);
    assert_eq!(world.vector(4, "mins"), [-16.0; 3]);
    // The box's floor at its z = 10, moved to its origin: 60.
    let fraction = world.float(5, "frags");
    assert!((fraction - 0.2).abs() < 1e-3, "{fraction}");
    assert_eq!(world.entity_field(5, "enemy"), world.entity_value(2));
    // Dropped onto the box: its hull 1 top is 34 up, at 84.
    assert!((world.vector(6, "origin")[2] - 84.031_25).abs() < 1e-3);
    assert_eq!(world.entity_field(6, "groundentity"), world.entity_value(2));
    let models: Vec<String> = world.entities().into_iter().map(|e| e.model).collect();
    assert!(models.contains(&"progs/t.mdl".to_owned()), "{models:?}");
    assert!(models.contains(&"maps/b_box.bsp".to_owned()), "{models:?}");
}

// spec MOVE-5.1: movetogoal does nothing for a monster not on the ground, flying or swimming,
// writing 0
// spec MOVE-5.2: it stops when the goal is within the step of an entity with an enemy
// spec MOVE-5.3: else it steps toward ideal_yaw unless rand() & 3 is 1
// spec MOVE-3.1: the step sets ideal_yaw, turns and moves
// spec MOVE-3.2: a step not turned far enough is undone, but still counts
// spec BUILTIN-15.6: movetogoal moves toward the goal
#[test]
fn movetogoal_steps_toward_the_goal() {
    let source = format!(
        "void() goal = {{ {PLAYER_SIZE} setorigin(self, self.origin); marker = self; }};\n\
         void() chaser = {{ {PLAYER_SIZE} setorigin(self, self.origin); droptofloor(); \
         self.goalentity = marker; self.yaw_speed = 360; \
         if (self.count) self.enemy = self.goalentity; self.frags = 7; movetogoal(10); }};\n\
         void() lazy = {{ {PLAYER_SIZE} setorigin(self, self.origin); self.frags = 7; \
         movetogoal(10); self.health = self.frags; }};\n"
    );
    let entities = [
        entity("goal", "90 0 30", ""),
        entity("chaser", "20 0 30", ""),
        entity("chaser", "60 0 30", "\"count\" \"1\"\n"),
        entity("lazy", "20 0 30", ""),
    ]
    .concat();
    let world = world(&source, &entities);
    // The first draw of seed 0 is 0, so the step toward ideal_yaw 0 is tried, and taken.
    assert_eq!(world.vector(3, "origin")[0], 30.0);
    assert_eq!(world.float(3, "frags"), 7.0);
    // Within 10 of the goal with an enemy: no move.
    assert_eq!(world.vector(4, "origin")[0], 60.0);
    // Not on the ground: 0 returned, nothing moved.
    assert_eq!(world.vector(5, "origin")[0], 20.0);
}

/// A pillar: floor at z = 0 within 20 of the origin on x and y, nothing around it; for the
/// player's hull, within 36 at 24.
fn pillar() -> MapSpec {
    let axial = |kind: i32, dist: f32| {
        let mut normal = [0.0; 3];
        normal[usize::try_from(kind).unwrap()] = 1.0;
        plane(normal, dist, kind)
    };
    MapSpec {
        planes: vec![
            axial(0, 20.0),
            axial(0, -20.0),
            axial(1, 20.0),
            axial(1, -20.0),
            axial(2, 0.0),
            axial(0, 36.0),
            axial(0, -36.0),
            axial(1, 36.0),
            axial(1, -36.0),
            axial(2, 24.0),
        ],
        nodes: vec![
            node(0, [-2, 1]),
            node(1, [2, -2]),
            node(2, [-2, 3]),
            node(3, [4, -2]),
            node(4, [-2, -1]),
        ],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY],
        // The player's hull: the walls 16 further out, the floor 24 up.
        clipnodes: vec![
            ClipNode {
                planenum: 5,
                children: [CONTENTS_EMPTY, 1],
            },
            ClipNode {
                planenum: 6,
                children: [2, CONTENTS_EMPTY],
            },
            ClipNode {
                planenum: 7,
                children: [CONTENTS_EMPTY, 3],
            },
            ClipNode {
                planenum: 8,
                children: [4, CONTENTS_EMPTY],
            },
            ClipNode {
                planenum: 9,
                children: [CONTENTS_EMPTY, CONTENTS_SOLID],
            },
        ],
    }
}

/// A goal and a chaser that steps `speed` toward it with `movetogoal`.
/// The chase, after `worldspawn` draws `skip` times from the random source.
fn chase_source_after(skip: usize) -> String {
    format!(
        "void() worldspawn = {{ local float i; i = 0; while (i < {skip}) {{ random(); i = i + 1; }} }};\n\
         void() goal = {{ {PLAYER_SIZE} setorigin(self, self.origin); marker = self; }};\n\
         void() chaser = {{ {PLAYER_SIZE} setorigin(self, self.origin); droptofloor(); \
         self.goalentity = marker; if (self.count) self.enemy = marker; \
         movetogoal(self.speed); }};\n"
    )
}

fn chase(spec: &MapSpec, goal: &str, chaser: &str, keys: &str) -> World {
    chase_after(spec, 0, goal, chaser, keys)
}

fn chase_after(spec: &MapSpec, skip: usize, goal: &str, chaser: &str, keys: &str) -> World {
    let entities = [entity("goal", goal, ""), entity("chaser", chaser, keys)].concat();
    world_on(spec, &chase_source_after(skip), &entities, Vec::new())
}

const EAST: &str = "\"ideal_yaw\" \"0\"\n\"speed\" \"10\"\n\"yaw_speed\" \"360\"\n";

/// The draws of seed 0's random source, from the first.
fn draws(count: usize) -> Vec<u32> {
    let mut rng = Rng::new(0);
    (0..count).map(|_| rng.next_rand()).collect()
}

/// The first number of draws to skip after which `first(draw)` and `second(next draw)` hold.
fn skip_where(first: impl Fn(u32) -> bool, second: impl Fn(u32) -> bool) -> usize {
    let all = draws(200);
    (0..199)
        .find(|&n| first(all[n]) && second(all[n + 1]))
        .expect("seed 0 has such a pair of draws")
}

// spec MOVE-4.1: the headings toward the goal are its sides on x and y beyond 10
// spec MOVE-4.3: when the goal is further off on y than on x, y is tried first
#[test]
fn a_blocked_chaser_turns_toward_the_goal() {
    // East of x = 80 the corner hangs over the pit, so the step toward ideal_yaw 0 fails.
    let world = chase(&ledge(), "80 100 30", "80 0 30", EAST);
    let [x, y, _] = world.vector(3, "origin");
    assert_eq!((x, y), (80.0, 10.0));
    assert_eq!(world.float(3, "ideal_yaw"), 90.0);
}

// spec MOVE-4.2: a goal off on both axes is tried on id's diagonal first
#[test]
fn a_chaser_takes_the_diagonal() {
    let world = chase(&ledge(), "-200 200 30", "80 0 30", EAST);
    assert_eq!(world.float(3, "ideal_yaw"), 135.0);
    let [x, y, _] = world.vector(3, "origin");
    assert!(
        (x - 72.928_93).abs() < 1e-3 && (y - 7.071_07).abs() < 1e-3,
        "{x} {y}"
    );
}

// spec MOVE-4.4: with no heading toward the goal, the eight headings are tried in the order a
// rand() picks
#[test]
fn the_search_order_follows_rand() {
    let world = chase(&ledge(), "80 0 30", "80 0 30", EAST);
    // Seed 0: movetogoal's draw, the swap's, then the order's.
    let ascending = draws(3)[2] & 1 != 0;
    let y = world.vector(3, "origin")[1];
    assert_eq!(y, if ascending { 10.0 } else { -10.0 });
}

// spec MOVE-4.5: when nothing succeeds ideal_yaw goes back to olddir, and an actor not on
// ground gets FL_PARTIALGROUND
#[test]
fn a_chaser_that_cannot_move() {
    let keys = "\"ideal_yaw\" \"100\"\n\"speed\" \"30\"\n\"yaw_speed\" \"360\"\n";
    let world = chase(&pillar(), "0 0 30", "0 0 30", keys);
    // olddir is (int)(100/45)*45.
    assert_eq!(world.float(3, "ideal_yaw"), 90.0);
    let [x, y, _] = world.vector(3, "origin");
    assert_eq!((x, y), (0.0, 0.0));
    assert_eq!(world.float(3, "flags"), 512.0);
    let world = chase(&pillar(), "10 0 30", "10 0 30", keys);
    assert_eq!(world.float(3, "flags"), 512.0 + 1024.0);
}

/// A floor at z = 0 with a hole 20 wide around the origin whose floor is `depth` down; the
/// player's hull is the floor at 24, the hole too narrow for it.
fn well(depth: f32) -> MapSpec {
    MapSpec {
        planes: vec![
            plane([1.0, 0.0, 0.0], 10.0, 0),
            plane([1.0, 0.0, 0.0], -10.0, 0),
            plane([0.0, 1.0, 0.0], 10.0, 1),
            plane([0.0, 1.0, 0.0], -10.0, 1),
            plane([0.0, 0.0, 1.0], 0.0, 2),
            plane([0.0, 0.0, 1.0], -depth, 2),
            plane([0.0, 0.0, 1.0], 24.0, 2),
        ],
        nodes: vec![
            node(0, [4, 1]),
            node(1, [2, 4]),
            node(2, [4, 3]),
            node(3, [5, 4]),
            node(4, [-2, -1]),
            node(5, [-2, -1]),
        ],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY],
        clipnodes: vec![ClipNode {
            planenum: 6,
            children: [CONTENTS_EMPTY, CONTENTS_SOLID],
        }],
    }
}

/// `checkbottom` of a box of `mins` to `maxs` at `origin` on `spec`.
fn bottom_of(spec: &MapSpec, origin: &str, size: &str) -> f32 {
    let source = format!(
        "void() prober = {{ {size} setorigin(self, self.origin); self.frags = checkbottom(self); }};\n"
    );
    let world = world_on(spec, &source, &entity("prober", origin, ""), Vec::new());
    world.float(2, "frags")
}

const SMALL: &str = "setsize(self, '-4 -4 -24', '4 4 32');";

// spec MOVE-1.1: solid one unit below every corner is ground, whatever is under the centre
// spec MOVE-1.2: the centre's trace starts from the middle of the box's bottom
// spec MOVE-1.3: a corner exactly 18 below the centre is still ground; more is not
#[test]
fn the_ground_check_reads_the_corners_and_the_centre() {
    assert_eq!(bottom_of(&well(50.0), "0 0 24", PLAYER_SIZE), 1.0);
    // A corner over a hole 20 deep, the centre on the floor: no ground.
    assert_eq!(bottom_of(&well(20.0), "12 0 24", SMALL), 0.0);
    assert_eq!(bottom_of(&well(20.0), "0 12 24", SMALL), 0.0);
    // The centre over the hole, a corner on the floor above it: ground.
    assert_eq!(bottom_of(&well(20.0), "6 0 24", SMALL), 1.0);
    assert_eq!(bottom_of(&well(20.0), "0 6 24", SMALL), 1.0);
    // Standing as droptofloor leaves it, 1/32 up, the centre ends 1/32 above the floor and the
    // corner 1/32 above the hole's: 18 exactly.
    assert_eq!(bottom_of(&well(18.0), "12 0 24.03125", SMALL), 1.0);
    assert_eq!(bottom_of(&well(18.5), "12 0 24.03125", SMALL), 0.0);
}

// spec MOVE-2.2: a step pushed down from 18 above lands up to 18 below: a stair down
#[test]
fn walkmove_takes_a_stair_down() {
    // The walker stands 1/32 above its floor, so a stair of exactly 18 is out of reach.
    let stair = |drop: f32| {
        let world = world_on(
            &ledge_over(drop),
            &walker_source(),
            &walker("50 0 30", "80", ""),
            Vec::new(),
        );
        (world.float(2, "frags"), world.vector(2, "origin"))
    };
    let (moved, [x, _, z]) = stair(17.5);
    assert_eq!((moved, x), (1.0, 130.0));
    assert!((z - 6.531_25).abs() < 1e-3, "{z}");
    assert_eq!(stair(18.0).0, 0.0);
}

/// A floor at z = 0 under a ceiling at 60; for the player's hull, 24 and 28.
fn tunnel() -> MapSpec {
    MapSpec {
        planes: vec![
            plane([0.0, 0.0, 1.0], 60.0, 2),
            plane([0.0, 0.0, 1.0], 0.0, 2),
            plane([0.0, 0.0, 1.0], 28.0, 2),
            plane([0.0, 0.0, 1.0], 24.0, 2),
        ],
        nodes: vec![node(0, [-1, 1]), node(1, [-2, -1])],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY],
        clipnodes: vec![
            ClipNode {
                planenum: 2,
                children: [CONTENTS_SOLID, 1],
            },
            ClipNode {
                planenum: 3,
                children: [CONTENTS_EMPTY, CONTENTS_SOLID],
            },
        ],
    }
}

// spec MOVE-2.2: a step that starts in solid a step up is tried again from the floor's height
#[test]
fn walkmove_under_a_low_ceiling() {
    let world = world_on(
        &tunnel(),
        &walker_source(),
        &walker("0 0 26", "10", ""),
        Vec::new(),
    );
    assert_eq!(world.float(2, "frags"), 1.0);
    let [x, _, z] = world.vector(2, "origin");
    assert_eq!(x, 10.0);
    assert!((z - 24.031_25).abs() < 1e-3, "{z}");
}

/// A flyer at `height` stepping 10 east, with an enemy at `enemy`.
fn flyer_at(height: f32, enemy: f32) -> [f32; 3] {
    let entities = [
        entity("mark", &format!("0 0 {enemy}"), ""),
        entity(
            "flyer",
            &format!("50 0 {height}"),
            "\"speed\" \"10\"\n\"flags\" \"1\"\n",
        ),
    ]
    .concat();
    world(&walker_source(), &entities).vector(3, "origin")
}

// spec MOVE-2.1: the height bends only above 40 and below 30, and a first try that hits is
// tried again without it
#[test]
fn flyers_bend_toward_their_enemy() {
    assert_eq!(flyer_at(140.0, 100.0), [60.0, 0.0, 140.0]);
    assert_eq!(flyer_at(141.0, 100.0), [60.0, 0.0, 133.0]);
    assert_eq!(flyer_at(130.0, 100.0), [60.0, 0.0, 130.0]);
    assert_eq!(flyer_at(120.0, 100.0), [60.0, 0.0, 128.0]);
    // 8 lower would be in the floor: the second try keeps the height.
    assert_eq!(flyer_at(30.0, -100.0), [60.0, 0.0, 30.0]);
}

/// A chaser at (20, 0) facing `ideal` with `angles` that turns `yaw_speed` a step.
fn turner(ideal: &str, angles: &str) -> [f32; 3] {
    let keys =
        format!("\"ideal_yaw\" \"{ideal}\"\n\"angles\" \"0 {angles} 0\"\n\"speed\" \"10\"\n");
    chase(&ledge(), "-500 -500 30", "20 0 30", &keys).vector(3, "origin")
}

// spec MOVE-3.2: a step whose yaw is more than 45 and less than 315 short of ideal_yaw is put
// back
#[test]
fn a_step_not_turned_far_enough_is_put_back() {
    let [x, y, _] = turner("90", "180");
    assert_eq!((x, y), (20.0, 0.0));
    let [x, y, _] = turner("90", "135");
    assert_eq!((x, y), (20.0, 10.0));
    let [x, y, _] = turner("90", "0");
    assert_eq!((x, y), (20.0, 10.0));
    let [x, y, _] = turner("0", "315");
    assert_eq!((x, y), (30.0, 0.0));
    let [x, y, _] = turner("0", "314");
    assert_eq!((x, y), (20.0, 0.0));
}

/// A chase from (-50, 50) facing north, where the first draw forces a new direction.
fn chase_from(goal: &str) -> [f32; 2] {
    let skip = skip_where(|d| d & 3 == 1, |_| true);
    let keys = "\"ideal_yaw\" \"90\"\n\"speed\" \"10\"\n\"yaw_speed\" \"360\"\n";
    let [x, y, _] = chase_after(&ledge(), skip, goal, "-50 50 30", keys).vector(3, "origin");
    [x, y]
}

fn near(got: [f32; 2], want: [f32; 2]) -> bool {
    (got[0] - want[0]).abs() < 1e-3 && (got[1] - want[1]).abs() < 1e-3
}

// spec MOVE-4.1: each axis heads toward the goal only when it is more than 10 off
// spec MOVE-4.2: the diagonal of the two headings, 45, 315, 135 or 215, is tried first
// spec MOVE-5.3: when rand() & 3 is 1 the step toward ideal_yaw is not tried
#[test]
fn chase_headings_and_diagonals() {
    let d = 10.0 * core::f32::consts::FRAC_1_SQRT_2;
    let cases: [(&str, [f32; 2]); 10] = [
        ("-50 150 30", [-50.0, 60.0]),
        ("100 50 30", [-40.0, 50.0]),
        ("-40 150 30", [-50.0, 60.0]),
        ("-60 150 30", [-50.0, 60.0]),
        ("100 40 30", [-40.0, 50.0]),
        ("100 60 30", [-40.0, 50.0]),
        ("100 150 30", [-50.0 + d, 50.0 + d]),
        ("100 -50 30", [-50.0 + d, 50.0 - d]),
        ("-200 150 30", [-50.0 - d, 50.0 + d]),
        (
            "-200 -50 30",
            [
                -50.0 + 10.0 * 215.0_f32.to_radians().cos(),
                50.0 + 10.0 * 215.0_f32.to_radians().sin(),
            ],
        ),
    ];
    for (goal, want) in cases {
        let got = chase_from(goal);
        assert!(near(got, want), "{goal}: {got:?} not {want:?}");
    }
}

/// A chaser at (20, 0) facing 225, so 45 is its turnaround, after `skip` draws.
fn swap_chase(skip: usize, goal: &str) -> [f32; 2] {
    let keys = "\"ideal_yaw\" \"225\"\n\"speed\" \"10\"\n\"yaw_speed\" \"360\"\n";
    let [x, y, _] = chase_after(&ledge(), skip, goal, "20 0 30", keys).vector(3, "origin");
    [x, y]
}

// spec MOVE-4.3: x is tried before y unless the draw's bit 0 is set or y is further off
#[test]
fn chase_order_follows_the_draw_and_the_distance() {
    let plain = skip_where(|d| d & 3 == 1, |d| d & 1 == 0);
    let swapped = skip_where(|d| d & 3 == 1, |d| d & 1 != 0);
    // 45, the diagonal, is the turnaround, so the two headings are tried alone.
    assert_eq!(swap_chase(plain, "200 30 30"), [30.0, 0.0]);
    assert_eq!(swap_chase(swapped, "200 30 30"), [20.0, 10.0]);
    assert_eq!(swap_chase(plain, "40 200 30"), [20.0, 10.0]);
    // As far off on both: x first.
    assert_eq!(swap_chase(plain, "220 200 30"), [30.0, 0.0]);
}

// spec MOVE-4.1: olddir is ideal_yaw cut to the 45 below it
#[test]
fn olddir_is_cut_to_45() {
    let keys = "\"ideal_yaw\" \"50\"\n\"speed\" \"30\"\n\"yaw_speed\" \"360\"\n";
    let world = chase(&pillar(), "0 0 30", "0 0 30", keys);
    assert_eq!(world.float(3, "ideal_yaw"), 45.0);
}

// spec MOVE-5.2: close enough is within the step of the goal's box on every axis, either side
#[test]
fn close_enough_is_within_a_step_either_side() {
    let at = |goal: &str| {
        let keys = format!("{EAST}\"count\" \"1\"\n");
        chase(&ledge(), goal, "60 0 30", &keys).vector(3, "origin")[0]
    };
    assert_eq!(at("90 0 30"), 60.0);
    assert_eq!(at("20 0 30"), 60.0);
    assert_eq!(at("130 0 30"), 70.0);
    assert_eq!(at("-10 0 30"), 70.0);
    assert_eq!(at("60 100 30"), 70.0);
}

// spec WORLD-8.6: a step links the walker touching triggers, and self is the walker after it
// spec WORLD-6.6: the touch runs with self the trigger and other the walker
#[test]
fn a_walker_touches_triggers_it_steps_into() {
    let source = format!(
        "{}void() pad = {{ self.solid = 1; self.touch = pad_touch; setsize(self, '-8 -8 -8', '8 8 8'); \
         setorigin(self, self.origin); }};\n\
         void() pad_touch = {{ self.frags = 1; other.health = 5; }};\n\
         void() pacer = {{ self.solid = 3; {PLAYER_SIZE} setorigin(self, self.origin); droptofloor(); \
         walkmove(0, 10); self.armorvalue = 1; }};\n",
        "void() pad_touch;\n"
    );
    let entities = [entity("pad", "80 0 30", ""), entity("pacer", "50 0 30", "")].concat();
    let world = world(&source, &entities);
    assert_eq!(world.float(2, "frags"), 1.0);
    assert_eq!(world.float(3, "health"), 5.0);
    assert_eq!(world.float(3, "armorvalue"), 1.0);
    assert_eq!(world.float(2, "armorvalue"), 0.0);
    assert_eq!(world.vector(3, "absmin")[0], 43.0);
}

// spec WORLD-8.5: droptofloor keeps the flags it had besides FL_ONGROUND
// spec MOVE-2.5: a step leaves flags that are not a whole number as they are
#[test]
fn flags_are_kept() {
    let source = format!(
        "void() dropper = {{ self.solid = 2; {PLAYER_SIZE} droptofloor(); }};\n{}",
        walker_source()
    );
    let entities = [
        entity("dropper", "-300 0 100", "\"flags\" \"512\"\n"),
        entity(
            "floater",
            "20 0 24.03125",
            "\"speed\" \"10\"\n\"flags\" \"512.5\"\n",
        ),
    ]
    .concat();
    let world = world(&source, &entities);
    assert_eq!(world.float(2, "flags"), 512.0);
    assert_eq!(world.float(3, "frags"), 1.0);
    assert_eq!(world.float(3, "flags"), 512.5);
}

// spec EDICT-4.1: the world's model and mapname are engine strings the progs can compare
#[test]
fn the_maps_names_are_engine_strings() {
    let source = "void() namer = { if (mapname == \"test\") self.frags = 1; \
        if (world.model == \"maps/test.bsp\") self.health = 1; };\n";
    let world = world(source, &entity("namer", "0 0 0", ""));
    assert_eq!(world.float(2, "frags"), 1.0);
    assert_eq!(world.float(2, "health"), 1.0);
}

// spec MOVE-5.1: a flyer off the ground still moves toward its goal
#[test]
fn a_flying_chaser_moves() {
    let source = "void() goal = { marker = self; };\n\
        void() glider = { setsize(self, '-16 -16 -24', '16 16 32'); setorigin(self, self.origin); \
        self.goalentity = marker; self.frags = 7; movetogoal(10); self.health = self.frags; };\n";
    let keys = "\"flags\" \"1\"\n\"ideal_yaw\" \"0\"\n\"yaw_speed\" \"360\"\n";
    let entities = [
        entity("goal", "500 0 100", ""),
        entity("glider", "20 0 100", keys),
    ]
    .concat();
    let world = world(source, &entities);
    assert_eq!(world.vector(3, "origin")[0], 30.0);
}

fn run_on(spec: &MapSpec, source: &str, entities: &str) -> Running {
    running(spec, source, entities, Vec::new())
}

// spec PHYS-1.1: each frame runs StartFrame with time the server's
// spec PHYS-1.3: then the server's time advances by the frame
// spec PHYS-2.1: a think runs when nextthink is within the frame, at nextthink or, when that
// is past, now; nextthink 0 never thinks
#[test]
fn thinks_run_on_time() {
    let source = "float starts; float started_at;\n\
        void() StartFrame = { starts = starts + 1; started_at = time; };\n\
        void() thought = { self.frags = time; self.health = self.health + 1; };\n\
        void() thinker = { self.think = thought; self.nextthink = self.count; };\n\
        void() counter = { self.frags = starts; self.armorvalue = started_at; \
        self.nextthink = time + 0.05; };\n\
        void() watcher = { self.think = counter; self.nextthink = 1.0; };\n";
    let entities = [
        entity("thinker", "0 0 0", "\"count\" \"1.25\"\n"),
        entity("thinker", "0 0 0", "\"count\" \"0.5\"\n"),
        entity("thinker", "0 0 0", "\"count\" \"0\"\n"),
        entity("watcher", "0 0 0", ""),
    ]
    .concat();
    let mut run = run_on(&ledge(), source, &entities);
    run.frames(3, 0.1).unwrap();
    let w = &run.world;
    assert!((w.time() - 1.3).abs() < 1e-9, "{}", w.time());
    // 1.25 is within the frame at 1.2: it runs at 1.25.
    assert_eq!(w.float(2, "frags"), 1.25);
    assert_eq!(w.float(2, "health"), 1.0);
    assert_eq!(w.float(2, "nextthink"), 0.0);
    // 0.5 is past at 1.0: it runs at 1.0.
    assert_eq!(w.float(3, "frags"), 1.0);
    assert_eq!(w.float(4, "health"), 0.0);
}

// spec PHYS-1.2: while force_retouch is set every entity is linked touching triggers
// spec PHYS-1.3: force_retouch goes down by one each frame
#[test]
fn force_retouch_touches_every_frame() {
    let source = "void() worldspawn = { force_retouch = 2; };\n\
        void() pad_touch;\n\
        void() pad = { self.solid = 1; self.touch = pad_touch; \
        setsize(self, '-8 -8 -8', '8 8 8'); setorigin(self, self.origin); };\n\
        void() pad_touch = { self.frags = self.frags + 1; };\n\
        void() block = { self.solid = 2; setsize(self, '-8 -8 -8', '8 8 8'); \
        setorigin(self, self.origin); };\n";
    let entities = [entity("pad", "0 0 50", ""), entity("block", "4 0 50", "")].concat();
    let mut run = run_on(&ledge(), source, &entities);
    run.frames(6, 0.1).unwrap();
    assert_eq!(run.world.float(2, "frags"), 2.0);
}

// spec PHYS-1.2: a movetype no entity may have is id's Sys_Error, the movetype as an int
#[test]
fn a_bad_movetype_stops_the_server() {
    for (movetype, message) in [("3", "bad movetype 3"), ("1.5", "bad movetype 1")] {
        let source = "void() odd = { };\n";
        let keys = format!("\"movetype\" \"{movetype}\"\n");
        let mut run = run_on(&ledge(), source, &entity("odd", "0 0 0", &keys));
        let error = run.frames(1, 0.1).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
}

// spec PHYS-1.2: an entity spawned during the frame moves in the same frame
// spec PHYS-2.3: noclip moves by velocity and turns by avelocity, linking
#[test]
fn noclip_and_entities_spawned_mid_frame() {
    let source = "void() mover = { self.movetype = 8; self.velocity = '100 0 0'; \
        self.avelocity = '0 90 0'; self.solid = 2; };\n\
        void() maker = { local entity e; e = spawn(); e.movetype = 8; e.velocity = '0 50 0'; };\n\
        void() spawner = { self.think = maker; self.nextthink = 1.0; };\n";
    let entities = [
        entity("mover", "0 0 50", ""),
        entity("spawner", "0 0 0", ""),
    ]
    .concat();
    let mut run = run_on(&ledge(), source, &entities);
    run.frames(1, 0.1).unwrap();
    let w = &run.world;
    assert_eq!(w.vector(2, "origin"), [10.0, 0.0, 50.0]);
    assert_eq!(w.vector(2, "angles"), [0.0, 9.0, 0.0]);
    assert!(w.linked(2));
    assert_eq!(w.vector(4, "origin"), [0.0, 5.0, 0.0]);
}

// spec PHYS-4.1: a tossed entity on the ground does not move
// spec PHYS-4.2: else gravity pulls it and it is pushed by its velocity
// spec PHYS-4.3: on a floor it lands, its velocity cleared
// spec PHYS-6.4: gravity is 800, scaled by the progs' gravity field when it is not 0
#[test]
fn tossed_entities_fall_and_land() {
    let source = format!(
        "void() thrown = {{ self.movetype = 6; self.solid = 2; {PLAYER_SIZE} \
         setorigin(self, self.origin); }};\n"
    );
    let entities = [
        entity("thrown", "50 0 100", ""),
        entity("thrown", "-50 0 100", "\"gravity\" \"0.5\"\n"),
        entity("thrown", "-150 0 100", "\"flags\" \"512\"\n"),
    ]
    .concat();
    let mut run = run_on(&ledge(), &source, &entities);
    run.frames(1, 0.1).unwrap();
    let w = &run.world;
    // One frame: velocity -80, moved -8.
    assert_eq!(w.vector(2, "velocity")[2], -80.0);
    assert_eq!(w.vector(2, "origin")[2], 92.0);
    assert_eq!(w.vector(3, "velocity")[2], -40.0);
    assert_eq!(w.vector(4, "origin")[2], 100.0);
    run.frames(20, 0.1).unwrap();
    let w = &run.world;
    assert_eq!(w.float(2, "flags"), 512.0);
    assert_eq!(w.vector(2, "velocity"), [0.0; 3]);
    assert!((w.vector(2, "origin")[2] - 24.031_25).abs() < 1e-3);
    assert_eq!(w.entity_field(2, "groundentity"), 0);
}

// spec PHYS-4.3: a bouncer leaves the floor with 1.5 times its speed into it, and lands only
// when slower than 60
// spec PHYS-6.2: the velocity is the incoming less the normal times the backoff
#[test]
fn bouncers_bounce() {
    let source = format!(
        "void() ball = {{ self.movetype = 10; self.solid = 2; {PLAYER_SIZE} \
         self.velocity = '0 0 -400'; setorigin(self, self.origin); }};\n"
    );
    let mut run = run_on(&ledge(), &source, &entity("ball", "50 0 40", ""));
    run.frames(1, 0.1).unwrap();
    let w = &run.world;
    // -400 - 80 into the floor, back out at half of it.
    assert_eq!(w.vector(2, "velocity")[2], 240.0);
    assert_eq!(w.float(2, "flags"), 0.0);
}

// spec PHYS-4.2: a flyer feels no gravity
// spec PHYS-6.3: velocity is bounded to 2000 on each axis
#[test]
fn flyers_ignore_gravity_and_speed_is_bounded() {
    let source = "void() dart = { self.movetype = 5; self.velocity = '3000 0 0'; };\n";
    let mut run = run_on(&ledge(), source, &entity("dart", "-500 0 100", ""));
    run.frames(1, 0.1).unwrap();
    let w = &run.world;
    assert_eq!(w.vector(2, "velocity"), [2000.0, 0.0, 0.0]);
    assert_eq!(w.vector(2, "origin"), [-300.0, 0.0, 100.0]);
}

// spec PHYS-4.4: after a move that hit something the water is checked: first the contents
// become the watertype, then out of liquid the watertype is empty and the waterlevel the
// contents; a move that hit nothing checks nothing
#[test]
fn water_is_checked_after_an_impact() {
    let source = format!(
        "void() ball = {{ self.movetype = 10; self.solid = 2; {PLAYER_SIZE} \
         self.velocity = '0 0 -400'; setorigin(self, self.origin); }};\n\
         void() dart = {{ self.movetype = 5; self.velocity = '10 0 0'; }};\n"
    );
    let entities = [
        entity("ball", "50 0 40", ""),
        entity("dart", "-500 0 100", ""),
    ]
    .concat();
    let mut run = run_on(&ledge(), &source, &entities);
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.world.float(2, "watertype"), -1.0);
    assert_eq!(run.world.float(2, "waterlevel"), 1.0);
    assert_eq!(run.world.float(3, "watertype"), 0.0);
    // Up and down again: the second impact.
    let mut frames = 0;
    while run.world.float(2, "waterlevel") > 0.0 && frames < 30 {
        run.frames(1, 0.1).unwrap();
        frames += 1;
    }
    assert_eq!(run.world.float(2, "watertype"), -1.0);
    assert_eq!(run.world.float(2, "waterlevel"), -1.0);
}

// spec PHYS-5.1: a stepping monster off the ground falls by sliding, lands and links
// spec PHYS-5.2: falling or not, it thinks
// spec PHYS-7.1: a slide moves the whole way when nothing is hit
// spec PHYS-7.2: a floor of the world sets FL_ONGROUND
#[test]
fn stepping_monsters_fall_to_the_floor() {
    let source = format!(
        "void() landed = {{ self.health = 1; }};\n\
         void() monster = {{ self.movetype = 4; self.solid = 3; {PLAYER_SIZE} \
         setorigin(self, self.origin); self.think = landed; self.nextthink = 1.5; }};\n"
    );
    let mut run = run_on(&ledge(), &source, &entity("monster", "50 0 100", ""));
    run.frames(10, 0.1).unwrap();
    let w = &run.world;
    assert_eq!(w.float(2, "flags"), 512.0);
    assert!((w.vector(2, "origin")[2] - 24.031_25).abs() < 1e-3);
    assert_eq!(w.float(2, "health"), 1.0);
    let sounds = run.world.take_sounds();
    assert!(
        sounds
            .iter()
            .any(|s| s.sample == "demon/dland2.wav" && s.entity == 2),
        "{sounds:?}"
    );
}

// spec PHYS-7.3: hitting a floor at an angle slides along it
// spec PHYS-7.4: and keeps going while the slide is along the first velocity
#[test]
fn a_fall_slides_along_the_floor() {
    let source = format!(
        "void() monster = {{ self.movetype = 4; self.solid = 3; {PLAYER_SIZE} \
         self.velocity = '200 0 -400'; setorigin(self, self.origin); }};\n"
    );
    let mut run = run_on(&ledge(), &source, &entity("monster", "0 0 40", ""));
    run.frames(1, 0.1).unwrap();
    let [x, _, z] = run.world.vector(2, "origin");
    assert!((z - 24.031_25).abs() < 1e-3, "{z}");
    assert!((x - 20.0).abs() < 1e-3, "{x}");
    assert_eq!(run.world.vector(2, "velocity")[2], 0.0);
}

// spec PHYS-6.1: an impact runs both entities' touches, each with the other as other
// spec PHYS-3.6: a pushed entity's trace end is its origin, and what it hits it touches
#[test]
fn impacts_touch_both_ways() {
    let source = format!(
        "void() hit_touch = {{ self.frags = self.frags + 1; other.health = 7; }};\n\
         void() rock = {{ self.movetype = 6; self.solid = 2; {PLAYER_SIZE} self.touch = hit_touch; \
         setorigin(self, self.origin); }};\n\
         void() crate = {{ self.solid = 2; {PLAYER_SIZE} self.touch = hit_touch; \
         setorigin(self, self.origin); }};\n"
    );
    let entities = [
        entity("crate", "50 0 24", ""),
        entity("rock", "50 0 85", ""),
    ]
    .concat();
    let mut run = run_on(&ledge(), &source, &entities);
    run.frames(3, 0.1).unwrap();
    let w = &run.world;
    assert!(w.float(3, "frags") >= 1.0);
    assert!(w.float(2, "frags") >= 1.0);
    assert_eq!(w.float(2, "health"), 7.0);
    assert_eq!(w.float(3, "health"), 7.0);
}

/// A pusher: the box BSP moving up at 100, thinking at `stop`, with a rider on it.
fn lift(spec: &MapSpec, velocity: &str, stop: &str, rider_z: &str, more: &str) -> Running {
    let crate_box = MapSpec {
        planes: vec![
            plane([0.0, 0.0, 1.0], 10.0, 2),
            plane([0.0, 0.0, 1.0], 34.0, 2),
        ],
        nodes: vec![node(0, [-2, -1])],
        ..MapSpec::default()
    };
    let bytes = bsp_file(&crate_box, [-8.0; 3], [8.0; 3]);
    let source = format!(
        "void() worldspawn = {{ precache_model(\"maps/b_box.bsp\"); }};\n\
         void() lift_stop = {{ self.velocity = '0 0 0'; self.frags = self.ltime; }};\n\
         void() lift_blocked = {{ self.health = self.health + 1; }};\n\
         void() lift = {{ self.solid = 4; self.movetype = 7; setmodel(self, \"maps/b_box.bsp\"); \
         setorigin(self, self.origin); self.velocity = '{velocity}'; self.think = lift_stop; \
         self.nextthink = {stop}; self.blocked = lift_blocked; }};\n\
         void() rider = {{ self.solid = 3; self.movetype = 4; {PLAYER_SIZE} \
         setorigin(self, self.origin); droptofloor(); }};\n\
         void() stuck = {{ self.solid = 3; self.movetype = 4; self.flags = 512; {PLAYER_SIZE} \
         setorigin(self, self.origin); }};\n\
         void() beam = {{ self.solid = 2; setsize(self, '-20 -20 -4', '20 20 4'); \
         setorigin(self, self.origin); }};\n"
    );
    let entities = [
        entity("lift", "300 0 50", ""),
        entity("rider", &format!("300 0 {rider_z}"), ""),
        more.to_owned(),
    ]
    .concat();
    running(
        spec,
        &source,
        &entities,
        vec![("maps/b_box.bsp".to_owned(), bytes)],
    )
}

// spec PHYS-3.1: a pusher moves until its think, whose time is its own ltime
// spec PHYS-3.2: it moves by velocity times the move's time
// spec PHYS-3.3: what stands on it moves with it
// spec PHYS-3.4: a moved entity loses FL_ONGROUND and is pushed by the same move
#[test]
fn a_lift_carries_its_rider() {
    let mut run = lift(&ledge(), "0 0 100", "0.3", "200", "");
    let before = run.world.vector(3, "origin")[2];
    assert!((before - 84.031_25).abs() < 1e-3, "{before}");
    run.frames(5, 0.1).unwrap();
    let w = &run.world;
    // ltime starts at 0: 0.3 of rising at 100, then it stops.
    assert!((w.vector(2, "origin")[2] - 80.0).abs() < 1e-3);
    assert!((w.float(2, "frags") - 0.3).abs() < 1e-6);
    // After the think, nextthink 0 makes the move's time 0: ltime stays.
    assert!((w.float(2, "ltime") - 0.3).abs() < 1e-6);
    assert!((w.vector(3, "origin")[2] - (before + 30.0)).abs() < 1e-2);
}

// spec PHYS-3.5: a pusher that would crush something goes back, runs blocked, and puts back
// what it moved
#[test]
fn a_blocked_lift_goes_back() {
    // The rider's head is at 116; the beam's underside 1.5 above it.
    let mut run = lift(
        &ledge(),
        "0 0 100",
        "5",
        "200",
        &entity("beam", "300 0 121.5", ""),
    );
    let rider = run.world.vector(3, "origin");
    run.frames(2, 0.1).unwrap();
    let w = &run.world;
    assert!(w.float(2, "health") >= 1.0);
    // The lift is back where it started and its clock with it; the rider too.
    assert_eq!(w.vector(2, "origin")[2], 50.0);
    assert_eq!(w.float(2, "ltime"), 0.0);
    assert_eq!(w.vector(3, "origin"), rider);
}

// spec PHYS-8.1: an unconnected client slot does nothing
// spec PHYS-2.2: MOVETYPE_NONE only thinks
#[test]
fn client_slots_and_none() {
    let source = "void() still = { self.velocity = '100 0 0'; self.think = still; \
        self.nextthink = time + 0.1; self.frags = self.frags + 1; };\n";
    let mut run = run_on(&ledge(), source, &entity("still", "0 0 50", ""));
    run.frames(3, 0.1).unwrap();
    let w = &run.world;
    assert_eq!(w.vector(2, "origin"), [0.0, 0.0, 50.0]);
    assert!(w.float(2, "frags") >= 3.0);
}

impl Running {
    /// The player enters the game.
    fn connect(&mut self) {
        let progs = Progs::read(&self.progs).unwrap();
        self.world.connect(&progs).unwrap();
    }
}

/// A connected player on `spec`: `PutClientInServer` makes a walking, living player of the
/// player's size and runs `put`; `PlayerPreThink` runs `pre`. `more` is further source.
fn player(spec: &MapSpec, put: &str, pre: &str, more: &str) -> Running {
    let source = format!(
        "{more}void() PutClientInServer = {{ self.health = 100; self.solid = 3; \
         self.movetype = 3; self.view_ofs = '0 0 22'; {PLAYER_SIZE} {put} }};\n\
         void() PlayerPreThink = {{ {pre} }};\n"
    );
    let mut run = run_on(spec, &source, "");
    run.connect();
    run
}

/// The player with the command `cmd` after `count` frames of `frametime`.
fn moved(run: &mut Running, cmd: UserCmd, count: usize, frametime: f64) -> &World {
    run.world.set_command(cmd);
    run.frames(count, frametime).unwrap();
    &run.world
}

const P: usize = 1;

fn close(got: f32, want: f32) -> bool {
    (got - want).abs() < 1e-3
}

// spec PHYS-10.1: connecting runs SetNewParms, clears the client's edict, names it, puts the
// parms back and runs ClientConnect then PutClientInServer with self the client
#[test]
fn connecting_runs_the_entry_points() {
    let source = "void() SetNewParms = { parm1 = 5; parm16 = 7; };\n\
        void() ClientConnect = { self.frags = parm1 + parm16; self.ammo_shells = time; \
        if (self.netname == \"player\") self.ammo_nails = 1; };\n\
        void() PutClientInServer = { self.ammo_nails = self.ammo_nails * 10 + 2; };\n";
    let mut run = run_on(&ledge(), source, "");
    run.world
        .machine
        .edicts_mut()
        .set_field(P, field_slot("armorvalue"), Slot::from_f32(9.0));
    assert!(run.world.player_view().is_none());
    run.connect();
    let w = &run.world;
    assert_eq!(w.float(P, "armorvalue"), 0.0);
    assert_eq!(w.float(P, "colormap"), 1.0);
    assert_eq!(w.float(P, "team"), 1.0);
    assert_eq!(w.float(P, "frags"), 12.0);
    assert_eq!(f64::from(w.float(P, "ammo_shells")), w.time());
    assert_eq!(w.float(P, "ammo_nails"), 12.0);
    assert!(w.player_view().is_some());
}

// spec PHYS-10.2: while fixangle is set the client's angles go to the client through
// MSG_WriteAngle, once, and fixangle is cleared
// spec PHYS-9.1: with fixangle set the pitch and yaw are kept
#[test]
fn fixangle_turns_the_view_once() {
    let mut run = player(
        &ledge(),
        "self.angles = '0 100 0'; self.fixangle = 1;",
        "",
        "",
    );
    let w = moved(&mut run, UserCmd::default(), 1, 0.1);
    assert_eq!(w.vector(P, "angles")[1], 100.0);
    assert_eq!(w.float(P, "fixangle"), 0.0);
    assert_eq!(run.world.take_set_angle(), Some([0.0, 99.843_75, 0.0]));
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.world.take_set_angle(), None);
    // Unfixed, the yaw follows the view.
    assert_eq!(run.world.vector(P, "angles")[1], 0.0);
}

// spec PHYS-10.3: a server frame sets the frametime global before the clients move
#[test]
fn the_frame_sets_frametime() {
    let mut run = player(&ledge(), "", "self.ammo_shells = frametime;", "");
    let w = moved(&mut run, UserCmd::default(), 1, 0.05);
    assert_eq!(w.float(P, "ammo_shells"), 0.05);
}

// spec PHYS-9.6: the view angles become v_angle as the wire carries them; the buttons and a
// non-zero impulse are set
#[test]
fn the_command_sets_the_view_and_buttons() {
    let mut run = player(&ledge(), "self.health = 0;", "", "");
    let cmd = UserCmd {
        angles: [10.0, -10.0, 350.0],
        buttons: 3,
        impulse: 5,
        ..UserCmd::default()
    };
    let w = moved(&mut run, cmd, 1, 0.1);
    assert_eq!(w.vector(P, "v_angle"), [9.843_75, -9.843_75, -11.25]);
    assert_eq!((w.float(P, "button0"), w.float(P, "button2")), (1.0, 1.0));
    assert_eq!(w.float(P, "impulse"), 5.0);
    let cmd = UserCmd {
        buttons: 2,
        ..UserCmd::default()
    };
    let w = moved(&mut run, cmd, 1, 0.1);
    assert_eq!((w.float(P, "button0"), w.float(P, "button2")), (0.0, 1.0));
    assert_eq!(w.float(P, "impulse"), 5.0);
}

/// A player standing on the ledge at `origin`, already moving at `velocity`.
fn standing(spec: &MapSpec, origin: &str, velocity: &str, more: &str) -> Running {
    let put = format!(
        "setorigin(self, '{origin}'); self.velocity = '{velocity}'; self.flags = 512; {more}"
    );
    player(spec, &put, "", "")
}

fn forward(speed: f32) -> UserCmd {
    UserCmd {
        forward: speed,
        ..UserCmd::default()
    }
}

// spec PHYS-9.2: on the ground the wish is forward times the forward move, bounded to 320,
// with no z for a walker
// spec PHYS-9.4: acceleration adds up to sv_accelerate * frametime * wishspeed toward it
#[test]
fn the_player_accelerates_on_the_ground() {
    let mut run = standing(&ledge(), "0 0 24.03125", "0 0 0", "");
    let w = moved(&mut run, forward(200.0), 1, 0.1);
    assert!(
        close(w.vector(P, "velocity")[0], 200.0),
        "{:?}",
        w.vector(P, "velocity")
    );
    assert!(close(w.vector(P, "origin")[0], 20.0));
    let mut run = standing(&ledge(), "0 0 24.03125", "0 0 0", "");
    let cmd = UserCmd {
        up: 300.0,
        ..forward(1000.0)
    };
    let w = moved(&mut run, cmd, 1, 0.1);
    assert!(close(w.vector(P, "velocity")[0], 320.0));
    assert_eq!(w.vector(P, "velocity")[2], 0.0);
    // Half the way there, acceleration is a step of 10 * 0.05 * 200.
    let mut run = standing(&ledge(), "0 0 24.03125", "0 0 0", "");
    let w = moved(&mut run, forward(200.0), 1, 0.05);
    assert!(close(w.vector(P, "velocity")[0], 100.0));
}

// spec PHYS-9.3: friction takes frametime * max(speed, 100) * 4, twice that over an edge,
// not below 0
#[test]
fn friction_stops_the_player() {
    let speed = |spec: &MapSpec, origin: &str, velocity: &str| {
        let mut run = standing(spec, origin, velocity, "");
        moved(&mut run, UserCmd::default(), 1, 0.1).vector(P, "velocity")[0]
    };
    assert!(close(speed(&ledge(), "0 0 24.03125", "200 0 0"), 120.0));
    // Below sv_stopspeed the control is 100.
    assert!(close(speed(&ledge(), "0 0 24.03125", "50 0 0"), 10.0));
    assert_eq!(speed(&ledge(), "0 0 24.03125", "20 0 0"), 0.0);
    // 16 ahead is past the edge at 100, and nothing is within 34 below it.
    assert!(close(
        speed(&ledge_over(100.0), "90 0 24.03125", "100 0 0"),
        20.0
    ));
    assert!(close(
        speed(&ledge_over(20.0), "90 0 24.03125", "100 0 0"),
        60.0
    ));
}

// spec PHYS-9.2: off the ground only air acceleration applies; going back is cut while
// teleport_time is ahead; noclip takes the wish, with up, as its velocity
// spec PHYS-9.4: air acceleration aims for at most 30, by sv_accelerate * wishspeed * frametime
// spec PHYS-8.2: a noclip player moves by frametime * velocity
#[test]
fn air_teleport_and_noclip_moves() {
    let mut run = player(&ledge(), "setorigin(self, '0 0 200');", "", "");
    let w = moved(&mut run, forward(200.0), 1, 0.1);
    assert!(close(w.vector(P, "velocity")[0], 30.0));
    let mut run = player(
        &ledge(),
        "setorigin(self, '0 0 200'); self.velocity = '40 0 0';",
        "",
        "",
    );
    assert!(close(
        moved(&mut run, forward(200.0), 1, 0.1).vector(P, "velocity")[0],
        40.0
    ));
    // A slow wish: the step is 10 * 2 * 0.01.
    let mut run = player(&ledge(), "setorigin(self, '0 0 200');", "", "");
    assert!(close(
        moved(&mut run, forward(2.0), 1, 0.01).vector(P, "velocity")[0],
        0.2
    ));
    let mut run = standing(
        &ledge(),
        "0 0 24.03125",
        "0 0 0",
        "self.teleport_time = 100;",
    );
    assert_eq!(
        moved(&mut run, forward(-200.0), 1, 0.1).vector(P, "velocity")[0],
        0.0
    );
    let mut run = player(
        &ledge(),
        "setorigin(self, '0 0 200'); self.movetype = 8;",
        "",
        "",
    );
    let cmd = UserCmd {
        up: 100.0,
        ..UserCmd::default()
    };
    let w = moved(&mut run, cmd, 1, 0.1);
    assert_eq!(w.vector(P, "velocity"), [0.0, 0.0, 100.0]);
    assert!(close(w.vector(P, "origin")[2], 210.0));
}

// spec PHYS-9.1: the punch fades by 10 a second; the roll is 4 times V_CalcRoll of the
// velocity along the right; the pitch is a third of the view's, negated, and the yaw the
// view's, both with the punch; a dead player only fades its punch, and MOVETYPE_NONE nothing
#[test]
fn view_angles_roll_and_punch() {
    let put = "setorigin(self, '0 0 200'); self.punchangle = '10 0 0'; self.velocity = '0 -100 0';";
    let cmd = UserCmd {
        angles: [30.0, 45.0, 0.0],
        ..UserCmd::default()
    };
    let mut run = player(&ledge(), put, "", "");
    let w = moved(&mut run, cmd, 1, 0.1);
    assert!(close(w.vector(P, "punchangle")[0], 9.0));
    let [pitch, yaw, roll] = w.vector(P, "angles");
    assert!(close(pitch, -(29.531_25 + 9.0) / 3.0), "{pitch}");
    assert!(close(yaw, 45.0));
    assert!(close(roll, 4.0), "{roll}");
    let mut run = player(
        &ledge(),
        &format!("{put} self.velocity = '0 300 0';"),
        "",
        "",
    );
    assert!(close(
        moved(&mut run, cmd, 1, 0.1).vector(P, "angles")[2],
        -8.0
    ));
    let mut run = player(&ledge(), &format!("{put} self.health = 0;"), "", "");
    let w = moved(&mut run, cmd, 1, 0.1);
    assert!(close(w.vector(P, "punchangle")[0], 9.0));
    assert_eq!(w.vector(P, "angles"), [0.0; 3]);
    let mut run = player(&ledge(), &format!("{put} self.movetype = 0;"), "", "");
    assert_eq!(
        moved(&mut run, cmd, 1, 0.1).vector(P, "punchangle")[0],
        10.0
    );
}

/// Water from z = 0 to 40 over a floor at 0; for the player's hull, the floor at 24.
fn pool() -> MapSpec {
    MapSpec {
        planes: vec![
            plane([0.0, 0.0, 1.0], 40.0, 2),
            plane([0.0, 0.0, 1.0], 0.0, 2),
            plane([0.0, 0.0, 1.0], 24.0, 2),
        ],
        nodes: vec![node(0, [-2, 1]), node(1, [-3, -1])],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY, CONTENTS_WATER],
        clipnodes: vec![ClipNode {
            planenum: 2,
            children: [CONTENTS_EMPTY, CONTENTS_SOLID],
        }],
    }
}

// spec PHYS-8.4: the waterlevel is 1 for liquid 1 above the feet, 2 at the middle, 3 at the
// eyes, with the watertype the feet's contents
// spec PHYS-8.2: a walker waist deep has no gravity
#[test]
fn water_levels() {
    let level = |origin: &str, eyes: &str| {
        let put =
            format!("setorigin(self, '{origin}'); self.view_ofs = '0 0 {eyes}'; self.health = 0;");
        let mut run = player(&pool(), &put, "", "");
        let w = moved(&mut run, UserCmd::default(), 1, 0.1);
        (
            w.float(P, "waterlevel"),
            w.float(P, "watertype"),
            w.vector(P, "velocity")[2],
        )
    };
    assert_eq!(level("0 0 30", "22"), (2.0, -3.0, 0.0));
    assert_eq!(level("0 0 30", "8"), (3.0, -3.0, 0.0));
    let (one, kind, fall) = level("0 0 37", "22");
    assert_eq!((one, kind), (1.0, -3.0));
    assert!(close(fall, -80.0));
    let (dry, kind, _) = level("0 0 100", "22");
    assert_eq!((dry, kind), (0.0, -1.0));
}

// spec PHYS-9.5: a swimmer with no move sinks toward 60 * 0.7; one moving loses
// frametime * speed * 4 and gains toward 0.7 of its wish along v_angle
// spec PHYS-9.1: a player waist deep swims
#[test]
fn swimming() {
    let put = "setorigin(self, '0 0 30'); self.waterlevel = 2;";
    let mut run = player(&pool(), put, "", "");
    let w = moved(&mut run, UserCmd::default(), 1, 0.1);
    assert!(
        close(w.vector(P, "velocity")[2], -42.0),
        "{:?}",
        w.vector(P, "velocity")
    );
    let mut run = player(
        &pool(),
        &format!("{put} self.velocity = '100 0 0';"),
        "",
        "",
    );
    let w = moved(&mut run, forward(100.0), 1, 0.1);
    assert!(close(w.vector(P, "velocity")[0], 70.0));
    // Faster than the wish: only the water's friction.
    let mut run = player(
        &pool(),
        &format!("{put} self.velocity = '300 0 0';"),
        "",
        "",
    );
    assert!(close(
        moved(&mut run, forward(100.0), 1, 0.1).vector(P, "velocity")[0],
        180.0
    ));
    let cmd = UserCmd {
        up: 100.0,
        ..UserCmd::default()
    };
    let mut run = player(&pool(), put, "", "");
    assert!(close(
        moved(&mut run, cmd, 1, 0.1).vector(P, "velocity")[2],
        70.0
    ));
}

// spec PHYS-9.5: a water jump keeps the velocity's x and y at movedir's, and ends once
// teleport_time is past or the player is out of water
// spec PHYS-8.2: with FL_WATERJUMP a walker has no gravity
#[test]
fn water_jumps() {
    let jump = |teleport: &str| {
        let put = format!(
            "setorigin(self, '0 0 100'); self.flags = 2048; self.waterlevel = 1; \
             self.movedir = '50 0 0'; self.velocity = '0 0 300'; self.teleport_time = {teleport};"
        );
        let mut run = player(&ledge(), &put, "", "");
        let w = moved(&mut run, UserCmd::default(), 1, 0.1);
        (
            w.vector(P, "velocity"),
            w.float(P, "flags"),
            w.float(P, "teleport_time"),
        )
    };
    let (velocity, flags, teleport) = jump("time + 5");
    assert_eq!(velocity, [50.0, 0.0, 300.0]);
    assert_eq!(flags, 2048.0);
    assert!(teleport > 5.0);
    let (velocity, flags, teleport) = jump("0");
    assert_eq!((velocity[0], flags, teleport), (50.0, 0.0, 0.0));
    assert!(close(velocity[2], 220.0));
}

// spec PHYS-8.1: a connected client runs PlayerPreThink, has its velocity bounded, moves,
// touches triggers and runs PlayerPostThink, each think with self the client and time now
#[test]
fn the_client_thinks_around_its_move() {
    let more = "void() PlayerPostThink = { self.armorvalue = self.origin * '1 0 0'; \
        self.ammo_nails = time; };\n\
        void() pad_touch = { other.ammo_shells = other.ammo_shells + 1; };\n\
        void() worldspawn = { local entity pad; pad = spawn(); pad.solid = 1; \
        pad.touch = pad_touch; setsize(pad, '-8 -8 -8', '8 8 8'); setorigin(pad, '200 0 200'); };\n";
    let put = "setorigin(self, '0 0 200'); self.movetype = 8;";
    let pre = "self.frags = self.frags + 1; self.velocity = '5000 0 0'; self.health = time;";
    let mut run = player(&ledge(), put, pre, more);
    let start = run.world.time();
    let w = moved(&mut run, UserCmd::default(), 1, 0.1);
    assert_eq!(w.float(P, "frags"), 1.0);
    assert_eq!(f64::from(w.float(P, "health")), start);
    assert_eq!(w.vector(P, "velocity")[0], 2000.0);
    assert!(close(w.vector(P, "origin")[0], 200.0));
    assert!(close(w.float(P, "armorvalue"), 200.0));
    assert_eq!(f64::from(w.float(P, "ammo_nails")), start);
    assert_eq!(w.float(P, "ammo_shells"), 1.0);
    assert!(w.linked(P));
}

// spec PHYS-8.2: MOVETYPE_NONE thinks, toss falls, fly slides without gravity, and any other
// movetype is Sys_Error
#[test]
fn client_movetypes() {
    let more = "void() thought = { self.frags = 3; };\n";
    let put = |movetype: &str| {
        format!(
            "setorigin(self, '0 0 200'); self.movetype = {movetype}; self.velocity = '10 0 0'; \
             self.think = thought; self.nextthink = time + 0.05;"
        )
    };
    let mut run = player(&ledge(), &put("0"), "", more);
    let w = moved(&mut run, UserCmd::default(), 1, 0.1);
    assert_eq!(w.float(P, "frags"), 3.0);
    assert_eq!(w.vector(P, "origin"), [0.0, 0.0, 200.0]);
    let mut run = player(&ledge(), &put("6"), "", more);
    let w = moved(&mut run, UserCmd::default(), 1, 0.1);
    assert!(
        close(w.vector(P, "origin")[2], 192.0),
        "{:?}",
        w.vector(P, "origin")
    );
    let mut run = player(&ledge(), &put("5"), "", more);
    let w = moved(&mut run, UserCmd::default(), 1, 0.1);
    assert_eq!(w.float(P, "frags"), 3.0);
    assert!(close(w.vector(P, "origin")[2], 200.0));
    assert!(w.vector(P, "origin")[0] > 0.9);
    let mut run = player(&ledge(), &put("7"), "", more);
    run.world.set_command(UserCmd::default());
    let error = run.frames(1, 0.1).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("SV_Physics_client: bad movetype 7"),
        "{error}"
    );
}

/// A dead player in the pit of `spec` at `origin`, its velocity and flags set each frame by
/// the think before its move.
fn pushed(spec: &MapSpec, origin: &str, velocity: &str, flags: &str) -> Running {
    let put = format!("setorigin(self, '{origin}'); self.health = 0;");
    let pre = format!("self.velocity = '{velocity}'; self.flags = {flags};");
    player(spec, &put, &pre, "")
}

// spec PHYS-8.3: a walk blocked by a step goes up 18, across and down onto the floor; not
// while off the ground and out of water
#[test]
fn the_player_climbs_steps() {
    let step = |flags: &str| {
        let mut run = pushed(&ledge_over(10.0), "120 0 14.03125", "-200 0 0", flags);
        moved(&mut run, UserCmd::default(), 1, 0.1).vector(P, "origin")
    };
    let [x, _, z] = step("512");
    assert!(close(x, 100.0), "{x}");
    assert!(close(z, 24.031_25), "{z}");
    let [x, _, z] = step("0");
    assert!(close(x, 116.031_25), "{x}");
    assert!(z < 15.0);
}

// spec PHYS-8.3: when the step's push down does not reach a floor, the move without the
// step is kept
#[test]
fn a_step_that_finds_no_floor_is_undone() {
    let mut run = pushed(&ledge_over(50.0), "120 0 -25.96875", "-200 0 100", "512");
    let w = moved(&mut run, UserCmd::default(), 1, 0.1);
    assert!(
        close(w.vector(P, "velocity")[2], 20.0),
        "{:?}",
        w.vector(P, "velocity")
    );
    assert!(close(w.vector(P, "origin")[0], 116.031_25));
}

// spec PHYS-8.5: facing a wall it walks into slows the player along it by 1 + d
#[test]
fn walls_slow_the_player_facing_them() {
    let along = |yaw: f32| {
        let mut run = pushed(&ledge_over(50.0), "120 0 -25.96875", "-100 100 0", "512");
        let cmd = UserCmd {
            angles: [0.0, yaw, 0.0],
            ..UserCmd::default()
        };
        moved(&mut run, cmd, 1, 0.1).vector(P, "velocity")[1]
    };
    assert!(close(along(180.0), 50.0), "{}", along(180.0));
    assert!(close(along(0.0), 100.0), "{}", along(0.0));
}

// spec PHYS-8.6: a step that makes no progress is unstuck by pushes of 2, keeping the first
// that moves the player more than 4; when none does it is stopped
#[test]
fn a_player_stuck_on_a_step_is_pushed_free() {
    let mut run = pushed(
        &ledge_over(50.0),
        "116.03125 100 -25.96875",
        "-100 30 -100",
        "512",
    );
    let y = moved(&mut run, UserCmd::default(), 1, 0.001).vector(P, "origin")[1];
    assert!(y > 104.0 && y < 106.0, "{y}");
    // Going the other way only the fourth push, 2 toward -y, frees it.
    let mut run = pushed(
        &ledge_over(50.0),
        "116.03125 100 -25.96875",
        "-100 -30 -100",
        "512",
    );
    let y = moved(&mut run, UserCmd::default(), 1, 0.001).vector(P, "origin")[1];
    assert!(y > 94.0 && y < 96.0, "{y}");
    let mut run = pushed(
        &ledge_over(50.0),
        "116.03125 0 -25.96875",
        "-100 0 -100",
        "512",
    );
    let w = moved(&mut run, UserCmd::default(), 1, 0.001);
    assert_eq!(w.vector(P, "velocity"), [0.0; 3]);
    assert!(close(w.vector(P, "origin")[1], 0.0));
}

// spec PHYS-8.7: a player out of solid keeps its origin as oldorigin; one in solid goes back
// to oldorigin, else to the first free spot -1 to 1 across and 0 to 17 up; else it stays
#[test]
fn a_player_in_solid_is_unstuck() {
    let stuck = |origin: &str, old: &str| {
        let put =
            format!("setorigin(self, '{origin}'); self.oldorigin = '{old}'; self.health = 0;");
        let mut run = player(&ledge(), &put, "", "");
        let w = moved(&mut run, UserCmd::default(), 1, 0.01);
        (w.vector(P, "origin"), w.vector(P, "oldorigin"))
    };
    let (origin, old) = stuck("50 0 100", "0 0 0");
    assert_eq!(old, [50.0, 0.0, 100.0]);
    assert!(close(origin[2], 99.92));
    let (origin, _) = stuck("50 0 20", "60 5 40");
    assert_eq!((origin[0], origin[1]), (60.0, 5.0));
    let (origin, _) = stuck("50 0 20", "0 0 0");
    assert_eq!((origin[0], origin[1]), (49.0, -1.0));
    assert!(close(origin[2], 24.0), "{origin:?}");
    let (origin, _) = stuck("50 0 -100", "0 0 0");
    assert_eq!(origin, [50.0, 0.0, -100.0]);
}

/// The ledge turned to face y: the floor for y below 100, the pit past it.
fn ledge_y(pit: f32) -> MapSpec {
    let mut spec = ledge_over(pit);
    for n in [0, 3] {
        let dist = spec.planes[n].dist;
        spec.planes[n] = plane([0.0, 1.0, 0.0], dist, 1);
    }
    spec
}

// spec PHYS-9.6: the moves arrive as whole numbers; a button bit sets only its own button
// spec PHYS-9.2: the wish adds the side move along the right; noclip takes the bounded wish
#[test]
fn moves_arrive_whole_and_sideways() {
    let mut run = player(
        &ledge(),
        "setorigin(self, '0 0 200'); self.movetype = 8;",
        "",
        "",
    );
    let cmd = UserCmd {
        forward: 100.9,
        side: 30.9,
        up: 50.9,
        buttons: 1,
        ..UserCmd::default()
    };
    let w = moved(&mut run, cmd, 1, 0.1);
    let [x, y, z] = w.vector(P, "velocity");
    assert!(
        close(x, 100.0) && close(y, -30.0) && close(z, 50.0),
        "{x} {y} {z}"
    );
    assert_eq!((w.float(P, "button0"), w.float(P, "button2")), (1.0, 0.0));
    let mut run = player(
        &ledge(),
        "setorigin(self, '0 0 200'); self.movetype = 8;",
        "",
        "",
    );
    let w = moved(&mut run, forward(1000.0), 1, 0.1);
    assert!(close(w.vector(P, "velocity")[0], 320.0));
    // Side steps on the ground.
    let mut run = standing(&ledge(), "0 0 24.03125", "0 0 0", "");
    let cmd = UserCmd {
        side: 100.0,
        ..UserCmd::default()
    };
    assert!(close(
        moved(&mut run, cmd, 1, 0.1).vector(P, "velocity")[1],
        -100.0
    ));
}

// spec PHYS-9.2: a backward move is only cut while teleport_time is ahead
// spec PHYS-9.4: acceleration adds the wish less the speed already along it
#[test]
fn acceleration_counts_the_speed_already_made() {
    let mut run = standing(
        &ledge(),
        "0 0 24.03125",
        "0 0 0",
        "self.teleport_time = 0.5;",
    );
    assert!(close(
        moved(&mut run, forward(-200.0), 1, 0.1).vector(P, "velocity")[0],
        -200.0
    ));
    // 100 less friction is 60; 140 more makes the wish.
    let mut run = standing(&ledge(), "0 0 24.03125", "100 0 0", "");
    assert!(close(
        moved(&mut run, forward(200.0), 1, 0.1).vector(P, "velocity")[0],
        200.0
    ));
}

// spec PHYS-9.3: the edge is looked for 16 ahead along the horizontal velocity, both axes,
// and the speed counts both
#[test]
fn edge_friction_looks_along_the_velocity() {
    let speed = |spec: &MapSpec, origin: &str, velocity: &str| {
        let mut run = standing(spec, origin, velocity, "");
        let [x, y, _] = moved(&mut run, UserCmd::default(), 1, 0.1).vector(P, "velocity");
        (x * x + y * y).sqrt()
    };
    // 9.6 ahead of 86 is short of the edge; 16 would not be.
    assert!(close(
        speed(&ledge_over(100.0), "86 0 24.03125", "60 80 0"),
        60.0
    ));
    assert!(close(
        speed(&ledge_over(100.0), "95 0 24.03125", "60 80 0"),
        20.0
    ));
    assert!(close(
        speed(&ledge_y(100.0), "0 86 24.03125", "80 60 0"),
        60.0
    ));
    assert!(close(
        speed(&ledge_y(100.0), "0 95 24.03125", "80 60 0"),
        20.0
    ));
}

// spec PHYS-9.5: a swim wish is bounded to 320 before its 0.7, sideways along the right, and
// the step is sv_accelerate * wishspeed * frametime
#[test]
fn swimming_bounds_and_steps() {
    let put = "setorigin(self, '0 0 30'); self.waterlevel = 2;";
    let mut run = player(&pool(), put, "", "");
    let w = moved(&mut run, forward(1000.0), 1, 0.1);
    let [x, y, z] = w.vector(P, "velocity");
    assert!(
        close(x, 224.0) && close(y, 0.0) && close(z, 0.0),
        "{x} {y} {z}"
    );
    let mut run = player(&pool(), put, "", "");
    let cmd = UserCmd {
        side: 100.0,
        ..UserCmd::default()
    };
    assert!(close(
        moved(&mut run, cmd, 1, 0.1).vector(P, "velocity")[1],
        -70.0
    ));
    let mut run = player(&pool(), put, "", "");
    assert!(close(
        moved(&mut run, forward(100.0), 1, 0.01).vector(P, "velocity")[0],
        7.0
    ));
}

// spec PHYS-8.4: the feet are tested 1 above mins and the middle halfway up the box
#[test]
fn water_is_tested_at_the_feet_and_the_middle() {
    let level = |z: &str| {
        let put = format!("setorigin(self, '0 0 {z}'); self.health = 0;");
        let mut run = player(&pool(), &put, "", "");
        moved(&mut run, UserCmd::default(), 1, 0.1).float(P, "waterlevel")
    };
    // The feet 0.5 below the water's floor, 1 above them in it.
    assert_eq!(level("23.5"), 2.0);
    // The middle at 37, in the water; 8.5 up would not be.
    assert_eq!(level("33"), 2.0);
}

// spec PHYS-8.2: a MOVETYPE_NONE client that is not freed goes on to PlayerPostThink
#[test]
fn a_still_client_runs_its_post_think() {
    let more = "void() PlayerPostThink = { self.ammo_rockets = 1; };
";
    let mut run = player(&ledge(), "self.movetype = 0;", "", more);
    assert_eq!(
        moved(&mut run, UserCmd::default(), 1, 0.1).float(P, "ammo_rockets"),
        1.0
    );
}

// spec PHYS-8.5: the slowing is 1 + d for d the facing's dot with the wall's normal plus 0.5,
// and only along the wall
#[test]
fn wall_friction_scales_with_the_facing() {
    let mut run = pushed(&ledge_over(50.0), "120 0 -25.96875", "-100 100 0", "512");
    let cmd = UserCmd {
        angles: [0.0, 135.0, 0.0],
        ..UserCmd::default()
    };
    let w = moved(&mut run, cmd, 1, 0.1);
    let [x, y, _] = w.vector(P, "velocity");
    let d = 0.5 - core::f32::consts::FRAC_1_SQRT_2;
    assert!(close(y, 100.0 * (1.0 + d)), "{y}");
    assert!(close(x, 0.0), "{x}");
}

// spec PHYS-8.3: a step that cleared its wall is not slowed by it; the push down is 18 less
// the old vertical speed's move, and a step down that lands does not set FL_ONGROUND
#[test]
fn steps_down_by_the_old_speed() {
    let mut run = pushed(&ledge_over(10.0), "120 0 14.03125", "-200 100 0", "512");
    let cmd = UserCmd {
        angles: [0.0, 180.0, 0.0],
        ..UserCmd::default()
    };
    assert!(close(
        moved(&mut run, cmd, 1, 0.1).vector(P, "velocity")[1],
        100.0
    ));
    // With no fall the push down ends 1/1000 short of the floor below the step: no step.
    let mut run = pushed(
        &ledge_over(50.0),
        "116.03125 100 -25.96875",
        "-100 30 0",
        "512",
    );
    let y = moved(&mut run, UserCmd::default(), 1, 0.001).vector(P, "origin")[1];
    assert!(close(y, 100.03), "{y}");
    // Level after gravity, the first move finds no floor; the step lands without the flag.
    let mut run = pushed(&ledge_over(10.0), "120 0 14.03125", "-200 0 80", "512");
    let w = moved(&mut run, UserCmd::default(), 1, 0.1);
    assert!(
        close(w.vector(P, "origin")[2], 24.031_25),
        "{:?}",
        w.vector(P, "origin")
    );
    assert_eq!(w.float(P, "flags"), 0.0);
}

// spec PHYS-3.1: a think due within the frame stops the move at it; a still pusher's clock
// runs on
#[test]
fn pushers_move_to_their_think() {
    let mut run = lift(&ledge(), "0 0 100", "0.25", "200", "");
    run.frames(3, 0.1).unwrap();
    assert!(
        close(run.world.vector(2, "origin")[2], 75.0),
        "{:?}",
        run.world.vector(2, "origin")
    );
    assert!(close(run.world.float(2, "frags"), 0.25));
    let mut run = lift(&ledge(), "0 0 0", "0.25", "200", "");
    run.frames(3, 0.1).unwrap();
    assert!(close(run.world.float(2, "ltime"), 0.25));
    assert!(close(run.world.float(2, "frags"), 0.25));
}

// spec PHYS-3.3: what rides the pusher goes down with it, without the overlap test
// spec PHYS-3.4: a ridden stepping monster loses FL_ONGROUND and so falls
#[test]
fn a_lift_takes_its_rider_down() {
    let mut run = lift(&ledge(), "0 0 -100", "5", "200", "");
    let before = run.world.vector(3, "origin")[2];
    run.frames(1, 0.1).unwrap();
    let after = run.world.vector(3, "origin")[2];
    assert!(close(after, before - 10.0), "{before} {after}");
}

// spec PHYS-3.3: an entity apart from the pusher's moved box on any side is not pushed, even
// stuck in the world
#[test]
fn a_lift_leaves_what_it_does_not_touch() {
    let mut run = lift(
        &ledge(),
        "0 0 100",
        "5",
        "200",
        &entity("stuck", "250 0 -10", ""),
    );
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.world.vector(4, "origin"), [250.0, 0.0, -10.0]);
}

// spec PHYS-4.3: a bouncer lands once the bounce leaves it rising slower than 60
#[test]
fn a_slow_bounce_lands() {
    let source = format!(
        "void() ball = {{ self.movetype = 10; self.solid = 2; {PLAYER_SIZE} \
         setorigin(self, self.origin); }};\n"
    );
    let mut run = run_on(&ledge(), &source, &entity("ball", "50 0 25", ""));
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.world.float(2, "flags"), 512.0);
    assert_eq!(run.world.vector(2, "velocity"), [0.0; 3]);
}

// spec PHYS-4.4: going into liquid from empty, or out of it, makes the splash; staying makes
// none
// spec PHYS-5.1: a stepping monster checks the water every frame
#[test]
fn one_splash_going_in() {
    let source = format!(
        "void() monster = {{ self.movetype = 4; self.solid = 3; {PLAYER_SIZE} \
         setorigin(self, self.origin); }};\n"
    );
    let mut run = run_on(&pool(), &source, &entity("monster", "0 0 60", ""));
    run.frames(10, 0.1).unwrap();
    let splashes = run
        .world
        .take_sounds()
        .iter()
        .filter(|s| s.sample == "misc/h2ohit1.wav")
        .count();
    assert_eq!(splashes, 1);
}

// spec PHYS-5.1: flying or swimming monsters do not fall; landing makes the sound only from a
// fall faster than 80, and only on landing
#[test]
fn falls_and_their_sound() {
    let source = format!(
        "void() monster = {{ self.movetype = 4; self.solid = 3; {PLAYER_SIZE} \
         setorigin(self, self.origin); }};\n"
    );
    let entities = [
        entity("monster", "0 0 200", "\"flags\" \"1\"\n"),
        entity("monster", "60 0 200", "\"flags\" \"2\"\n"),
        entity("monster", "-60 0 25", ""),
        entity("monster", "-200 0 1000", ""),
    ]
    .concat();
    let mut run = run_on(&ledge(), &source, &entities);
    for n in 2..6 {
        if n == 5 {
            run.world.machine.edicts_mut().set_field(
                n,
                field_slot("velocity") + 2,
                Slot::from_f32(-400.0),
            );
        }
    }
    run.frames(1, 0.1).unwrap();
    let w = &run.world;
    assert_eq!(w.vector(2, "origin")[2], 200.0);
    assert_eq!(w.vector(3, "origin")[2], 200.0);
    assert_eq!(w.float(4, "flags"), 512.0);
    assert!(run.world.take_sounds().is_empty());
}

// spec PHYS-2.1: a think due exactly at the frame's end runs in it
#[test]
fn a_think_at_the_frames_end_runs() {
    let source = "void() thought = { self.frags = time; };\n\
        void() thinker = { self.think = thought; self.nextthink = self.ammo_shells; };\n";
    let mut run = run_on(
        &ledge(),
        source,
        &entity("thinker", "0 0 0", "\"ammo_shells\" \"1.25\"\n"),
    );
    run.frames(1, 0.25).unwrap();
    assert_eq!(run.world.float(2, "frags"), 1.25);
}

// spec PHYS-1.2: a freed edict is skipped
// spec PHYS-1.1: StartFrame runs with time the server's
#[test]
fn freed_edicts_rest_and_time_is_set() {
    let source = "void() gone = { remove(self); };\n\
        void() rock = { self.movetype = 6; self.think = gone; self.nextthink = time + 0.05; };\n\
        void() StartFrame = { marker.frags = time; };\n\
        void() worldspawn = { marker = spawn(); };\n";
    let mut run = run_on(&ledge(), source, &entity("rock", "0 0 200", ""));
    run.frames(1, 0.1).unwrap();
    let rested = run.world.vector(3, "origin");
    run.frames(2, 0.1).unwrap();
    assert_eq!(run.world.vector(3, "origin"), rested);
    assert!(
        close(run.world.float(2, "frags"), 1.2),
        "{}",
        run.world.float(2, "frags")
    );
}

/// Walls at x = 100 and y = 100 meeting in a corner over a floor at 0; for the player's hull
/// 116, 116 and 24.
fn corner() -> MapSpec {
    MapSpec {
        planes: vec![
            plane([1.0, 0.0, 0.0], 100.0, 0),
            plane([0.0, 1.0, 0.0], 100.0, 1),
            plane([0.0, 0.0, 1.0], 0.0, 2),
            plane([1.0, 0.0, 0.0], 116.0, 0),
            plane([0.0, 1.0, 0.0], 116.0, 1),
            plane([0.0, 0.0, 1.0], 24.0, 2),
        ],
        nodes: vec![node(0, [1, -1]), node(1, [2, -1]), node(2, [-2, -1])],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY],
        clipnodes: vec![
            ClipNode {
                planenum: 3,
                children: [1, CONTENTS_SOLID],
            },
            ClipNode {
                planenum: 4,
                children: [2, CONTENTS_SOLID],
            },
            ClipNode {
                planenum: 5,
                children: [CONTENTS_EMPTY, CONTENTS_SOLID],
            },
        ],
    }
}

// spec PHYS-7.3: into a corner no single plane's clip fits both, so the velocity goes along
// their crease
#[test]
fn a_slide_into_a_corner_follows_the_crease() {
    let source = format!(
        "void() monster = {{ self.movetype = 4; self.solid = 3; {PLAYER_SIZE} \
         self.velocity = '-300 -300 0'; setorigin(self, self.origin); }};\n"
    );
    let mut run = run_on(&corner(), &source, &entity("monster", "130 130 300", ""));
    run.frames(1, 0.1).unwrap();
    let [vx, vy, vz] = run.world.vector(2, "velocity");
    assert!(
        close(vx, 0.0) && close(vy, 0.0) && close(vz, -80.0),
        "{vx} {vy} {vz}"
    );
    let [x, y, _] = run.world.vector(2, "origin");
    assert!(close(x, 116.031_25) && close(y, 116.031_25), "{x} {y}");
}
