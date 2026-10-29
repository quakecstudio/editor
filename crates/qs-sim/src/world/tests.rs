// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use qc_codegen::{Options, compile_module};
use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_progs::Progs;
use qc_sema::{Db, FileId, ModuleInput};
use qc_types::{Etype, Slot};
use qs_bsp::{Bsp, BspLoader, ClipNode};

use super::{World, field_slot};
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
    let compiled = compile_module(&mut db, &input, |id| names[id.0].clone(), Options::default())
        .unwrap_or_else(|e| panic!("{source:?} did not compile: {e:?}"));
    qc_progs::write(&compiled.progs).unwrap()
}

/// A floor at z = 0 for x below 100, and a pit floor at z = -30 past it; for the player's
/// hull, 24 and -6, with the edge at x = 116.
fn ledge() -> MapSpec {
    MapSpec {
        planes: vec![
            plane([1.0, 0.0, 0.0], 100.0, 0),
            plane([0.0, 0.0, 1.0], 0.0, 2),
            plane([0.0, 0.0, 1.0], -30.0, 2),
            plane([1.0, 0.0, 0.0], 116.0, 0),
            plane([0.0, 0.0, 1.0], 24.0, 2),
            plane([0.0, 0.0, 1.0], -6.0, 2),
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
    let progs_bytes = compile(source);
    let progs = Progs::read(&progs_bytes).unwrap();
    let lump = format!("{{\n\"classname\" \"worldspawn\"\n}}\n{entities}");
    let mut map = bsp(&ledge());
    map.entity_lump = Box::leak(lump.into_bytes().into_boxed_slice());
    let read = Box::new(move |name: &str| {
        files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, bytes)| bytes.clone())
    });
    World::load(&progs, "test", &map, read).expect("the world loads")
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
        self.armorvalue = trace_allsolid; self.dmg = trace_plane_dist; \
        self.enemy = trace_ent; };\n";
    let world = world(source, &entity("tracer", "0 0 0", ""));
    assert!((world.float(2, "frags") - 0.499_843_75).abs() < 1e-6);
    assert_eq!(world.vector(2, "oldorigin"), [50.0, 0.0, 0.031_25]);
    assert_eq!(world.vector(2, "movedir"), [0.0, 0.0, 1.0]);
    assert_eq!(world.float(2, "health"), 1.0);
    assert_eq!(world.float(2, "armorvalue"), 0.0);
    assert_eq!(world.float(2, "dmg"), 0.0);
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
        planes: vec![plane([0.0, 0.0, 1.0], 10.0, 2), plane([0.0, 0.0, 1.0], 34.0, 2)],
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
        self.frags = trace_fraction; self.enemy = trace_ent; };\n";
    let entities = [
        entity("boxed", "300 0 50", ""),
        entity("sprite", "0 0 0", ""),
        entity("thing", "0 0 0", ""),
        entity("tracer", "0 0 0", ""),
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
