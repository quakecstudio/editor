// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_progs::Progs;
use qc_types::Etype;

use super::{Change, ChangeKind, Migration, Reason};
use crate::LoadOptions;
use crate::world::QcValue;
use crate::world::tests::{Running, compile, entity, ledge, running_from};

/// A ticker whose think `tick` sets `frags` to `frags`, and the declarations `extra` adds.
fn ticker(frags: &str, extra: &str) -> String {
    format!(
        "{extra}float counter; float pace = 10; string label;\n\
         void() tick = {{ self.frags = {frags}; counter = counter + 1; label = \"ticked\"; \
         self.nextthink = time + 0.1; }};\n\
         void() ticker = {{ self.think = tick; self.nextthink = time + 0.05; \
         self.enemy = self; }};\n"
    )
}

/// The ticker's world, run for three frames of 0.1.
fn running(source: &str) -> Running {
    let mut run = running_from(
        &ledge(),
        compile(source),
        &entity("ticker", "0 0 0", ""),
        Vec::new(),
        LoadOptions::default(),
    );
    run.frames(3, 0.1).unwrap();
    run
}

/// Reloads `run` with `source`; the migration.
fn reload(run: &mut Running, source: &str) -> Migration {
    let new = compile(source);
    let old_bytes = core::mem::replace(&mut run.progs, new);
    let old = Progs::read(&old_bytes).unwrap();
    let new = Progs::read(&run.progs).unwrap();
    run.world.reload(&old, &new).unwrap()
}

/// Field `name` of edict `n`, read through the running progs.
fn field(run: &Running, n: usize, name: &str) -> QcValue {
    let progs = Progs::read(&run.progs).unwrap();
    run.world
        .edict_fields(&progs, n)
        .unwrap()
        .into_iter()
        .find(|f| f.name == name)
        .map(|f| f.value)
        .unwrap()
}

/// Global `name`, read through the running progs.
fn global(run: &Running, name: &str) -> QcValue {
    let progs = Progs::read(&run.progs).unwrap();
    run.world
        .named_globals(&progs)
        .into_iter()
        .find(|g| g.name == name)
        .map(|g| g.value)
        .unwrap()
}

// spec WORLD-10.1: the map keeps running: the edicts, their number and the time are kept
// spec WORLD-10.4: a function value is rebound by name, so a changed think takes effect
#[test]
fn a_changed_think_takes_effect_without_a_restart() {
    let mut run = running(&ticker("1", ""));
    assert_eq!(field(&run, 2, "frags"), QcValue::Float(1.0));
    let (edicts, time) = (run.world.num_edicts(), run.world.time());
    let migration = reload(&mut run, &ticker("2", ""));
    assert!(migration.lost.is_empty(), "{migration:?}");
    assert_eq!(run.world.num_edicts(), edicts);
    assert!((run.world.time() - time).abs() < f64::EPSILON);
    assert_eq!(
        field(&run, 2, "think"),
        QcValue::Function("tick".to_owned())
    );
    run.frames(2, 0.1).unwrap();
    assert_eq!(
        field(&run, 2, "frags"),
        QcValue::Float(2.0),
        "the new think ran"
    );
}

// spec WORLD-10.2: a global the game changed keeps its value; one still at its initial value
// takes the new progs' initial value
// spec WORLD-10.4: a string the new progs' table lacks is kept by its text
#[test]
fn globals_keep_what_the_game_changed() {
    let mut run = running(&ticker("1", ""));
    let counter = global(&run, "counter");
    reload(&mut run, &ticker("1", "").replace("pace = 10", "pace = 20"));
    assert_eq!(global(&run, "counter"), counter, "carried");
    assert_eq!(
        global(&run, "pace"),
        QcValue::Float(20.0),
        "the edit's value"
    );
    assert_eq!(global(&run, "label"), QcValue::String(b"ticked".to_vec()));
    let mut gone = running(&ticker("1", ""));
    reload(
        &mut gone,
        &ticker("1", "").replace("\"ticked\"", "\"other\""),
    );
    assert_eq!(
        global(&gone, "label"),
        QcValue::String(b"ticked".to_vec()),
        "an engine string now"
    );
}

// spec WORLD-10.3: an added field is reported and starts at 0; the edicts' other fields keep
// their values, an entity still naming its edict when the edict's size changes
#[test]
fn an_added_field_is_reported_and_the_others_kept() {
    let mut run = running(&ticker("1", ""));
    let migration = reload(&mut run, &ticker("1", ".float extra;\n"));
    assert_eq!(
        migration.added,
        [Change {
            kind: ChangeKind::Field,
            name: "extra".to_owned(),
            reason: Reason::Added,
        }]
    );
    assert!(migration.migrated > 0);
    assert_eq!(field(&run, 2, "extra"), QcValue::Float(0.0));
    assert_eq!(
        field(&run, 2, "enemy"),
        QcValue::Entity(2),
        "self, by its edict"
    );
    assert_eq!(field(&run, 2, "frags"), QcValue::Float(1.0));
    assert_eq!(
        field(&run, 2, "classname"),
        QcValue::String(b"ticker".to_vec())
    );
}

// spec WORLD-10.3: a field whose type changed is reported, and its values are not carried
// spec WORLD-10.5: the reason names both types
#[test]
fn a_changed_field_type_is_reported() {
    let source = |ty: &str| {
        ticker("1", &format!(".{ty} mark;\n")).replace(
            "self.enemy = self;",
            "self.enemy = self; self.mark = self.mark;",
        )
    };
    let mut run = running(&source("float"));
    let migration = reload(&mut run, &source("string"));
    assert_eq!(
        migration.lost,
        [Change {
            kind: ChangeKind::Field,
            name: "mark".to_owned(),
            reason: Reason::TypeChanged {
                from: Etype::Float,
                to: Etype::String,
            },
        }]
    );
    assert_eq!(
        migration.lost.first().unwrap().reason.to_string(),
        "type changed from float to string"
    );
}

// spec WORLD-10.4: a removed function is reported, and each value naming it is reported where
// it was, and is 0
#[test]
fn a_removed_function_is_reported_with_its_references() {
    let mut run = running(&ticker("1", ""));
    let renamed = ticker("1", "")
        .replace("void() tick", "void() tock")
        .replace("self.think = tick", "self.think = tock");
    let migration = reload(&mut run, &renamed);
    assert!(migration.lost.contains(&Change {
        kind: ChangeKind::Function,
        name: "tick".to_owned(),
        reason: Reason::Removed,
    }));
    assert!(migration.lost.contains(&Change {
        kind: ChangeKind::Function,
        name: "tick".to_owned(),
        reason: Reason::Referenced("edict 2 .think".to_owned()),
    }));
    assert!(migration.added.iter().any(|c| c.name == "tock"));
    assert_eq!(field(&run, 2, "think"), QcValue::Function(String::new()));
    let referenced = migration
        .lost
        .iter()
        .find(|c| matches!(c.reason, Reason::Referenced(_)))
        .unwrap();
    assert_eq!(
        referenced.reason.to_string(),
        "removed, and edict 2 .think named it: now 0"
    );
}

// spec WORLD-10.2: a removed global is reported
// spec WORLD-10.5: each change has its kind's name and a reason
#[test]
fn a_removed_global_is_reported() {
    let mut run = running(&ticker("1", "float spare;\n"));
    let migration = reload(&mut run, &ticker("1", ""));
    assert_eq!(
        migration.lost,
        [Change {
            kind: ChangeKind::Global,
            name: "spare".to_owned(),
            reason: Reason::Removed,
        }]
    );
    assert_eq!(Reason::Removed.to_string(), "removed");
    assert_eq!(ChangeKind::Global.name(), "global");
    assert_eq!(ChangeKind::Field.name(), "field");
    assert_eq!(ChangeKind::Function.name(), "function");
}
