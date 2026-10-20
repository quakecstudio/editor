// SPDX-License-Identifier: GPL-3.0-or-later

//! The host's errors and the end of a scenario (DIFF-8): the error records of the errors it
//! raises while spawning the map (EDICT-1.7, EDICT-6.2, EDICT-8.8), the entity key conversion
//! (EDICT-8.2), and the shutdown after a runtime, host or fatal error (DIFF-8.3 to DIFF-8.6).

#![expect(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed expect or overflow is a failed test"
)]

use qc_trace::Record;

use super::super::fixture::{FuncSpec, ProgsBuilder, Stmt};
use super::super::{Ended, run_scenario};
use super::{EMPTY_BODY_FUNCTION, progs_with_spawn, scenario};

/// The `owner` system field, an entity (EDICT-3.1).
const OWNER_SLOT: u32 = 95;
/// The `solid` system field (EDICT-3.1), which the host sets on the world (DIFF-3.4).
const SOLID_SLOT: u32 = 9;
/// An edict's size in bytes for the fixture's 110 field slots: 110 * 4 + 96 (EDICT-1.5).
const EDICT_SIZE: u32 = 536;

/// The error records of a trace, as (function, statement, message).
fn errors(records: &[Record]) -> Vec<(i32, i32, String)> {
    let mut found = Vec::new();
    for record in records {
        if let Record::Error {
            function,
            statement,
            message,
            ..
        } = record
        {
            let text = String::from_utf8_lossy(message.as_bytes()).into_owned();
            found.push((*function, *statement, text));
        }
    }
    found
}

/// The indices of the run records of function `function`.
fn runs_of(records: &[Record], function: i32) -> Vec<usize> {
    let mut found = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if let Record::Run { function: f, .. } = record
            && *f == function
        {
            found.push(index);
        }
    }
    found
}

/// The world's (edict 0) `owner` bits in the first run record's change set, if written.
fn world_owner(records: &[Record]) -> Option<u32> {
    let changes = records.iter().find_map(|r| match r {
        Record::Run { changes, .. } => Some(changes),
        _ => None,
    })?;
    changes
        .fields()
        .iter()
        .find(|f| f.edict == 0 && f.field == OWNER_SLOT)
        .map(|f| f.bits)
}

/// Spawns a map whose world has `"owner" "<value>"`.
fn spawn_with_owner(value: &str) -> super::super::RunOutput {
    let bytes = progs_with_spawn("worldspawn");
    let entities = format!("{{\n\"classname\" \"worldspawn\"\n\"owner\" \"{value}\"\n}}\n");
    run_scenario(&scenario("test"), &bytes, entities.as_bytes()).expect("the run is attempted")
}

// spec EDICT-8.2: an entity key stores the entity value of edict atoi(value), id's
// EDICT_TO_PROG(EDICT_NUM(atoi(s))): "1" is edict 1's value, the edict size, not 1.
#[test]
fn an_entity_key_stores_the_edicts_entity_value() {
    let output = spawn_with_owner("1");
    assert_eq!(output.ended, Ended::Normally);
    assert_eq!(world_owner(&output.trace.records), Some(EDICT_SIZE));
    let output = spawn_with_owner("599");
    assert_eq!(world_owner(&output.trace.records), Some(599 * EDICT_SIZE));
}

// spec EDICT-1.7: an entity key whose number is negative or not less than 600 is the fatal
// EDICT_NUM: bad number <n>, written as the host's error record (DIFF-8.1) before any spawn
// function runs; the map is loading, so the scenario ends (DIFF-8.6).
#[test]
fn an_entity_key_out_of_range_is_a_bad_number() {
    for (value, message) in [
        ("-1", "EDICT_NUM: bad number -1"),
        ("600", "EDICT_NUM: bad number 600"),
    ] {
        let output = spawn_with_owner(value);
        assert_eq!(output.ended, Ended::Error);
        let records = &output.trace.records;
        assert_eq!(errors(records), vec![(0, -1, message.to_owned())]);
        assert!(matches!(records.last(), Some(Record::Error { .. })));
        assert!(runs_of(records, EMPTY_BODY_FUNCTION).is_empty(), "{value}");
    }
}

// spec EDICT-6.2: a map with more entities than the array holds is the fatal
// ED_Alloc: no free edicts, written as the host's error record with function 0 and statement
// -1 (DIFF-8.1): with maxclients 1 the world and 598 more spawn, and the next fails.
#[test]
fn a_full_edict_array_while_spawning_is_no_free_edicts() {
    let bytes = progs_with_spawn("worldspawn");
    let mut entities = String::new();
    for _ in 0..601 {
        entities.push_str("{\n\"classname\" \"worldspawn\"\n}\n");
    }
    let output =
        run_scenario(&scenario("test"), &bytes, entities.as_bytes()).expect("the run is attempted");
    assert_eq!(output.ended, Ended::Error);
    let records = &output.trace.records;
    assert_eq!(
        errors(records),
        vec![(0, -1, "ED_Alloc: no free edicts".to_owned())]
    );
    assert_eq!(runs_of(records, EMPTY_BODY_FUNCTION).len(), 599);
    assert!(matches!(records.last(), Some(Record::Error { .. })));
}

// spec EDICT-8.8: text that is not `{` where an entity should start is the error
// ED_LoadFromFile: found <token> when expecting {, written as the host's error record with
// function 0 and statement -1 (DIFF-8.1).
// spec TRACE-3.1: that record carries the host's changes since the header, as the oracle's
// qs_trace_error does: the time global set to 1 and the world's solid set to SOLID_BSP.
#[test]
fn a_bad_entity_lump_writes_the_hosts_error_record() {
    let bytes = progs_with_spawn("worldspawn");
    let output = run_scenario(&scenario("test"), &bytes, b"notbrace\n").expect("attempted");
    assert_eq!(output.ended, Ended::Error);
    let changes = output
        .trace
        .records
        .iter()
        .find_map(|r| match r {
            Record::Error { changes, .. } => Some(changes),
            _ => None,
        })
        .expect("an error record");
    assert!(
        changes
            .globals()
            .iter()
            .any(|g| g.slot == 31 && g.bits == 1.0_f32.to_bits())
    );
    assert!(
        changes
            .fields()
            .iter()
            .any(|f| f.edict == 0 && f.field == SOLID_SLOT && f.bits == 4.0_f32.to_bits())
    );
    assert_eq!(
        errors(&output.trace.records),
        vec![(
            0,
            -1,
            "ED_LoadFromFile: found notbrace when expecting {".to_owned()
        )]
    );
}

/// What the 30th function of the `PlayerPreThink` chain does.
#[derive(Clone, Copy)]
enum Last {
    /// Calls builtin 10, `error` (a host error, BUILTIN-3.2).
    Error,
    /// Calls function 0 (the runtime error `NULL function`, VM-14.2).
    NullCall,
    /// Calls builtin 6, `break` (the fatal error `break statement`, BUILTIN-3.3).
    Break,
}

/// The global slot holding the function the chain's function `j` calls is `CHAIN_SLOTS + j`.
const CHAIN_SLOTS: i16 = 95;
/// The global slot holding `inner`, which `ClientDisconnect` calls.
const INNER_SLOT: i16 = 125;
/// The table index of `ClientDisconnect` in [`deep_progs`].
const DISCONNECT: i32 = 8;

/// A progs whose `PlayerPreThink` calls a chain 30 QC functions deep, the last doing `last`,
/// and whose `ClientDisconnect` makes one QC call, as the oracle scenario `depth-kept` does.
///
/// Table indices: 1 `worldspawn` and 2 to 6 the other entry points share the empty body; 7 is
/// the builtin `last` calls; 8 `ClientDisconnect`; 9 `inner`; 10 `PlayerPreThink`; 11 to 39
/// `deep_1` to `deep_29`.
fn deep_progs(last: Last) -> Vec<u8> {
    let empty = |name: &'static str| FuncSpec {
        name,
        first_statement: 1,
        ..FuncSpec::default()
    };
    let builtin = match last {
        Last::Error | Last::NullCall => -10,
        Last::Break => -6,
    };
    let mut builder = ProgsBuilder::default()
        // 1: the empty body; 2-3: ClientDisconnect calls inner.
        .statement(Stmt::op("DONE", 0, 0, 0))
        .statement(Stmt::op("CALL0", INNER_SLOT, 0, 0))
        .statement(Stmt::op("DONE", 0, 0, 0))
        .function(empty("worldspawn"))
        .function(empty("StartFrame"))
        .function(empty("PlayerPostThink"))
        .function(empty("ClientConnect"))
        .function(empty("PutClientInServer"))
        .function(empty("SetNewParms"))
        .function(FuncSpec {
            name: "builtin",
            first_statement: builtin,
            ..FuncSpec::default()
        })
        .function(FuncSpec {
            name: "ClientDisconnect",
            first_statement: 2,
            ..FuncSpec::default()
        })
        .function(empty("inner"))
        .global(slot_index(INNER_SLOT), 9);
    // The chain: function j (0 is PlayerPreThink, table index 10 + j) is at statement 4 + 2j.
    let names = std::iter::once("PlayerPreThink").chain([
        "deep_1", "deep_2", "deep_3", "deep_4", "deep_5", "deep_6", "deep_7", "deep_8", "deep_9",
        "deep_10", "deep_11", "deep_12", "deep_13", "deep_14", "deep_15", "deep_16", "deep_17",
        "deep_18", "deep_19", "deep_20", "deep_21", "deep_22", "deep_23", "deep_24", "deep_25",
        "deep_26", "deep_27", "deep_28", "deep_29",
    ]);
    for (j, name) in (0_i16..).zip(names) {
        let slot = CHAIN_SLOTS + j;
        builder = builder
            .statement(Stmt::op("CALL0", slot, 0, 0))
            .statement(Stmt::op("DONE", 0, 0, 0))
            .function(FuncSpec {
                name,
                first_statement: i32::from(4 + 2 * j),
                ..FuncSpec::default()
            });
        let callee = if j < 29 {
            u32::try_from(11 + j).expect("a function index")
        } else {
            match last {
                Last::Error | Last::Break => 7,
                Last::NullCall => 0,
            }
        };
        builder = builder.global(slot_index(slot), callee);
    }
    builder.build()
}

/// A global slot as the builder's index.
fn slot_index(slot: i16) -> usize {
    usize::try_from(slot).expect("a global slot")
}

/// Runs [`deep_progs`] with one client for one frame.
fn run_deep(last: Last) -> super::super::RunOutput {
    let mut scn = scenario("test");
    scn.clients = 1;
    scn.frames = 1;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    run_scenario(&scn, &deep_progs(last), entities).expect("the scenario runs")
}

// spec BUILTIN-3.2, DIFF-8.4: after `error` 30 calls deep the depth is kept, so the shutdown's
// ClientDisconnect run starts at 30 and its first QC call fails with stack overflow, which
// ends the scenario (DIFF-8.5), as the oracle scenario depth-kept pins.
#[test]
fn the_shutdown_after_a_host_error_starts_at_its_depth() {
    let output = run_deep(Last::Error);
    assert_eq!(output.ended, Ended::Error);
    let records = &output.trace.records;
    let found = errors(records);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].2, "");
    assert_eq!(found[1], (DISCONNECT, 2, "stack overflow".to_owned()));
    assert_eq!(runs_of(records, DISCONNECT).len(), 1);
    assert!(matches!(records.last(), Some(Record::Error { .. })));
}

// spec DIFF-8.4: a runtime error 30 calls deep sets the depth to 0, so the same shutdown runs
// ClientDisconnect and its call to the end.
#[test]
fn the_shutdown_after_a_runtime_error_starts_at_depth_0() {
    let output = run_deep(Last::NullCall);
    assert_eq!(output.ended, Ended::Error);
    let records = &output.trace.records;
    let found = errors(records);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].2, "NULL function");
    assert_eq!(runs_of(records, DISCONNECT).len(), 1);
    assert!(matches!(records.last(), Some(Record::Exit { .. })));
}

// spec DIFF-8.6: a fatal error writes its record and ends the scenario with no shutdown:
// ClientDisconnect never runs after `break`.
#[test]
fn a_fatal_error_runs_no_shutdown() {
    let output = run_deep(Last::Break);
    assert_eq!(output.ended, Ended::Error);
    let records = &output.trace.records;
    assert_eq!(errors(records).len(), 1);
    assert_eq!(errors(records)[0].2, "break statement");
    assert!(runs_of(records, DISCONNECT).is_empty());
    assert!(matches!(records.last(), Some(Record::Error { .. })));
}
