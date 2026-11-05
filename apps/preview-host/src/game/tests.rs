// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use qc_codegen::compile_module;
use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_progs::Progs;
use qc_sema::{Db, FileId, ModuleInput};
use qc_types::Etype;
use qs_bridge::{ErrorCode, Id, Message, RpcError, Session, check_schema};
use qs_bsp::{
    BSP_HEADER_LUMPS, BSP_MAX_MAP_HULLS, BSP_NUM_AMBIENTS, Bsp, BspVersion, ClipNode, Leaf, Lump,
    Model, Node, Plane,
};
use qs_sim::{LoadOptions, World};
use serde_json::{Value, json};

use super::{Eye, Game, Reporting};
use crate::screenshot::Frame;

/// The QuakeC the tests run: a crate the map places, a counter `StartFrame` raises, a
/// thinker that fails and the entry points; every progs precaches the player's model, which
/// the baselines need (NET-2.1).
const SOURCE: &str = "\
void(entity e, vector o) setorigin = #2;
void(entity e, vector min, vector max) setsize = #4;
entity() spawn = #14;
void(entity e) remove = #15;
void(string s) changelevel = #70;
string(string s) precache_model = #20;
float frames;
.float count;
void() main = {};
void() StartFrame = { frames = frames + 1; };
void() PlayerPreThink = {};
void() PlayerPostThink = {};
void() ClientKill = {};
void() ClientConnect = {};
void() PutClientInServer = { self.classname = \"player\"; self.health = 100; };
void() ClientDisconnect = {};
void() SetNewParms = {};
void() SetChangeParms = { parm1 = self.health; };
void() worldspawn = { precache_model(\"progs/player.mdl\"); };
void() item_crate = { self.count = 3; self.solid = 2; setsize(self, '-8 -8 -8', '8 8 8'); setorigin(self, self.origin); };
.void() nothing;
void() broken = { self.nothing(); };
void() leave = { changelevel(\"e1m2\"); };
void() info_exit = { self.think = leave; self.nextthink = 0.05; };
void() info_fail = { self.think = broken; self.nextthink = 0.05; };
";

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

/// The progs `qc-codegen` writes for [`SOURCE`] after the system definitions.
fn progs() -> Vec<u8> {
    let mut defs = String::new();
    for var in SYSTEM_GLOBALS {
        writeln!(defs, "{} {};", type_name(var.etype), var.name).unwrap();
    }
    defs.push_str("void end_sys_globals;\n");
    for var in SYSTEM_FIELDS {
        writeln!(defs, ".{} {};", type_name(var.etype), var.name).unwrap();
    }
    defs.push_str("void end_sys_fields;\n");
    let files = [("defs.qc", defs), ("t.qc", SOURCE.to_owned())];
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
    let compiled = compile_module(&mut db, &input, |id| names[id.index()].clone())
        .unwrap_or_else(|e| panic!("the test progs did not compile: {e:?}"));
    qc_progs::write(&compiled.progs).unwrap()
}

fn plane(dist: f32) -> Plane {
    Plane {
        normal: [0.0, 0.0, 1.0],
        dist,
        plane_type: 2,
        signbits: 0,
    }
}

fn leaf(contents: i32) -> Leaf {
    Leaf {
        contents,
        visofs: -1,
        mins: [0.0; 3],
        maxs: [0.0; 3],
        firstmarksurface: 0,
        nummarksurfaces: 0,
        ambient_level: [0; BSP_NUM_AMBIENTS],
    }
}

/// A floor at z = 0, solid below; for the player's hull, at z = 24.
fn floor(entities: &'static [u8]) -> Bsp<'static> {
    Bsp {
        version: BspVersion::Bsp29,
        lumps: [Lump::default(); BSP_HEADER_LUMPS],
        entity_lump: entities,
        planes: vec![plane(0.0), plane(24.0)],
        textures: Vec::new(),
        vertexes: Vec::new(),
        visibility: b"",
        nodes: vec![Node {
            planenum: 0,
            children: [-2, -1],
            mins: [0.0; 3],
            maxs: [0.0; 3],
            firstface: 0,
            numfaces: 0,
        }],
        texinfo: Vec::new(),
        faces: Vec::new(),
        lighting: b"",
        clipnodes: vec![ClipNode {
            planenum: 1,
            children: [-1, -2],
        }],
        leafs: vec![leaf(-2), leaf(-1)],
        marksurfaces: Vec::new(),
        edges: Vec::new(),
        surfedges: Vec::new(),
        models: vec![Model {
            mins: [-1000.0; 3],
            maxs: [1000.0; 3],
            origin: [0.0; 3],
            headnode: [0; BSP_MAX_MAP_HULLS],
            visleafs: 0,
            firstface: 0,
            numfaces: 0,
            radius: 0.0,
        }],
    }
}

/// The map's entities after the worldspawn: a crate at edict 2.
const CRATE: &str = "{\n\"classname\" \"item_crate\"\n\"origin\" \"0 0 40\"\n}\n";

/// The game of the test progs on the floor with `entities`, its player in the game, and an
/// initialized session.
fn game_with(entities: &str) -> (Game, Session) {
    let bytes = progs();
    let progs = Progs::read(&bytes).unwrap();
    let lump = format!("{{\n\"classname\" \"worldspawn\"\n}}\n{entities}");
    let map = floor(Box::leak(lump.into_bytes().into_boxed_slice()));
    let mut world = World::load(
        &progs,
        "test",
        &map,
        Box::new(|_| None),
        LoadOptions::default(),
    )
    .expect("the world loads");
    world.connect(&progs).unwrap();
    let mut game = Game::new(world, bytes.clone(), Reporting::Bridge);
    let mut session = Session::new();
    let init = request(
        0,
        "initialize",
        &json!({ "protocolVersion": "1.0", "client": { "name": "test" } }),
    );
    session.handle(&mut game, init);
    game.take_events();
    (game, session)
}

fn game() -> (Game, Session) {
    game_with(CRATE)
}

fn request(id: i64, method: &str, params: &Value) -> Message {
    let content = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    Message::parse(content.to_string().as_bytes()).unwrap()
}

/// Sends `method` through the session; a result is checked against its schema.
fn call(
    game: &mut Game,
    session: &mut Session,
    method: &str,
    params: &Value,
) -> Result<Value, RpcError> {
    let reply = session.handle(game, request(1, method, params));
    let Some(Message::Response { outcome, .. }) = reply.response else {
        panic!("{method} was not answered");
    };
    if let Ok(result) = &outcome {
        check_schema(&format!("{}.result", method.replace('/', ".")), result)
            .unwrap_or_else(|e| panic!("{method}'s result {result} breaks its schema: {e:?}"));
    }
    outcome
}

/// The events raised since the last call, each checked against its schema.
fn events(game: &mut Game) -> Vec<(&'static str, Value)> {
    let events = game.take_events();
    for (method, params) in &events {
        check_schema(&format!("event.{}", method.replace('/', ".")), params)
            .unwrap_or_else(|e| panic!("{method} {params} breaks its schema: {e:?}"));
    }
    events
}

fn methods(events: &[(&'static str, Value)]) -> Vec<&'static str> {
    events.iter().map(|(m, _)| *m).collect()
}

fn code(outcome: Result<Value, RpcError>) -> ErrorCode {
    outcome.expect_err("an error").code
}

fn float_field(game: &mut Game, session: &mut Session, edict: usize, name: &str) -> Value {
    let result = call(
        game,
        session,
        "inspect",
        &json!({ "what": "edict", "edict": edict }),
    )
    .unwrap();
    result["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == name)
        .map_or(Value::Null, |f| f["value"]["value"].clone())
}

// spec BRIDGE-2.3: the game offers every command it implements, and no other
#[test]
fn the_game_offers_its_commands() {
    let (mut game, mut session) = game();
    let reply = session.handle(
        &mut game,
        request(
            5,
            "initialize",
            &json!({ "protocolVersion": "1.0", "client": { "name": "again" } }),
        ),
    );
    drop(reply);
    let (mut fresh, mut session) = (game_with("").0, Session::new());
    let reply = session.handle(
        &mut fresh,
        request(
            1,
            "initialize",
            &json!({ "protocolVersion": "1.0", "client": { "name": "test" } }),
        ),
    );
    let Some(Message::Response {
        outcome: Ok(result),
        ..
    }) = reply.response
    else {
        panic!("initialize was not answered");
    };
    let capabilities = &result["capabilities"];
    for command in super::COMMANDS {
        assert_eq!(capabilities[command], true, "{command}");
    }
    for later in ["reload", "breakpoint/set", "breakpoint/clear", "evaluate"] {
        assert_ne!(capabilities[later], true, "{later}");
    }
}

// spec BRIDGE-3.1: progs/loaded names the progs and its counts; map/loaded the map and its
// entities in use
#[test]
fn loading_announces_the_progs_and_the_map() {
    let (mut game, _session) = game();
    game.announce("progs.dat", "test");
    let raised = events(&mut game);
    assert_eq!(methods(&raised), ["progs/loaded", "map/loaded"]);
    let progs = Progs::read(game.progs()).unwrap();
    assert_eq!(raised[0].1["path"], "progs.dat");
    assert_eq!(raised[0].1["crc"], progs.header().crc);
    assert_eq!(raised[0].1["functions"], progs.header().numfunctions);
    // The world, the player and the crate.
    assert_eq!(raised[1].1["entities"], 3);
}

// spec BRIDGE-4.1: pause stops the server clock and resume starts it, each answered with the
// time
// spec BRIDGE-3.3: pause and resume each send state/changed
#[test]
fn pause_and_resume_stop_and_start_the_clock() {
    let (mut game, mut session) = game();
    assert!(game.frame(0.1));
    let before = game.world().time();
    let paused = call(&mut game, &mut session, "pause", &json!({})).unwrap();
    assert_eq!(paused["time"], before);
    assert!(!game.frame(0.1), "no frame runs while paused");
    assert_eq!(game.world().time(), before);
    let raised = events(&mut game);
    let state: Vec<&Value> = raised
        .iter()
        .filter(|(m, _)| *m == "state/changed")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(state.len(), 1);
    assert_eq!(state[0]["state"], "paused");
    assert_eq!(state[0]["reason"], "pause");

    call(&mut game, &mut session, "resume", &json!({})).unwrap();
    assert_eq!(events(&mut game).last().unwrap().1["state"], "running");
    assert!(game.frame(0.1));
    assert!(game.world().time() > before);
}

// spec BRIDGE-4.3: step needs the host paused; a frame step runs one frame and stays paused
// spec BRIDGE-3.3: a step ends with state/changed to paused, reason step
#[test]
fn step_runs_one_frame_while_paused() {
    let (mut game, mut session) = game();
    let running = call(&mut game, &mut session, "step", &json!({ "kind": "frame" }));
    assert_eq!(code(running), ErrorCode::NotPaused);
    call(&mut game, &mut session, "pause", &json!({})).unwrap();
    events(&mut game);
    let before = game.world().time();
    let stepped = call(&mut game, &mut session, "step", &json!({ "kind": "frame" })).unwrap();
    assert!(stepped["time"].as_f64().unwrap() > before);
    let raised = events(&mut game);
    let (method, last) = raised.last().unwrap();
    assert_eq!(*method, "state/changed");
    assert_eq!(last["state"], "paused");
    assert_eq!(last["reason"], "step");
    assert!(!game.frame(0.1), "still paused");
    let statement = call(
        &mut game,
        &mut session,
        "step",
        &json!({ "kind": "statement" }),
    );
    assert_eq!(code(statement), ErrorCode::Unsupported);
    let bad = call(
        &mut game,
        &mut session,
        "step",
        &json!({ "kind": "sideways" }),
    );
    assert_eq!(code(bad), ErrorCode::InvalidParams);
}

// spec BRIDGE-4.4: while running a field is set now; while paused on resume, in order; a
// value of another type is refused at once and not applied
// spec BRIDGE-3.2: a field set while paused is reported when it is applied
#[test]
fn field_set_applies_now_or_on_resume() {
    let (mut game, mut session) = game();
    let set = |game: &mut Game, session: &mut Session, value: f64| {
        call(
            game,
            session,
            "field/set",
            &json!({ "edict": 2, "field": "count", "value": { "type": "float", "value": value } }),
        )
    };
    assert_eq!(set(&mut game, &mut session, 5.0).unwrap()["applied"], "now");
    assert_eq!(float_field(&mut game, &mut session, 2, "count"), 5.0);
    let changed = events(&mut game);
    assert_eq!(methods(&changed), ["edict/changed"]);
    // The field, and the bounds relinking the edict writes (WORLD-3).
    let names: Vec<&Value> = changed[0].1["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| &f["name"])
        .collect();
    assert!(names.contains(&&json!("count")), "{names:?}");

    call(&mut game, &mut session, "pause", &json!({})).unwrap();
    events(&mut game);
    assert_eq!(
        set(&mut game, &mut session, 6.0).unwrap()["applied"],
        "on_resume"
    );
    assert_eq!(
        set(&mut game, &mut session, 7.0).unwrap()["applied"],
        "on_resume"
    );
    assert_eq!(
        float_field(&mut game, &mut session, 2, "count"),
        5.0,
        "held"
    );
    assert!(events(&mut game).is_empty());
    let wrong = call(
        &mut game,
        &mut session,
        "field/set",
        &json!({ "edict": 2, "field": "count", "value": { "type": "vector", "value": [1, 2, 3] } }),
    );
    assert_eq!(code(wrong), ErrorCode::BadValue);
    call(&mut game, &mut session, "resume", &json!({})).unwrap();
    assert_eq!(
        float_field(&mut game, &mut session, 2, "count"),
        7.0,
        "in order"
    );
    let resumed = events(&mut game);
    assert_eq!(methods(&resumed), ["edict/changed", "state/changed"]);
}

// spec BRIDGE-4.5: an edict not in use is answered with no such edict, whatever the command
#[test]
fn an_edict_not_in_use_is_refused() {
    let (mut game, mut session) = game();
    let float = json!({ "type": "float", "value": 1 });
    for (method, params) in [
        ("inspect", json!({ "what": "edict", "edict": 40 })),
        (
            "field/set",
            json!({ "edict": 40, "field": "count", "value": float }),
        ),
        ("entity/edit", json!({ "edict": 40, "keyvalues": [] })),
        ("entity/remove", json!({ "edict": 40 })),
    ] {
        assert_eq!(
            code(call(&mut game, &mut session, method, &params)),
            ErrorCode::NoSuchEdict,
            "{method}"
        );
    }
}

// spec BRIDGE-4.1: inspect reads an edict's classname and fields, the named globals, and the
// edicts in use with classname, model and origin
// spec BRIDGE-4.3: the stack needs the host paused; the rest run at any time
#[test]
fn inspect_reads_edicts_globals_and_the_entity_list() {
    let (mut game, mut session) = game();
    let edict = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "edict", "edict": 2 }),
    )
    .unwrap();
    assert_eq!(edict["classname"], "item_crate");
    assert_eq!(float_field(&mut game, &mut session, 2, "count"), 3.0);
    let globals = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "globals" }),
    )
    .unwrap();
    assert!(
        globals["globals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["name"] == "frames")
    );
    let list = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "entities" }),
    )
    .unwrap();
    let entities = list["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 3);
    assert_eq!(entities[2]["classname"], "item_crate");
    assert_eq!(entities[2]["origin"], json!([0.0, 0.0, 40.0]));
    let stack = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "stack" }),
    );
    assert_eq!(code(stack), ErrorCode::NotPaused);
    call(&mut game, &mut session, "pause", &json!({})).unwrap();
    let stack = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "stack" }),
    )
    .unwrap();
    assert_eq!(stack["stack"], json!([]));
}

// spec BRIDGE-4.1: an entity is spawned from a classname and key-values as the map spawns
// one, edited the same way, and removed; the world and the player are never removed
// spec BRIDGE-3.1: spawning and removing send edict/spawned and edict/removed
#[test]
fn entities_are_spawned_edited_and_removed() {
    let (mut game, mut session) = game();
    let spawned = call(
        &mut game,
        &mut session,
        "entity/spawn",
        &json!({ "classname": "item_crate", "keyvalues": [{ "key": "origin", "value": "8 0 40" }] }),
    )
    .unwrap();
    let n = usize::try_from(spawned["edict"].as_u64().unwrap()).unwrap();
    let raised = events(&mut game);
    assert_eq!(raised[0].0, "edict/spawned");
    assert_eq!(raised[0].1["edict"], n);
    assert_eq!(raised[0].1["classname"], "item_crate");
    assert_eq!(float_field(&mut game, &mut session, n, "count"), 3.0);

    let edited = call(
        &mut game,
        &mut session,
        "entity/edit",
        &json!({ "edict": n, "keyvalues": [{ "key": "count", "value": "9" }] }),
    )
    .unwrap();
    assert_eq!(edited["applied"], "now");
    assert_eq!(float_field(&mut game, &mut session, n, "count"), 9.0);
    call(&mut game, &mut session, "pause", &json!({})).unwrap();
    let held = call(
        &mut game,
        &mut session,
        "entity/edit",
        &json!({ "edict": n, "keyvalues": [{ "key": "count", "value": "4" }] }),
    )
    .unwrap();
    assert_eq!(held["applied"], "on_resume");
    call(&mut game, &mut session, "resume", &json!({})).unwrap();
    assert_eq!(float_field(&mut game, &mut session, n, "count"), 4.0);
    events(&mut game);

    call(
        &mut game,
        &mut session,
        "entity/remove",
        &json!({ "edict": n }),
    )
    .unwrap();
    assert_eq!(methods(&events(&mut game)), ["edict/removed"]);
    for kept in [0, 1] {
        let refused = call(
            &mut game,
            &mut session,
            "entity/remove",
            &json!({ "edict": kept }),
        );
        assert_eq!(code(refused), ErrorCode::InvalidParams);
    }
    let unknown = call(
        &mut game,
        &mut session,
        "entity/spawn",
        &json!({ "classname": "nonesuch" }),
    );
    assert_eq!(code(unknown), ErrorCode::InvalidParams);
}

// spec BRIDGE-4.1: spawn player at moves the player to the origin and angles
#[test]
fn the_player_is_moved() {
    let (mut game, mut session) = game();
    let moved = call(
        &mut game,
        &mut session,
        "player/spawn_at",
        &json!({ "origin": [64, 32, 24], "angles": [0, 90, 0] }),
    )
    .unwrap();
    assert_eq!(moved["edict"], 1);
    let result = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "edict", "edict": 1 }),
    )
    .unwrap();
    let origin = result["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "origin")
        .unwrap();
    assert_eq!(origin["value"]["value"], json!([64.0, 32.0, 24.0]));
}

// spec BRIDGE-4.1: a console command's output comes back as console/line events
#[test]
fn console_output_comes_back_as_lines() {
    let (mut game, mut session) = game();
    let result = call(
        &mut game,
        &mut session,
        "console",
        &json!({ "command": "fly; \"me\" now" }),
    )
    .unwrap();
    assert_eq!(result, json!({}));
    let lines = events(&mut game);
    assert_eq!(methods(&lines), ["console/line", "console/line"]);
    assert_eq!(lines[0].1["text"], "Unknown command \"fly\"");
    assert_eq!(lines[1].1["text"], "Unknown command \"me\"");
}

// spec BRIDGE-3.1: each frame reports the fields written during it
// spec BRIDGE-3.3: a frame that fails sends qc/error, then state/changed to paused with
// reason error, at the failing function
#[test]
fn a_frame_reports_changes_and_a_failure_pauses() {
    let (mut game, _session) = game();
    assert!(game.frame(0.1));
    let changed = events(&mut game);
    assert!(changed.iter().all(|(m, _)| *m == "edict/changed"));

    let (mut failing, _session) = game_with("{\n\"classname\" \"info_fail\"\n}\n");
    events(&mut failing);
    for _ in 0..5 {
        failing.frame(0.1);
    }
    let raised = events(&mut failing);
    let at = raised
        .iter()
        .position(|(m, _)| *m == "qc/error")
        .expect("a qc/error");
    assert_eq!(raised[at].1["location"]["function"], "broken");
    let (method, paused) = &raised[at + 1];
    assert_eq!(*method, "state/changed");
    assert_eq!(paused["reason"], "error");
    assert!(!failing.frame(0.1), "paused");
}

// spec BRIDGE-2.5: shutdown stops the game
#[test]
fn shutdown_stops_the_game() {
    let (mut game, mut session) = game();
    assert!(!game.stopped());
    session.handle(&mut game, request(9, "shutdown", &json!({})));
    assert!(game.stopped());
}

/// The eye at `origin` looking along `angles`, a square view with GLQuake's `fov 90`.
fn eye(origin: [f32; 3], angles: [f32; 3]) -> Eye {
    Eye {
        origin,
        angles,
        fov_x: 90.0,
        aspect: 1.0,
    }
}

// spec BRIDGE-4.1: pick names the edict under a point of the window and where the ray stops;
// the world, or nothing, names no edict
#[test]
fn pick_finds_the_edict_under_a_point() {
    let (mut game, mut session) = game();
    let early = call(
        &mut game,
        &mut session,
        "pick",
        &json!({ "x": 0.5, "y": 0.5 }),
    );
    assert_eq!(code(early), ErrorCode::InternalError, "nothing drawn yet");
    game.set_eye(eye([-100.0, 0.0, 40.0], [0.0, 0.0, 0.0]));
    let hit = call(
        &mut game,
        &mut session,
        "pick",
        &json!({ "x": 0.5, "y": 0.5 }),
    )
    .unwrap();
    assert_eq!(hit["edict"], 2, "the crate");
    let x = hit["point"][0].as_f64().unwrap();
    assert!((x + 8.0).abs() < 0.1, "{hit}");
    // The bottom of the view looks 45 degrees down, at the floor.
    let floor = call(
        &mut game,
        &mut session,
        "pick",
        &json!({ "x": 0.5, "y": 1.0 }),
    )
    .unwrap();
    assert_eq!(floor.get("edict"), None);
    assert!(floor["point"][2].as_f64().unwrap().abs() < 0.1, "{floor}");
    let up = call(
        &mut game,
        &mut session,
        "pick",
        &json!({ "x": 0.5, "y": 0.0 }),
    )
    .unwrap();
    assert_eq!(up, json!({}), "nothing above");
}

fn frame(width: u32, height: u32) -> Frame {
    Frame {
        width,
        height,
        rgba: vec![128; usize::try_from(width * height * 4).unwrap()],
    }
}

// spec BRIDGE-4.1: a screenshot waits for the next frame drawn, then is answered with it as a
// PNG at the size asked; each waiting request gets its own response
#[test]
fn a_screenshot_is_answered_with_the_next_frame() {
    let (mut game, mut session) = game();
    assert!(!game.wants_screenshot());
    let reply = session.handle(&mut game, request(7, "screenshot", &json!({})));
    assert_eq!(reply.response, None, "answered later");
    let sized = request(8, "screenshot", &json!({ "width": 2, "height": 3 }));
    assert_eq!(session.handle(&mut game, sized).response, None);
    assert!(game.wants_screenshot());
    assert!(game.take_responses().is_empty());
    game.screenshot_taken(&frame(4, 4));
    assert!(!game.wants_screenshot());
    let responses = game.take_responses();
    assert_eq!(responses.len(), 2);
    let sizes: Vec<(Id, Value, Value)> = responses
        .into_iter()
        .map(|(id, outcome)| {
            let result = outcome.unwrap();
            check_schema("screenshot.result", &result).unwrap();
            (id, result["width"].clone(), result["height"].clone())
        })
        .collect();
    assert_eq!(
        sizes,
        [
            (Id::Number(7), json!(4), json!(4)),
            (Id::Number(8), json!(2), json!(3))
        ]
    );
}

// spec BRIDGE-3.3: a step whose frame fails pauses once, with reason error, not step
#[test]
fn a_failed_step_pauses_once_for_the_error() {
    let (mut game, mut session) = game_with("{\n\"classname\" \"info_fail\"\n}\n");
    call(&mut game, &mut session, "pause", &json!({})).unwrap();
    events(&mut game);
    let mut raised = Vec::new();
    for _ in 0..10 {
        call(&mut game, &mut session, "step", &json!({ "kind": "frame" })).unwrap();
        raised.extend(events(&mut game));
        if raised.iter().any(|(m, _)| *m == "qc/error") {
            break;
        }
    }
    let at = raised
        .iter()
        .position(|(m, _)| *m == "qc/error")
        .expect("a qc/error");
    let after: Vec<&Value> = raised[at..]
        .iter()
        .filter(|(m, _)| *m == "state/changed")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(after.len(), 1, "{raised:?}");
    assert_eq!(after[0]["reason"], "error");
}

// spec BRIDGE-3.1: a game no client hears records no edict changes
#[test]
fn a_game_played_alone_records_no_changes() {
    let bytes = progs();
    let progs_read = Progs::read(&bytes).unwrap();
    let lump = format!("{{\n\"classname\" \"worldspawn\"\n}}\n{CRATE}");
    let map = floor(Box::leak(lump.into_bytes().into_boxed_slice()));
    let mut world = World::load(
        &progs_read,
        "test",
        &map,
        Box::new(|_| None),
        LoadOptions::default(),
    )
    .unwrap();
    world.connect(&progs_read).unwrap();
    let mut game = Game::new(world, bytes.clone(), Reporting::Off);
    for _ in 0..3 {
        assert!(game.frame(0.1));
    }
    assert!(game.take_events().is_empty());
}

// spec BRIDGE-4.4: a field set moves the edict in the world too: a pick finds it at its new
// origin, not its old one
#[test]
fn a_moved_edict_is_found_where_it_now_is() {
    let (mut game, mut session) = game();
    call(
        &mut game,
        &mut session,
        "field/set",
        &json!({ "edict": 2, "field": "origin", "value": { "type": "vector", "value": [0, 200, 40] } }),
    )
    .unwrap();
    game.set_eye(eye([-100.0, 0.0, 40.0], [0.0, 0.0, 0.0]));
    let old = call(
        &mut game,
        &mut session,
        "pick",
        &json!({ "x": 0.5, "y": 0.5 }),
    )
    .unwrap();
    assert_eq!(old.get("edict"), None, "{old}");
    game.set_eye(eye([-100.0, 200.0, 40.0], [0.0, 0.0, 0.0]));
    let new = call(
        &mut game,
        &mut session,
        "pick",
        &json!({ "x": 0.5, "y": 0.5 }),
    )
    .unwrap();
    assert_eq!(new["edict"], 2, "{new}");
}

// spec PHYS-10.4: changelevel names the next level, and the player carries the parms
// SetChangeParms leaves; no level is asked for before it
#[test]
fn a_level_change_carries_the_players_parms() {
    let (mut idle, _session) = game();
    for _ in 0..3 {
        idle.frame(0.1);
    }
    assert!(idle.level_change().is_none());
    let (mut game, _session) = game_with("{\n\"classname\" \"info_exit\"\n}\n");
    let mut change = None;
    for _ in 0..10 {
        game.frame(0.1);
        change = game.level_change();
        if change.is_some() {
            break;
        }
    }
    let (map, kept) = change.expect("a level change");
    assert_eq!(map, "e1m2");
    assert_eq!(kept.parms()[0], 100.0, "parm1 is the player's health");
    assert!(game.level_change().is_none(), "taken once");
}
