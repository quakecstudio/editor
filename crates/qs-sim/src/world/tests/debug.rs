// SPDX-License-Identifier: GPL-3.0-or-later

//! The debugger (WORLD-9): breakpoints by line, stopping before a statement, stepping by
//! replaying the frame from a snapshot, and where a statement is.

use std::rc::Rc;

use crate::debug::BreakpointEdit;
use crate::spawn::LoadError;
use qc_progs::Progs;
use qc_vm::ErrorKind;

use super::{Running, compile_with_lines, entity, ledge, running_from};
use crate::debug::{Debugger, StepKind, Stop, locate, resolve};
use crate::{EvalError, LoadOptions, QcValue, World, WorldError};

/// A ticker whose think sets three fields on three lines and calls a helper on a fourth.
const SOURCE: &str = "void() helper = {
    self.ammo_shells = 4;
};
void() tick = {
    local float n;
    n = 5;
    self.frags = 1;
    self.health = 2;
    helper();
    self.armorvalue = 3;
    self.nextthink = time + 0.1;
};
void() ticker = { self.think = tick; self.nextthink = time + 0.05; };
";

/// The line `text` starts on in [`SOURCE`], counted from 1.
fn line_of(text: &str) -> i32 {
    let at = SOURCE.find(text).unwrap();
    i32::try_from(SOURCE.get(..at).unwrap().matches('\n').count() + 1).unwrap()
}

/// A world with the ticker and a debugger over its lines.
fn ticking() -> (Running, Vec<i32>) {
    let (progs, lines) = compile_with_lines(SOURCE);
    let mut run = running_from(
        &ledge(),
        progs,
        &entity("ticker", "0 0 0", ""),
        Vec::new(),
        LoadOptions::default(),
    );
    run.world.set_debugger(Some(Debugger::new(lines.clone())));
    (run, lines)
}

/// Runs frames of 0.1 until one stops; the world before that frame and where it stopped.
fn until_stopped(run: &mut Running) -> (World, Stop) {
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    for _ in 0..5 {
        let snapshot = run.world.clone();
        match run.world.frame(&progs, 0.1) {
            Ok(()) => {}
            Err(WorldError::Frame(error)) if error.kind == ErrorKind::Interrupted => {
                let stop = run.world.debugger().unwrap().stopped().unwrap();
                return (snapshot, stop);
            }
            Err(error) => panic!("{error}"),
        }
    }
    panic!("no stop")
}

/// Replays the frame from `snapshot` into `run`; the stop, if it stopped again.
fn replay(run: &mut Running, snapshot: &World) -> Option<Stop> {
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let debugger = run.world.debugger().cloned();
    run.world = snapshot.clone();
    run.world.set_debugger(debugger);
    match run.world.frame(&progs, 0.1) {
        Ok(()) => None,
        Err(WorldError::Frame(error)) if error.kind == ErrorKind::Interrupted => {
            run.world.debugger().and_then(Debugger::stopped)
        }
        Err(error) => panic!("{error}"),
    }
}

// spec WORLD-9.4: a breakpoint at a file and line is the first statement on that line of a
// function in that file; a line with none resolves to nothing
// spec WORLD-9.6: a frame stops before the breakpoint's statement, nothing of it run
#[test]
fn a_breakpoint_stops_the_think_before_its_line() {
    let (mut run, lines) = ticking();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let health = resolve(&progs, &lines, "t.qc", line_of("self.health = 2")).unwrap();
    assert_eq!(resolve(&progs, &lines, "t.qc", 1_000), None);
    assert_eq!(
        resolve(&progs, &lines, "other.qc", line_of("self.health = 2")),
        None
    );
    assert_eq!(
        resolve(&progs, &lines, "src/t.qc", line_of("self.health = 2")),
        Some(health),
        "a path ending in the file"
    );
    assert_eq!(
        resolve(
            &progs,
            &lines,
            "C:\\mod\\src\\t.qc",
            line_of("self.health = 2")
        ),
        Some(health),
        "a Windows path"
    );
    run.world
        .debugger_mut()
        .unwrap()
        .set_breakpoint(health, BreakpointEdit::Set);
    let (_, stop) = until_stopped(&mut run);
    assert_eq!(stop.statement, health);
    assert_eq!(stop.breakpoint, Some(health));
    assert_eq!(run.world.float(2, "frags"), 1.0, "the line before ran");
    assert_eq!(
        run.world.float(2, "health"),
        0.0,
        "the breakpoint's line did not"
    );
    let location = locate(&progs, &lines, stop.function, stop.statement);
    assert_eq!(location.function, "tick");
    assert_eq!(
        location.source,
        Some(("t.qc".to_owned(), line_of("self.health = 2")))
    );
}

// spec WORLD-9.2: going on replays the frame from its snapshot past the statements already
// run, so a frame with no other breakpoint runs to its end
// spec WORLD-9.3: over runs the line's call and stops on the next line; in stops in the call;
// out stops in the caller; a statement step stops at the next statement
#[test]
fn stepping_replays_the_frame() {
    let (mut run, lines) = ticking();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let call = resolve(&progs, &lines, "t.qc", line_of("helper();")).unwrap();
    run.world
        .debugger_mut()
        .unwrap()
        .set_breakpoint(call, BreakpointEdit::Set);
    let (snapshot, stop) = until_stopped(&mut run);
    assert_eq!(run.world.float(2, "health"), 2.0);

    run.world
        .debugger_mut()
        .unwrap()
        .step_from(StepKind::Over, &stop);
    let over = replay(&mut run, &snapshot).unwrap();
    assert_eq!(over.depth, stop.depth);
    assert_eq!(run.world.float(2, "ammo_shells"), 4.0, "the call ran");
    assert_eq!(
        run.world.float(2, "armorvalue"),
        0.0,
        "stopped before the next line"
    );
    assert_eq!(
        locate(&progs, &lines, over.function, over.statement).source,
        Some(("t.qc".to_owned(), line_of("self.armorvalue = 3")))
    );

    run.world
        .debugger_mut()
        .unwrap()
        .step_from(StepKind::In, &stop);
    let into = replay(&mut run, &snapshot).unwrap();
    assert!(into.depth > stop.depth, "into helper");
    assert_eq!(
        locate(&progs, &lines, into.function, into.statement).function,
        "helper"
    );

    run.world
        .debugger_mut()
        .unwrap()
        .step_from(StepKind::Out, &into);
    let out = replay(&mut run, &snapshot).unwrap();
    assert!(out.depth < into.depth);
    assert_eq!(
        locate(&progs, &lines, out.function, out.statement).function,
        "tick"
    );

    run.world
        .debugger_mut()
        .unwrap()
        .step_from(StepKind::Statement, &stop);
    let next = replay(&mut run, &snapshot).unwrap();
    assert_eq!(next.count, stop.count + 1);

    run.world
        .debugger_mut()
        .unwrap()
        .run_to_breakpoint(stop.count);
    run.world
        .debugger_mut()
        .unwrap()
        .set_breakpoint(call, BreakpointEdit::Clear);
    assert!(
        replay(&mut run, &snapshot).is_none(),
        "the frame runs to its end"
    );
    assert_eq!(run.world.float(2, "armorvalue"), 3.0);
}

// spec WORLD-9.6: without breakpoints, or set to finish, the frames run as without a debugger
#[test]
fn a_debugger_without_breakpoints_changes_nothing() {
    let (mut run, _) = ticking();
    assert!(!run.world.debugger().unwrap().may_stop());
    run.frames(3, 0.1).unwrap();
    assert_eq!(run.world.float(2, "armorvalue"), 3.0);
    run.world.debugger_mut().unwrap().finish();
    assert!(!run.world.debugger().unwrap().may_stop());
    run.frames(1, 0.1).unwrap();
}

// spec WORLD-9.2: a host snapshots a frame only when its debugger may stop it: a breakpoint
// set, or a step under way
#[test]
fn a_debugger_may_stop_only_with_a_breakpoint_or_a_step() {
    let mut debugger = Debugger::new(Vec::new());
    assert!(!debugger.may_stop());
    debugger.set_breakpoint(3, BreakpointEdit::Set);
    assert!(debugger.may_stop());
    debugger.set_breakpoint(3, BreakpointEdit::Clear);
    assert!(!debugger.may_stop(), "cleared");
    debugger.stop_at_first();
    assert!(debugger.may_stop(), "a step");
    debugger.run_to_breakpoint(0);
    assert!(!debugger.may_stop());
}

// spec WORLD-9.3: over and out stay in the run they started in, so stepping over the last
// line of one ticker's think does not stop in the next ticker's
#[test]
fn over_and_out_stop_only_in_their_own_run() {
    let (progs_bytes, lines) = compile_with_lines(SOURCE);
    let entities = format!(
        "{}{}",
        entity("ticker", "0 0 0", ""),
        entity("ticker", "64 0 0", "")
    );
    let mut run = running_from(
        &ledge(),
        progs_bytes,
        &entities,
        Vec::new(),
        LoadOptions::default(),
    );
    run.world.set_debugger(Some(Debugger::new(lines.clone())));
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let last = resolve(
        &progs,
        &lines,
        "t.qc",
        line_of("self.nextthink = time + 0.1"),
    )
    .unwrap();
    run.world
        .debugger_mut()
        .unwrap()
        .set_breakpoint(last, BreakpointEdit::Set);
    let (snapshot, stop) = until_stopped(&mut run);
    run.world
        .debugger_mut()
        .unwrap()
        .set_breakpoint(last, BreakpointEdit::Clear);
    for kind in [StepKind::Over, StepKind::Out] {
        run.world.debugger_mut().unwrap().step_from(kind, &stop);
        assert_eq!(
            replay(&mut run, &snapshot),
            None,
            "{kind:?}: the frame ends"
        );
        assert_eq!(
            run.world.float(3, "armorvalue"),
            3.0,
            "the other ticker ran"
        );
    }
    run.world
        .debugger_mut()
        .unwrap()
        .step_from(StepKind::In, &stop);
    let into = replay(&mut run, &snapshot).unwrap();
    assert!(into.run > stop.run, "in stops at whatever runs next");
}

// spec WORLD-9.5: a watch expression is a constant, a local of the frame's function before a
// global, or a field path read entity by entity
#[test]
fn watch_expressions_read_the_stopped_state() {
    let (mut run, lines) = ticking();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let health = resolve(&progs, &lines, "t.qc", line_of("self.health = 2")).unwrap();
    run.world
        .debugger_mut()
        .unwrap()
        .set_breakpoint(health, BreakpointEdit::Set);
    let (_, stop) = until_stopped(&mut run);
    let at = Some(stop.function);
    let eval = |text: &str| run.world.evaluate(&progs, at, text);
    assert_eq!(eval("self.frags"), Ok(QcValue::Float(1.0)));
    assert_eq!(eval(" self.health "), Ok(QcValue::Float(0.0)));
    assert_eq!(eval("n"), Ok(QcValue::Float(5.0)), "a local");
    assert_eq!(
        eval("self.owner.health"),
        Ok(QcValue::Float(0.0)),
        "the world's"
    );
    assert_eq!(eval("self"), Ok(QcValue::Entity(2)));
    assert_eq!(eval("42"), Ok(QcValue::Float(42.0)));
    assert_eq!(eval("'1 2 3'"), Ok(QcValue::Vector([1.0, 2.0, 3.0])));
    assert_eq!(eval("\"hi\""), Ok(QcValue::String(b"hi".to_vec())));
    assert_eq!(eval("nosuch"), Err(EvalError::Unknown("nosuch".to_owned())));
    let helper = progs
        .functions()
        .iter()
        .position(|f| progs.string(f.s_name) == Some(b"helper".as_slice()))
        .map(|i| i32::try_from(i).unwrap());
    assert_eq!(
        run.world.evaluate(&progs, helper, "n"),
        Err(EvalError::Unknown("n".to_owned())),
        "another function's local is not a global"
    );
    assert_eq!(
        eval("self.nosuch"),
        Err(EvalError::Unknown("nosuch".to_owned()))
    );
    assert_eq!(
        eval("self.frags.health"),
        Err(EvalError::NotAnEntity("self.frags".to_owned()))
    );
    assert_eq!(eval("1 + 2"), Err(EvalError::Syntax));
    assert_eq!(eval("'1 2'"), Err(EvalError::Syntax));
    assert_eq!(eval("self..frags"), Err(EvalError::Syntax));
}

/// A door whose spawn function sets fields on three lines.
const SPAWNING: &str = "void() door = {
    self.frags = 1;
    self.health = 2;
    self.armorvalue = 3;
};
";

/// Two doors' world before they spawn, its progs and the progs' lines.
fn prepared() -> (World, Vec<u8>, Vec<i32>) {
    let (progs_bytes, lines) = compile_with_lines(SPAWNING);
    let progs = Progs::read(&progs_bytes).unwrap();
    let lump = format!(
        "{{\n\"classname\" \"worldspawn\"\n}}\n{}{}",
        entity("door", "0 0 0", ""),
        entity("door", "64 0 0", "")
    );
    let mut map = super::bsp(&ledge());
    map.entity_lump = Box::leak(lump.into_bytes().into_boxed_slice());
    let read: crate::ModelFiles = Rc::new(|_: &str| None);
    let world = World::prepare(&progs, "test", &map, read, LoadOptions::default()).unwrap();
    (world, progs_bytes, lines)
}

/// Spawns `world`'s entities; the stop when the debugger stopped the spawning.
fn spawn(world: &mut World, progs: &Progs<'_>) -> Option<Stop> {
    match world.spawn_map(progs) {
        Ok(()) => None,
        Err(WorldError::Spawn(LoadError::Run(error))) if error.kind == ErrorKind::Interrupted => {
            world.debugger().and_then(Debugger::stopped)
        }
        Err(error) => panic!("{error}"),
    }
}

/// Each door's three fields.
fn doors(world: &World) -> Vec<[f32; 3]> {
    [2, 3]
        .map(|edict| {
            [
                world.float(edict, "frags"),
                world.float(edict, "health"),
                world.float(edict, "armorvalue"),
            ]
        })
        .to_vec()
}

// spec WORLD-9.6: a breakpoint in a spawn function stops the level's spawning before its line
// spec WORLD-9.7: going on replays the spawning from the prepared world, skipping the counted
// statements, to the next stop and then to the load a world with no debugger makes
#[test]
fn a_breakpoint_in_a_spawn_function_stops_the_load() {
    let (prepared, progs_bytes, lines) = prepared();
    let progs = Progs::read(&progs_bytes).unwrap();
    let health = resolve(&progs, &lines, "t.qc", 3).unwrap();
    let mut debugger = Debugger::new(lines);
    debugger.set_breakpoint(health, BreakpointEdit::Set);

    let mut world = prepared.clone();
    world.set_debugger(Some(debugger));
    let first = spawn(&mut world, &progs).expect("the first door stops");
    assert_eq!(first.breakpoint, Some(health));
    assert_eq!(
        doors(&world),
        [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
        "the first door stopped before its health"
    );

    let mut go_on = |stop: Stop| {
        let mut debugger = world.debugger().cloned().unwrap();
        debugger.run_to_breakpoint(stop.count);
        world = prepared.clone();
        world.set_debugger(Some(debugger));
        spawn(&mut world, &progs)
    };
    let second = go_on(first).expect("the second door stops at the same line");
    assert_eq!(second.breakpoint, Some(health));
    assert!(second.count > first.count);
    assert_eq!(
        go_on(second),
        None,
        "going on from the last stop finishes the load"
    );

    let mut plain = prepared.clone();
    plain.spawn_map(&progs).unwrap();
    assert_eq!(doors(&world), doors(&plain));
    assert_eq!(doors(&plain), [[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]]);
}

// spec WORLD-9.6: with a debugger and no breakpoint the spawning runs as without one
#[test]
fn a_load_with_no_breakpoint_runs_as_without_a_debugger() {
    let (prepared, progs_bytes, lines) = prepared();
    let progs = Progs::read(&progs_bytes).unwrap();
    let mut world = prepared.clone();
    world.set_debugger(Some(Debugger::new(lines)));
    assert_eq!(spawn(&mut world, &progs), None);
    let mut plain = prepared;
    plain.spawn_map(&progs).unwrap();
    assert_eq!(doors(&world), doors(&plain));
    assert_eq!(
        plain.spawn_map(&progs).ok(),
        Some(()),
        "nothing spawns twice"
    );
    assert_eq!(doors(&plain), [[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]]);
}

// spec WORLD-9.6: an entity spawned during play runs its spawn function without the debugger
#[test]
fn an_entity_spawned_in_play_runs_without_the_debugger() {
    let (mut world, progs_bytes, lines) = prepared();
    let progs = Progs::read(&progs_bytes).unwrap();
    world.spawn_map(&progs).unwrap();
    let health = resolve(&progs, &lines, "t.qc", 3).unwrap();
    let mut debugger = Debugger::new(lines);
    debugger.set_breakpoint(health, BreakpointEdit::Set);
    world.set_debugger(Some(debugger));
    let pairs = [("classname".to_owned(), "door".to_owned())];
    let edict = world.spawn_entity(&progs, &pairs).unwrap();
    assert_eq!(world.debugger().and_then(Debugger::stopped), None);
    assert_eq!(
        world.float(edict, "health"),
        2.0,
        "its spawn function ran to its end"
    );
}
