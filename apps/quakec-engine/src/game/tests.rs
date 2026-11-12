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
use std::rc::Rc;

use qc_codegen::compile_module;
use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_progs::Progs;
use qc_sema::{Db, FileId, ModuleInput};
use qc_types::Etype;
use qs_bridge::{ErrorCode, Host as _, Id, Message, RpcError, Session, check_schema};
use qs_bsp::{
    BSP_HEADER_LUMPS, BSP_MAX_MAP_HULLS, BSP_NUM_AMBIENTS, Bsp, BspVersion, ClipNode, Leaf, Lump,
    Model, Node, Plane,
};
use qs_sim::{LoadOptions, World};
use serde_json::{Value, json};

use super::{Eye, Game, HostAction, NextLevel, Placement, Reporting, Screen};
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
void(string s) localcmd = #46;
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
void() again = { localcmd(\"restart\\n\"); };
void() info_restart = { self.think = again; self.nextthink = 0.05; };
void() info_exit = { self.think = leave; self.nextthink = 0.05; };
void() info_fail = { self.think = broken; self.nextthink = 0.05; };
void() growl = {
    self.count = 9;
};
void() dog_think = {
    self.frags = self.frags + 1;
    self.health = 50;
    growl();
    self.nextthink = time + 0.1;
};
void() monster_dog = { self.think = dog_think; self.nextthink = 0.05; };
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
    progs_with_lines().0
}

/// [`progs`] and its `.lno`'s line of each statement.
fn progs_with_lines() -> (Vec<u8>, Vec<i32>) {
    let (bytes, lno) = progs_of(SOURCE);
    (bytes, lno.lines)
}

/// The progs and `.lno` `qc-codegen` writes for `source` after the system definitions.
fn progs_of(source: &str) -> (Vec<u8>, qc_lno::LnoFile) {
    let mut defs = String::new();
    for var in SYSTEM_GLOBALS {
        writeln!(defs, "{} {};", type_name(var.etype), var.name).unwrap();
    }
    defs.push_str("void end_sys_globals;\n");
    for var in SYSTEM_FIELDS {
        writeln!(defs, ".{} {};", type_name(var.etype), var.name).unwrap();
    }
    defs.push_str("void end_sys_fields;\n");
    let files = [("defs.qc", defs), ("t.qc", source.to_owned())];
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
    (qc_progs::write(&compiled.progs).unwrap(), compiled.lno)
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
pub(crate) fn floor(entities: &'static [u8]) -> Bsp<'static> {
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
    let (bytes, lines) = progs_with_lines();
    let progs = Progs::read(&bytes).unwrap();
    let lump = format!("{{\n\"classname\" \"worldspawn\"\n}}\n{entities}");
    let map = floor(Box::leak(lump.into_bytes().into_boxed_slice()));
    let mut world = World::load(
        &progs,
        "test",
        &map,
        Rc::new(|_| None),
        LoadOptions::default(),
    )
    .expect("the world loads");
    world.connect(&progs).unwrap();
    let mut game = Game::new(world, bytes.clone(), Reporting::Bridge);
    game.set_lines(lines);
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

// spec BRIDGE-2.6: the game offers every command it implements, and no other
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
    assert_eq!(capabilities["reload"], true, "P3-03");
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

// spec BRIDGE-4.9: pause stops the server clock and resume starts it, each answered with the
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
    // spec WORLD-9.3: a statement step from between two frames stops at the next frame's
    // first statement, here StartFrame's
    call(
        &mut game,
        &mut session,
        "step",
        &json!({ "kind": "statement" }),
    )
    .unwrap();
    let raised = events(&mut game);
    let (_, last) = raised.last().unwrap();
    assert_eq!(last["reason"], "step");
    assert_eq!(last["location"]["function"], "StartFrame");
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

// spec BRIDGE-4.9: inspect reads an edict's classname and fields, the named globals, and the
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

// spec BRIDGE-4.9: an entity is spawned from a classname and key-values as the map spawns
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

// spec BRIDGE-4.13: the map editor names an entity by its place in the map's list: one spawned
// at a place moves the later ones on, one removed moves them back, and one in no edict is
// refused for an edit and removed from the list alone
#[test]
fn map_entities_are_named_by_their_place() {
    let (mut game, mut session) = game();
    let edit = |game: &mut Game, session: &mut Session, place: usize, count: &str| {
        call(
            game,
            session,
            "entity/edit",
            &json!({ "map_entity": place, "keyvalues": [{ "key": "count", "value": count }] }),
        )
    };
    edit(&mut game, &mut session, 1, "7").unwrap();
    assert_eq!(float_field(&mut game, &mut session, 2, "count"), 7.0);

    let spawned = call(
        &mut game,
        &mut session,
        "entity/spawn",
        &json!({ "classname": "item_crate", "map_entity": 1 }),
    )
    .unwrap();
    let n = usize::try_from(spawned["edict"].as_u64().unwrap()).unwrap();
    edit(&mut game, &mut session, 1, "5").unwrap();
    assert_eq!(float_field(&mut game, &mut session, n, "count"), 5.0);
    edit(&mut game, &mut session, 2, "8").unwrap();
    assert_eq!(float_field(&mut game, &mut session, 2, "count"), 8.0);

    let remove = |game: &mut Game, session: &mut Session, place: usize| {
        call(
            game,
            session,
            "entity/remove",
            &json!({ "map_entity": place }),
        )
    };
    remove(&mut game, &mut session, 1).unwrap();
    edit(&mut game, &mut session, 1, "6").unwrap();
    assert_eq!(float_field(&mut game, &mut session, 2, "count"), 6.0);
    remove(&mut game, &mut session, 1).unwrap();
    assert_eq!(
        code(edit(&mut game, &mut session, 1, "1")),
        ErrorCode::NoSuchEdict
    );
    assert_eq!(remove(&mut game, &mut session, 9).unwrap(), json!({}));

    // A failed spawn still takes its place, in no edict.
    let failed = call(
        &mut game,
        &mut session,
        "entity/spawn",
        &json!({ "classname": "nonesuch", "map_entity": 1 }),
    );
    assert_eq!(code(failed), ErrorCode::InvalidParams);
    assert_eq!(
        code(edit(&mut game, &mut session, 1, "1")),
        ErrorCode::NoSuchEdict
    );
    let neither = call(&mut game, &mut session, "entity/remove", &json!({}));
    assert_eq!(code(neither), ErrorCode::InvalidParams);
    // A place written as a float is not a place, and nothing is spawned or listed.
    let float = call(
        &mut game,
        &mut session,
        "entity/spawn",
        &json!({ "classname": "item_crate", "map_entity": 2.0 }),
    );
    assert_eq!(code(float), ErrorCode::InvalidParams);
}

// spec BRIDGE-4.9: spawn player at moves the player to the origin and angles
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

// spec BRIDGE-4.9: a console command's output comes back as console/line events
#[test]
fn console_output_comes_back_as_lines() {
    let (mut game, mut session) = game();
    let result = call(
        &mut game,
        &mut session,
        "console",
        &json!({ "command": "nosuch; \"me\" now; color" }),
    )
    .unwrap();
    assert_eq!(result, json!({}));
    let lines = events(&mut game);
    assert_eq!(methods(&lines), ["console/line"; 4]);
    assert_eq!(lines[0].1["text"], "Unknown command \"nosuch\"");
    assert_eq!(lines[1].1["text"], "Unknown command \"me\"");
    assert_eq!(lines[2].1["text"], "\"color\" is \"0 0\"", "one line each");
    assert_eq!(lines[3].1["text"], "color <0-13> [0-13]");
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

// spec BRIDGE-4.9: pick names the edict under a point of the window and where the ray stops;
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

// spec BRIDGE-3.5: a click in the paused game sends edict/picked with the edict under it, a
// click at nothing sends nothing, and a running game's click picks nothing
#[test]
fn a_click_in_the_paused_game_picks_the_edict_under_it() {
    let (mut game, mut session) = game();
    game.set_eye(eye([-100.0, 0.0, 40.0], [0.0, 0.0, 0.0]));
    game.clicked(0.5, 0.5);
    assert!(
        events_named(&mut game, "edict/picked").is_empty(),
        "running"
    );
    call(&mut game, &mut session, "pause", &json!({})).unwrap();
    let _ = events(&mut game);
    game.clicked(0.5, 0.5);
    let picked = events(&mut game);
    assert_eq!(methods(&picked), ["edict/picked"]);
    assert_eq!(picked[0].1["edict"], 2, "the crate");
    game.clicked(0.5, 0.0);
    assert!(events(&mut game).is_empty(), "nothing above");
    assert!(game.paused(), "a click runs no frame and keeps the pause");
}

fn frame(width: u32, height: u32) -> Frame {
    Frame {
        width,
        height,
        rgba: vec![128; usize::try_from(width * height * 4).unwrap()],
    }
}

// spec BRIDGE-4.9: a screenshot waits for the next frame drawn, then is answered with it as a
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

// spec BRIDGE-3.1: a level change keeps what the old level owed the client: its events
// first, then the new level's, and the screenshots still waiting for a frame
#[test]
fn the_next_level_carries_what_the_last_one_owed() {
    let (mut old, mut session) = game();
    old.announce("progs.dat", "old");
    let reply = session.handle(&mut old, request(7, "screenshot", &json!({})));
    assert_eq!(reply.response, None);
    let (mut next, _) = game();
    next.announce("progs.dat", "next");
    next.carry_from(old);
    let maps: Vec<Value> = next
        .take_events()
        .into_iter()
        .filter(|(method, _)| *method == "map/loaded")
        .map(|(_, params)| params["map"].clone())
        .collect();
    assert_eq!(maps, [json!("old"), json!("next")]);
    assert!(next.wants_screenshot());
    next.screenshot_taken(&frame(1, 1));
    let responses = next.take_responses();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].0, Id::Number(7));
}

// spec BRIDGE-2.6: a headless host with a GPU device draws offscreen, so it offers screenshot
// as the window does; a frame it could not draw answers each waiting screenshot with an error
#[test]
fn an_offscreen_host_offers_screenshot_and_reports_a_failed_frame() {
    let (mut game, mut session) = game();
    game.set_screen(Screen::Offscreen);
    let capabilities = game.capabilities();
    for command in super::COMMANDS {
        assert_eq!(
            capabilities.accepts(command),
            command != "window/place",
            "{command}"
        );
    }
    for id in [7, 8] {
        let reply = session.handle(&mut game, request(id, "screenshot", &json!({})));
        assert!(reply.response.is_none(), "answered after the next frame");
    }
    assert!(game.wants_screenshot());
    game.screenshot_failed("the GPU did not finish the frame");
    assert!(!game.wants_screenshot());
    let responses = game.take_responses();
    let ids: Vec<_> = responses.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(ids, [Id::Number(7), Id::Number(8)]);
    for (_, outcome) in responses {
        let error = outcome.unwrap_err();
        assert_eq!(error.code, ErrorCode::InternalError);
        assert!(
            error.message.contains("did not finish"),
            "{}",
            error.message
        );
    }
}

// spec BRIDGE-2.6: a headless host with no GPU device draws nothing, so it leaves screenshot
// out of its capabilities and the session refuses one as unsupported; the rest are as the
// window's
#[test]
fn a_headless_host_with_no_device_offers_no_screenshot() {
    let (mut game, mut session) = game();
    game.set_screen(Screen::Headless);
    let capabilities = game.capabilities();
    for command in super::COMMANDS {
        assert_eq!(
            capabilities.accepts(command),
            !matches!(command, "screenshot" | "window/place"),
            "{command}"
        );
    }
    let reply = session.handle(&mut game, request(7, "screenshot", &json!({})));
    let Some(Message::Response {
        outcome: Err(error),
        ..
    }) = reply.response
    else {
        panic!("a headless screenshot was not refused");
    };
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(!game.wants_screenshot());
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
        Rc::new(|_| None),
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
    assert!(idle.host_action().is_none());
    let (mut game, _session) = game_with("{\n\"classname\" \"info_exit\"\n}\n");
    let mut change = None;
    for _ in 0..10 {
        game.frame(0.1);
        change = level(game.host_action());
        if change.is_some() {
            break;
        }
    }
    let next = change.expect("a level change");
    assert_eq!(next.map, "e1m2");
    assert_eq!(
        next.host,
        game.world().host_state(),
        "the host's state goes on"
    );
    assert_eq!(
        next.carried.map(|kept| kept.parms()[0]),
        Some(100.0),
        "parm1 is the player's health"
    );
    assert!(game.host_action().is_none(), "taken once");
}

// spec PHYS-10.6: restart asks for this level again, with the parms it was entered with: none
// in a new game
#[test]
fn a_restart_loads_this_level_again() {
    let (mut game, _session) = game_with("{\n\"classname\" \"info_restart\"\n}\n");
    let mut change = None;
    for _ in 0..10 {
        game.frame(0.1);
        change = level(game.host_action());
        if change.is_some() {
            break;
        }
    }
    let next = change.expect("a restart");
    assert_eq!(next.map, "test");
    assert!(next.carried.is_none(), "a new game carries nothing");
}

/// The level `action` asks for, if it asks for one.
fn level(action: Option<HostAction>) -> Option<NextLevel> {
    match action {
        Some(HostAction::Level(next)) => Some(next),
        _ => None,
    }
}

/// The line `text` starts on in [`SOURCE`], counted from 1: the test progs' `t.qc`.
fn source_line(text: &str) -> i64 {
    let at = SOURCE.find(text).unwrap();
    i64::try_from(SOURCE.get(..at).unwrap().matches('\n').count() + 1).unwrap()
}

/// The events since the last call with method `method`.
fn events_named(game: &mut Game, method: &str) -> Vec<Value> {
    game.take_events()
        .into_iter()
        .filter(|(m, _)| *m == method)
        .map(|(_, params)| params)
        .collect()
}

// spec WORLD-9.1, WORLD-9.4: a breakpoint at a line of a monster's think stops the frame there,
// raising breakpoint/hit with its location and self, then state/changed to paused
// spec WORLD-9.3, WORLD-9.2: stepping over and out replays the frame; resume runs it on
// spec WORLD-9.5: self's fields are inspected and watched where it stopped
#[test]
fn a_scripted_debug_session() {
    let dog = "{\n\"classname\" \"monster_dog\"\n\"origin\" \"0 0 40\"\n}\n";
    let (mut game, mut session) = game_with(dog);
    let line = source_line("    self.health = 50;");
    let set = call(
        &mut game,
        &mut session,
        "breakpoint/set",
        &json!({ "file": "t.qc", "line": line }),
    )
    .unwrap();
    assert_eq!(set["location"]["function"], "dog_think");
    assert_eq!(set["location"]["line"], line);
    let id = set["id"].clone();
    let missing = call(
        &mut game,
        &mut session,
        "breakpoint/set",
        &json!({ "file": "t.qc", "line": 10_000 }),
    );
    assert_eq!(missing.unwrap_err().code, ErrorCode::NoSuchBreakpoint);

    // The dog thinks within a frame or two and stops on the breakpoint's line.
    for _ in 0..5 {
        let ran = game.frame(0.1);
        if game.state == super::State::Paused {
            assert!(
                !ran,
                "the stopped frame has not run, so the client's clock stands"
            );
            assert!(
                game.world_mut().take_messages().packets.is_empty(),
                "nor are its first messages sent"
            );
            break;
        }
    }
    let hits = events_named(&mut game, "breakpoint/hit");
    assert_eq!(hits.len(), 1, "one hit");
    assert_eq!(hits[0]["breakpoint"], id);
    assert_eq!(hits[0]["self"], json!(2), "the dog is edict 2");
    assert_eq!(hits[0]["location"]["line"], line);
    assert!(!game.frame(0.1), "paused, no frame runs");
    // spec BRIDGE-4.6: what changes the world at once waits until the frame has ended
    for (method, params) in [
        ("console", json!({ "command": "sv_gravity 1" })),
        ("entity/spawn", json!({ "classname": "info_null" })),
        ("entity/remove", json!({ "edict": 2 })),
        (
            "player/spawn_at",
            json!({ "origin": [0, 0, 0], "angles": [0, 0, 0] }),
        ),
    ] {
        let refused = call(&mut game, &mut session, method, &params).unwrap_err();
        assert_eq!(refused.code, ErrorCode::StoppedInFrame, "{method}");
    }

    // inspect self and watch it, where it stopped: frags has gone up, health not yet set.
    let watched = |game: &mut Game, session: &mut Session, text: &str| {
        call(game, session, "evaluate", &json!({ "expression": text })).unwrap()["value"].clone()
    };
    assert_eq!(
        watched(&mut game, &mut session, "self.frags"),
        json!({ "type": "float", "value": 1.0 })
    );
    assert_eq!(
        watched(&mut game, &mut session, "self.health"),
        json!({ "type": "float", "value": 0.0 })
    );
    let fields = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "edict", "edict": 2 }),
    )
    .unwrap();
    let frags = fields["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "frags")
        .unwrap()["value"]
        .clone();
    assert_eq!(frags, json!({ "type": "float", "value": 1.0 }));
    let stack = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "stack" }),
    )
    .unwrap();
    assert_eq!(stack["stack"][0]["function"], "dog_think");
    assert_eq!(stack["stack"][0]["index"], 0);

    // over: the line runs, and it stops on the next.
    call(&mut game, &mut session, "step", &json!({ "kind": "over" })).unwrap();
    assert_eq!(
        watched(&mut game, &mut session, "self.health"),
        json!({ "type": "float", "value": 50.0 })
    );
    let changed = events_named(&mut game, "state/changed");
    assert_eq!(changed.last().unwrap()["reason"], "step");
    assert_eq!(
        changed.last().unwrap()["location"]["line"],
        source_line("    growl();")
    );

    // in, then out of growl.
    call(&mut game, &mut session, "step", &json!({ "kind": "in" })).unwrap();
    let stack = call(
        &mut game,
        &mut session,
        "inspect",
        &json!({ "what": "stack" }),
    )
    .unwrap();
    assert_eq!(stack["stack"][0]["function"], "growl");
    assert_eq!(stack["stack"][1]["function"], "dog_think");
    call(&mut game, &mut session, "step", &json!({ "kind": "out" })).unwrap();
    assert_eq!(
        watched(&mut game, &mut session, "self.count"),
        json!({ "type": "float", "value": 9.0 })
    );

    // resume: the frame runs on and the game runs; the next think stops again.
    call(
        &mut game,
        &mut session,
        "breakpoint/clear",
        &json!({ "id": id }),
    )
    .unwrap();
    assert_eq!(
        call(
            &mut game,
            &mut session,
            "breakpoint/clear",
            &json!({ "id": id })
        )
        .unwrap_err()
        .code,
        ErrorCode::NoSuchBreakpoint
    );
    call(&mut game, &mut session, "resume", &json!({})).unwrap();
    assert_eq!(game.state, super::State::Running);
    for _ in 0..4 {
        assert!(game.frame(0.1));
    }
    assert!(
        events_named(&mut game, "breakpoint/hit").is_empty(),
        "cleared"
    );
}

// spec WORLD-9.1: with no debugger a frame runs for the frametime it is given, as without one
#[test]
fn a_frame_runs_for_its_frametime() {
    let (mut game, _session) = game();
    let start = game.world().time();
    assert!(game.frame(0.05));
    assert!(game.frame(0.025));
    let ran = game.world().time() - start;
    assert!((ran - 0.075).abs() < 1e-6, "ran {ran}");
}

/// Gives `game` the search path a reload reads: `progs.dat` and `progs.lno` of `source`.
fn serve(game: &mut Game, source: &str) {
    let (bytes, lno) = progs_of(source);
    let lno = lno.write().unwrap();
    game.set_files(Rc::new(move |name: &str| match name {
        "progs.dat" => Some(bytes.clone()),
        "progs.lno" => Some(lno.clone()),
        _ => None,
    }));
}

/// The dog's `frags` after the frames run so far.
fn dog_frags(game: &Game) -> f32 {
    let progs = Progs::read(game.progs()).unwrap();
    game.world()
        .edict_fields(&progs, 2)
        .unwrap()
        .into_iter()
        .find(|f| f.name == "frags")
        .map(|f| match f.value {
            qs_sim::QcValue::Float(value) => value,
            _ => panic!("frags is a float"),
        })
        .unwrap()
}

/// A dog at edict 2.
const DOG: &str = "{\n\"classname\" \"monster_dog\"\n\"origin\" \"0 0 40\"\n}\n";

// spec BRIDGE-4.9: reload loads the progs over the running map, migrating its values, and its
// result counts them and lists what was lost and added; it sends progs/loaded
// spec WORLD-10.4: the dog's think is rebound by name, so its new code runs
#[test]
fn a_reload_changes_a_monsters_think_while_it_runs() {
    let (mut game, mut session) = game_with(DOG);
    for _ in 0..3 {
        game.frame(0.1);
    }
    let before = dog_frags(&game);
    assert!(before > 0.0);
    serve(
        &mut game,
        &SOURCE
            .replace(
                "self.frags = self.frags + 1;",
                "self.frags = self.frags + 10;",
            )
            .replace(".float count;", ".float count; .float bark;"),
    );
    game.take_events();
    let result = call(&mut game, &mut session, "reload", &json!({})).unwrap();
    assert!(result["migrated"].as_u64().unwrap() > 0);
    assert_eq!(result["lost"], json!([]));
    assert_eq!(
        result["added"],
        json!([{ "name": "bark", "kind": "field", "reason": "added" }])
    );
    assert_eq!(methods(&events(&mut game)), ["progs/loaded"]);
    game.frame(0.1);
    game.frame(0.1);
    assert!(
        dog_frags(&game) >= before + 10.0,
        "the new think ran: {}",
        dog_frags(&game)
    );
}

// spec BRIDGE-4.11: a dry run answers with the report the reload then gives, a lost think
// and a renamed field included, and changes nothing: no event, the old code still runs
#[test]
fn a_dry_run_predicts_the_reload_and_changes_nothing() {
    let (mut game, mut session) = game_with(DOG);
    for _ in 0..3 {
        game.frame(0.1);
    }
    serve(
        &mut game,
        &SOURCE
            .replace("dog_think", "dog_wait")
            .replace("count", "tally"),
    );
    game.take_events();
    let before = dog_frags(&game);
    let predicted = call(
        &mut game,
        &mut session,
        "reload",
        &json!({ "dry_run": true }),
    )
    .unwrap();
    assert!(events(&mut game).is_empty(), "a dry run sends nothing");
    let lost = predicted["lost"].as_array().unwrap();
    assert!(
        lost.iter()
            .any(|c| c["name"] == "dog_think" && c["kind"] == "function"),
        "{predicted}"
    );
    assert!(
        lost.iter()
            .any(|c| c["name"] == "count" && c["kind"] == "field"),
        "{predicted}"
    );
    game.frame(0.1);
    game.frame(0.1);
    assert!(dog_frags(&game) > before, "the old think still runs");
    let time = game.world().time();
    let mut reloaded = call(&mut game, &mut session, "reload", &json!({})).unwrap();
    let mut predicted = predicted;
    // The prediction was read two frames earlier; nothing else differs.
    predicted["time"] = json!(time);
    reloaded["time"] = json!(time);
    assert_eq!(predicted["lost"], reloaded["lost"]);
    assert_eq!(predicted["added"], reloaded["added"]);
    assert_eq!(
        predicted["breakpoints_cleared"],
        reloaded["breakpoints_cleared"]
    );
}

// spec BRIDGE-4.9: a reload while paused keeps the game paused and says so with reason reload
// spec BRIDGE-4.8: one naming a file that is not there is refused with nothing changed
#[test]
fn a_reload_while_paused_stays_paused() {
    let (mut game, mut session) = game_with(DOG);
    serve(&mut game, SOURCE);
    let missing = call(
        &mut game,
        &mut session,
        "reload",
        &json!({ "progs": "nothere.dat" }),
    )
    .unwrap_err();
    assert_eq!(missing.code, ErrorCode::InvalidParams);
    call(&mut game, &mut session, "pause", &json!({})).unwrap();
    game.take_events();
    call(&mut game, &mut session, "reload", &json!({})).unwrap();
    let events = events(&mut game);
    assert_eq!(methods(&events), ["progs/loaded", "state/changed"]);
    assert_eq!(events[1].1["reason"], "reload");
    assert_eq!(game.state, super::State::Paused);
}

// spec BRIDGE-4.8: a breakpoint is resolved again at its file and line in the reloaded progs
// spec BRIDGE-4.7: a reload while a frame is stopped at a statement is refused
#[test]
fn a_reload_keeps_breakpoints_at_their_lines() {
    let (mut game, mut session) = game_with(DOG);
    let line = source_line("    self.health = 50;");
    call(
        &mut game,
        &mut session,
        "breakpoint/set",
        &json!({ "file": "t.qc", "line": line }),
    )
    .unwrap();
    for _ in 0..5 {
        game.frame(0.1);
        if game.state == super::State::Paused {
            break;
        }
    }
    serve(
        &mut game,
        &SOURCE.replace("self.health = 50;", "self.health = 60;"),
    );
    let refused = call(&mut game, &mut session, "reload", &json!({})).unwrap_err();
    assert_eq!(refused.code, ErrorCode::StoppedInFrame);
    call(&mut game, &mut session, "resume", &json!({})).unwrap();
    game.take_events();
    call(&mut game, &mut session, "reload", &json!({})).unwrap();
    assert_eq!(game.breakpoints.len(), 1, "kept");
    for _ in 0..5 {
        game.frame(0.1);
        if game.state == super::State::Paused {
            break;
        }
    }
    let hits = events_named(&mut game, "breakpoint/hit");
    assert_eq!(hits.len(), 1, "it stops at the same line of the new progs");
    assert_eq!(hits[0]["location"]["line"], line);
}

// spec BRIDGE-4.8: a breakpoint whose line has no statement in the reloaded progs is cleared,
// and the result lists its id; the reload's own writes send no edict/changed
#[test]
fn a_reload_lists_the_breakpoints_it_cleared() {
    let (mut game, mut session) = game_with(DOG);
    let line = source_line("    self.health = 50;");
    let set = call(
        &mut game,
        &mut session,
        "breakpoint/set",
        &json!({ "file": "t.qc", "line": line }),
    )
    .unwrap();
    serve(&mut game, &SOURCE.replace("    self.health = 50;", ""));
    game.take_events();
    let result = call(&mut game, &mut session, "reload", &json!({})).unwrap();
    assert_eq!(result["breakpoints_cleared"], json!([set["id"]]));
    assert!(game.breakpoints.is_empty());
    assert_eq!(methods(&events(&mut game)), ["progs/loaded"]);
    game.frame(0.1);
    let changed = events_named(&mut game, "edict/changed");
    assert!(
        changed.len() <= 2,
        "only what the frame changed: {changed:?}"
    );
}

// spec BRIDGE-4.10: a window the host can place offers window/place, and a placement asked for
// is taken once by the next frame; a window on Wayland, or no window, does not offer it
#[test]
fn window_place_is_kept_for_the_next_frame() {
    let (mut game, mut session) = game();
    assert!(game.capabilities().accepts("window/place"));
    let params = json!({ "x": 1290, "y": 25.5, "width": 800, "height": 528, "above": true });
    let reply = session.handle(&mut game, request(7, "window/place", &params));
    let Some(Message::Response {
        outcome: Ok(result),
        ..
    }) = reply.response
    else {
        panic!("window/place was not answered");
    };
    assert_eq!(result, Value::Null);
    assert_eq!(
        game.take_placement(),
        Some(Placement {
            x: 1290.0,
            y: 25.5,
            width: 800.0,
            height: 528.0,
            above: true,
        })
    );
    assert_eq!(game.take_placement(), None, "taken once");
    for screen in [Screen::FixedWindow, Screen::Offscreen, Screen::Headless] {
        game.set_screen(screen);
        assert!(!game.capabilities().accepts("window/place"), "{screen:?}");
    }
    game.set_screen(Screen::FixedWindow);
    assert!(
        game.capabilities().accepts("screenshot"),
        "a fixed window still draws"
    );
}

// spec BRIDGE-4.2, BRIDGE-4.10: a width or height below 1, or a missing member, is invalid
// params and places nothing
#[test]
fn window_place_refuses_a_bad_frame() {
    let (mut game, mut session) = game();
    for params in [
        json!({ "x": 0, "y": 0, "width": 0, "height": 528, "above": true }),
        json!({ "x": 0, "y": 0, "width": 800, "height": 0.5, "above": false }),
        json!({ "x": 0, "y": 0, "width": 800, "height": 528 }),
    ] {
        let reply = session.handle(&mut game, request(7, "window/place", &params));
        let Some(Message::Response {
            outcome: Err(error),
            ..
        }) = reply.response
        else {
            panic!("{params} was not refused");
        };
        assert_eq!(error.code, ErrorCode::InvalidParams, "{params}");
    }
    assert_eq!(game.take_placement(), None);
}
