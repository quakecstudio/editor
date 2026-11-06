// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_progs::Progs;

use super::EdictChange;
use crate::world::tests::{entity, ledge, running};
use crate::world::{NamedValue, QcValue};

fn field(name: &str, value: QcValue) -> NamedValue {
    NamedValue {
        name: name.to_owned(),
        value,
    }
}

// spec BRIDGE-3.1: after a frame the host has each edict allocated and freed, in order, then
// each edict still in use with every field written; nothing is recorded before tracking
// starts
// spec BRIDGE-3.2: a field is listed once with its value after the frame, a vector whole, and
// a field written back to its old value is listed too
#[test]
fn a_frame_reports_spawns_removals_and_written_fields() {
    let source = "entity made, boxed, crated;\n\
        void() box = { boxed = self; };\n\
        void() crate = { self.health = 5; crated = self; };\n\
        void() StartFrame = {\n\
            if (boxed) {\n\
                boxed.health = 7; boxed.origin = '1 2 3'; boxed.origin_x = 4;\n\
                remove(boxed); boxed = world;\n\
            }\n\
            crated.health = crated.health; crated.origin = '1 2 3'; crated.origin_x = 4;\n\
            made = spawn(); made.classname = \"made\";\n\
        };\n";
    let entities = format!(
        "{}{}",
        entity("box", "0 0 0", ""),
        entity("crate", "0 0 0", "")
    );
    let mut run = running(&ledge(), source, &entities, Vec::new());
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    assert!(run.world.take_changes(&progs).is_empty());

    run.world.track_changes(true);
    run.frames(1, 0.1).unwrap();
    let changes = run.world.take_changes(&progs);
    // The box's edict, freed at time 0, is taken again at once (EDICT-6.3), and its writes
    // before the free are not the new edict's.
    let (reused, crate_n) = (2, 3);
    assert_eq!(
        changes,
        [
            EdictChange::Removed(reused),
            EdictChange::Spawned {
                edict: reused,
                classname: "made".to_owned()
            },
            EdictChange::Changed {
                edict: reused,
                fields: vec![field("classname", QcValue::String(b"made".to_vec()))]
            },
            EdictChange::Changed {
                edict: crate_n,
                fields: vec![
                    field("origin", QcValue::Vector([4.0, 2.0, 3.0])),
                    field("health", QcValue::Float(5.0))
                ]
            },
        ]
    );
    assert!(run.world.take_changes(&progs).is_empty());

    run.world.track_changes(false);
    run.frames(1, 0.1).unwrap();
    assert!(run.world.take_changes(&progs).is_empty());
}
