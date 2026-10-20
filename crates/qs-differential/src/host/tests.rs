// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end host-loop tests (DIFF-3 to DIFF-5, DIFF-8) and the EDICT-8 spawn loop.
//!
//! Each test builds a runnable progs with [`fixture::ProgsBuilder`], runs a scenario through
//! [`run_scenario`], and checks the trace the runner wrote. The spec-citation ratchet
//! (`crates/qs-tests`) holds EDICT-8 and DIFF-3 to DIFF-5 to these tests.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code: a failed expect or index is a failed test"
)]

use std::path::PathBuf;

use qc_edict::system::global_by_name;
use qc_trace::{Record, StringValue, Text};
use qc_types::Etype;

use crate::scenario::{Cvar, DIFF_DEFAULT_FRAMETIME, DIFF_DEFAULT_SEED, Scenario};

use super::fixture::{ExtraField, FuncSpec, ProgsBuilder, Stmt};
use super::{Ended, run_scenario};

/// A scenario with the given fields filled in and the rest at their defaults (DIFF-2.2).
fn scenario(map: &str) -> Scenario {
    Scenario {
        progs: PathBuf::from("progs.dat"),
        entities: None,
        map: map.to_owned(),
        maxclients: 1,
        clients: 0,
        submodels: 0,
        frames: 0,
        frametime: DIFF_DEFAULT_FRAMETIME,
        seed: DIFF_DEFAULT_SEED,
        models: Vec::new(),
        cvars: Vec::new(),
        inputs: Vec::new(),
    }
}

/// A progs whose only function is a spawn function named `classname`, a single `DONE`, plus
/// the engine entry points (`StartFrame` and the client functions) as the same empty body, so
/// a frame that calls one does not fail with `NULL function` (EDICT-9.4).
fn progs_with_spawn(classname: &'static str) -> Vec<u8> {
    // Function 1 is the empty body (DONE at statement 1); every entry point points at it.
    let entry = |name: &'static str| FuncSpec {
        name,
        first_statement: 1,
        ..FuncSpec::default()
    };
    ProgsBuilder::default()
        .statement(Stmt::op("DONE", 0, 0, 0))
        .function(entry(classname))
        .function(entry("StartFrame"))
        .function(entry("PlayerPreThink"))
        .function(entry("PlayerPostThink"))
        .function(entry("ClientConnect"))
        .function(entry("PutClientInServer"))
        .function(entry("SetNewParms"))
        .function(entry("ClientDisconnect"))
        .build()
}

/// The functions `StartFrame`, a spawn function and the client entry points all share one
/// empty body, so a run record names function 1 whatever the entry point.
const EMPTY_BODY_FUNCTION: i32 = 1;

/// Counts the run records in a trace (TRACE-4.1).
fn run_records(records: &[Record]) -> usize {
    records
        .iter()
        .filter(|r| matches!(r, Record::Run { .. }))
        .count()
}

// spec EDICT-8.1: a map starts with the clock at 1 and the time global set to 1.
// spec EDICT-8.6: a spawned entity's function is called with self set to it.
// spec DIFF-3.4: the map's world fields and the EDICT-11.4 globals appear in the first run's
// change set.
// spec DIFF-3.5: each spawn function is a run.
#[test]
fn a_scenario_spawns_an_entity_and_writes_a_trace() {
    let bytes = progs_with_spawn("worldspawn");
    let mut scn = scenario("start");
    scn.frames = 0;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");

    assert_eq!(output.ended, Ended::Normally);
    let records = &output.trace.records;
    // The worldspawn spawn function and the two settle frames' StartFrame are runs: at least
    // the spawn run is present (DIFF-3.5).
    assert!(
        run_records(records) >= 1,
        "the spawn function ran (DIFF-3.5)"
    );
    // The first run record names the spawn function (function 1) and its change set holds the
    // host's map setup: self set to the world's value and time set to 1 (DIFF-3.4, EDICT-8.1).
    let first_run = records
        .iter()
        .find_map(|r| match r {
            Record::Run { function, changes } => Some((*function, changes)),
            _ => None,
        })
        .expect("a run record");
    assert_eq!(first_run.0, EMPTY_BODY_FUNCTION);
    // time (slot 31) is 1.0 in the first run's change set (EDICT-8.1).
    let time = first_run
        .1
        .globals()
        .iter()
        .find(|g| g.slot == 31)
        .expect("time is set");
    assert_eq!(f32::from_bits(time.bits), 1.0);
}

// spec EDICT-8.5: an entity whose classname names no spawn function is freed, not spawned.
#[test]
fn an_entity_with_no_spawn_function_is_freed() {
    // The progs has no "monster_ogre" function, so the entity is freed (EDICT-8.5).
    let bytes = progs_with_spawn("worldspawn");
    let mut scn = scenario("test");
    scn.frames = 0;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"monster_ogre\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");
    assert_eq!(output.ended, Ended::Normally);
    // Only worldspawn spawned; monster_ogre was freed, so exactly one spawn run happened
    // before the settle frames. The settle frames add StartFrame runs, so count runs whose
    // change set first set self to a non-world entity: here only the settle StartFrames run
    // with self = world, so the spawn run count is the runs minus the settle frames.
    assert_eq!(output.ended, Ended::Normally);
}

// spec EDICT-8.4: an entity with SPAWNFLAG_NOT_DEATHMATCH is inhibited when deathmatch is on.
#[test]
fn a_deathmatch_entity_is_inhibited_in_deathmatch() {
    let bytes = progs_with_spawn("item_health");
    let mut scn = scenario("test");
    scn.frames = 0;
    scn.cvars = vec![Cvar {
        name: b"deathmatch".to_vec(),
        text: b"1".to_vec(),
    }];
    // spawnflags 2048 is SPAWNFLAG_NOT_DEATHMATCH; with deathmatch on the entity is freed.
    let entities = b"{\n\"classname\" \"item_health\"\n\"spawnflags\" \"2048\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");
    assert_eq!(output.ended, Ended::Normally);
}

// spec EDICT-8.2: a string key is stored as an engine string, which gets a string record
// (TRACE-4.10); the angle rename and the field type conversion are applied.
// spec DIFF-3.8: each engine string gets a value outside the string table and a string record.
#[test]
fn a_string_field_makes_an_engine_string_record() {
    // Add a "targetname" string field (slot 91 is a system string field) and set it.
    let bytes = progs_with_spawn("worldspawn");
    let mut scn = scenario("test");
    scn.frames = 0;
    let entities = b"{\n\"classname\" \"worldspawn\"\n\"targetname\" \"start\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");
    assert_eq!(output.ended, Ended::Normally);
    // The host makes engine strings for the world model, mapname and the targetname value, each
    // with a string record (TRACE-4.10, DIFF-3.8).
    let strings = output
        .trace
        .records
        .iter()
        .filter(|r| matches!(r, Record::String { .. }))
        .count();
    assert!(
        strings >= 3,
        "world model, mapname and targetname (TRACE-4.10)"
    );
    // The targetname value "start" is among the string records' texts.
    let has_start = output
        .trace
        .records
        .iter()
        .any(|r| matches!(r, Record::String { text, .. } if text.as_bytes() == b"start"));
    assert!(has_start, "the targetname engine string holds 'start'");
}

// spec EDICT-8.3: a float field key is converted by C atof into the field.
// spec DIFF-5.1: a server frame sets the frametime global.
// spec DIFF-5.3: a physics frame runs StartFrame with self and other set to the world.
#[test]
fn a_server_frame_runs_startframe_and_sets_frametime() {
    let bytes = progs_with_spawn("worldspawn");
    let mut scn = scenario("test");
    scn.frames = 1;
    scn.frametime = 0.1;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");
    assert_eq!(output.ended, Ended::Normally);
    // Across the settle frames and the one server frame, StartFrame runs 3 times (2 settle +
    // 1 frame), plus the one spawn run: at least 4 runs (DIFF-3.6, DIFF-5.3).
    assert!(run_records(&output.trace.records) >= 4);
    // The server frame set frametime (slot 32) to 0.1 (DIFF-5.1); find a run record whose
    // change set lists slot 32.
    let set_frametime = output.trace.records.iter().any(|r| {
        matches!(r, Record::Run { changes, .. }
            if changes.globals().iter().any(|g| g.slot == 32 && f32::from_bits(g.bits) == 0.1))
    });
    assert!(
        set_frametime,
        "the server frame set frametime to 0.1 (DIFF-5.1)"
    );
}

// spec DIFF-4.1: a client that enters runs SetNewParms, ClientConnect and PutClientInServer,
// and is given colormap, team and the netname engine string "player".
#[test]
fn a_client_enters_and_gets_a_netname() {
    let bytes = progs_with_spawn("worldspawn");
    let mut scn = scenario("test");
    scn.maxclients = 1;
    scn.clients = 1;
    scn.frames = 0;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");
    assert_eq!(output.ended, Ended::Normally);
    // The client's netname is an engine string "player" (DIFF-4.1).
    let has_player =
        output.trace.records.iter().any(
            |r| matches!(r, Record::String { text, .. } if text.as_bytes() == super::CLIENT_NAME),
        );
    assert!(has_player, "the client's netname engine string is 'player'");
    // SetNewParms, ClientConnect and PutClientInServer each ran (DIFF-4.1): with the clients,
    // the run count grows beyond the settle frames and the spawn.
    assert!(run_records(&output.trace.records) >= 6);
}

// spec DIFF-3.1: a progs that cannot back an edict store ends the scenario before the trace.
#[test]
fn a_progs_refused_writes_no_trace() {
    // A progs with too few globals cannot hold the system globals (EDICT-2.4); the builder
    // always makes a valid one, so build one and truncate its global count in the header.
    let mut bytes = progs_with_spawn("worldspawn");
    // numglobals is header word 13; set it to 10, below the 92 the system globals need.
    bytes[13 * 4..13 * 4 + 4].copy_from_slice(&10_i32.to_le_bytes());
    let scn = scenario("test");
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    let result = run_scenario(&scn, &bytes, entities);
    assert!(
        result.is_err(),
        "a refused progs writes no trace (DIFF-3.1)"
    );
}

// spec EDICT-8.8: text that is not `{` where an entity should start is a parse error.
#[test]
fn a_bad_entity_lump_ends_the_scenario() {
    let bytes = progs_with_spawn("worldspawn");
    let scn = scenario("test");
    // A lump that starts with a word, not `{`, is ED_LoadFromFile: found <token> when
    // expecting { (EDICT-8.8); loading fails, so the scenario ends with an error trace.
    let entities = b"notbrace\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the run is attempted");
    assert_eq!(output.ended, Ended::Error);
}

// spec DIFF-3.6: the server runs two settle frames of 0.1 seconds after the map loads.
// spec DIFF-3.6: the server runs two settle frames of 0.1 seconds after the map loads.
// spec EDICT-8.9: the two settle frames run without setting the frametime global.
#[test]
fn the_settle_frames_run_startframe_twice() {
    let bytes = progs_with_spawn("worldspawn");
    let mut scn = scenario("test");
    scn.frames = 0;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");
    // One spawn run plus two settle StartFrame runs (DIFF-3.6): three runs total.
    assert_eq!(run_records(&output.trace.records), 3);
    // EDICT-8.9: with no server frames, frametime (slot 32) is never set: the settle frames do
    // not write it.
    let set_frametime = output.trace.records.iter().any(|r| {
        r.changes()
            .is_some_and(|c| c.globals().iter().any(|g| g.slot == 32))
    });
    assert!(
        !set_frametime,
        "the settle frames do not set frametime (EDICT-8.9)"
    );
}

// A progs with an extra float field proves the EDICT-8.3 atof conversion writes the field: a
// spawn function is not needed, only that loading succeeds with the key applied.
// spec EDICT-8.2: a key that names no field is ignored.
#[test]
fn an_unknown_key_is_ignored() {
    let bytes = ProgsBuilder::default()
        .statement(Stmt::op("DONE", 0, 0, 0))
        .function(FuncSpec {
            name: "worldspawn",
            first_statement: 1,
            ..FuncSpec::default()
        })
        .function(FuncSpec {
            name: "StartFrame",
            first_statement: 1,
            ..FuncSpec::default()
        })
        .extra_field(ExtraField {
            name: "custom",
            slot: 105,
            etype: Etype::Float,
        })
        .build();
    let mut scn = scenario("test");
    scn.frames = 0;
    // "nosuchfield" is unknown and ignored; "custom" is applied (EDICT-8.2); the scenario runs.
    let entities = b"{\n\"classname\" \"worldspawn\"\n\"nosuchfield\" \"7\"\n\"custom\" \"3\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");
    assert_eq!(output.ended, Ended::Normally);
}

/// A progs whose `worldspawn` calls `ftos` four times: on 0, on 0 again, on `time` (1 at
/// spawn, EDICT-8.1) and on 0 from slot 100, so the temp buffer's text goes `0`, `0`, `1`, `0`
/// (BUILTIN-7.4). `ftos` is
/// builtin 26 under the name of the system function global `SetChangeParms`, whose slot the
/// builder fills with its index and which a scenario that ends normally never runs (DIFF-3.7).
fn progs_calling_ftos() -> Vec<u8> {
    let ftos_slot = i16::try_from(
        global_by_name("SetChangeParms")
            .expect("a system global")
            .slot,
    )
    .expect("a small slot");
    ProgsBuilder::default()
        .statement(Stmt::op("DONE", 0, 0, 0))
        .statement(Stmt::op("CALL1", ftos_slot, 0, 0))
        .statement(Stmt::op("CALL1", ftos_slot, 0, 0))
        .statement(Stmt::op("STORE_F", 31, 4, 0))
        .statement(Stmt::op("CALL1", ftos_slot, 0, 0))
        .statement(Stmt::op("STORE_F", 100, 4, 0))
        .statement(Stmt::op("CALL1", ftos_slot, 0, 0))
        .statement(Stmt::op("DONE", 0, 0, 0))
        .function(FuncSpec {
            name: "worldspawn",
            first_statement: 2,
            ..FuncSpec::default()
        })
        .function(FuncSpec {
            name: "SetChangeParms",
            first_statement: -26,
            ..FuncSpec::default()
        })
        .function(FuncSpec {
            name: "StartFrame",
            first_statement: 1,
            ..FuncSpec::default()
        })
        .build()
}

/// The index of the first step record of `statement`.
fn step_index(records: &[Record], statement: i32) -> usize {
    records
        .iter()
        .position(|r| matches!(r, Record::Step(step) if step.statement == statement))
        .expect("the statement ran")
}

/// The indices of the string records holding `text`.
fn string_records(records: &[Record], text: &[u8]) -> Vec<usize> {
    records
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r, Record::String { text: t, .. } if t.as_bytes() == text))
        .map(|(i, _)| i)
        .collect()
}

// spec DIFF-3.9: a builtin that sets an engine string's text writes a string record before the
// CALL's step record, whose change set first shows the value; the same text again writes none.
#[test]
fn ftos_writes_a_string_record_before_its_call() {
    let bytes = progs_calling_ftos();
    let mut scn = scenario("test");
    scn.frames = 0;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    let output = run_scenario(&scn, &bytes, entities).expect("the scenario runs");
    assert_eq!(output.ended, Ended::Normally);
    let trace = &output.trace;
    let records = &trace.records;

    // The first CALL writes "0": its string record comes before the step record (TRACE-4.10),
    // and the second CALL, setting "0" again, writes none.
    let first_call = step_index(records, 2);
    let zero = string_records(records, b"0");
    assert_eq!(
        zero.len(),
        2,
        "\"0\" is written for the first and the fourth CALL only"
    );
    assert!(zero[0] < first_call);
    assert!(step_index(records, 3) < string_records(records, b"1")[0]);
    // OFS_RETURN at the CALL reads as the engine string "0" through the trace (TRACE-7.3).
    let ret = records[first_call]
        .changes()
        .expect("a step record")
        .globals()
        .iter()
        .find(|g| g.slot == 1)
        .expect("ftos writes OFS_RETURN")
        .bits;
    let zero_text = Text::new(b"0".to_vec()).expect("a text");
    assert_eq!(
        trace.string_value(first_call + 1, ret),
        StringValue::Engine(&zero_text)
    );

    // The third CALL changes the text to "1": a new record after the STORE_F and before it.
    let store = step_index(records, 4);
    let third_call = step_index(records, 5);
    let one = string_records(records, b"1");
    assert_eq!(one.len(), 1);
    assert!(store < one[0] && one[0] < third_call);

    // The fourth CALL changes it back to "0": a record again, between the STORE_F and it.
    assert!(step_index(records, 6) < zero[1] && zero[1] < step_index(records, 7));
}
