// SPDX-License-Identifier: GPL-3.0-or-later

//! What the preview host reports after a server frame (BRIDGE-3.1, BRIDGE-3.2): the edicts
//! allocated and freed, and each edict's fields written during it with their values then.

use std::collections::BTreeMap;

use qc_edict::EdictEvent;
use qc_progs::{Def, Progs};

use super::World;
use super::inspect::named;
use crate::world::NamedValue;

/// Something that happened to an edict since the last [`World::take_changes`].
#[derive(Clone, Debug, PartialEq)]
pub enum EdictChange {
    /// The edict was allocated (`ED_Alloc`), with its classname now.
    Spawned {
        /// Its number.
        edict: usize,
        /// Its `classname`, empty when it has none.
        classname: String,
    },
    /// The edict was freed (`ED_Free`).
    Removed(usize),
    /// Fields of the edict were written, a field written back to its old value included,
    /// each listed once with its value now (BRIDGE-3.2).
    Changed {
        /// Its number.
        edict: usize,
        /// The fields written, in their definitions' order.
        fields: Vec<NamedValue>,
    },
}

impl World {
    /// Starts or stops recording what happens to the edicts, for [`World::take_changes`].
    pub fn track_changes(&mut self, on: bool) {
        self.machine.edicts_mut().track(on);
    }

    /// What has happened to the edicts since the last call, while tracking is on: each
    /// allocation and free in order, then, for each edict still in use, lowest first, every
    /// field written (BRIDGE-3.2).
    pub fn take_changes(&mut self, progs: &Progs<'_>) -> Vec<EdictChange> {
        let events = self.machine.edicts_mut().take_events();
        let mut changes = Vec::new();
        // For each edict, whether each field definition was written.
        let defs = progs.field_defs();
        let mut written: BTreeMap<usize, Vec<bool>> = BTreeMap::new();
        for event in events {
            match event {
                // The writes to an edict before it was freed, or taken again, are not its
                // new life's.
                EdictEvent::Allocated(n) => {
                    written.remove(&n.index());
                    changes.push(EdictChange::Spawned {
                        edict: n.index(),
                        classname: String::new(),
                    });
                }
                EdictEvent::Freed(n) => {
                    written.remove(&n.index());
                    changes.push(EdictChange::Removed(n.index()));
                }
                EdictEvent::Written(n, slot) => {
                    let marks = written
                        .entry(n.index())
                        .or_insert_with(|| vec![false; defs.len()]);
                    for (mark, def) in marks.iter_mut().zip(defs.iter()) {
                        if covers(progs, def, slot) {
                            *mark = true;
                        }
                    }
                }
            }
        }
        for change in &mut changes {
            if let EdictChange::Spawned { edict, classname } = change {
                *classname = self.classname(progs, *edict);
            }
        }
        for (edict, marks) in written {
            if !self.in_use(edict) {
                continue;
            }
            let fields: Vec<NamedValue> = defs
                .iter()
                .zip(marks)
                .filter(|&(_, mark)| mark)
                .filter_map(|(def, _)| self.field_value(progs, edict, def))
                .collect();
            if !fields.is_empty() {
                changes.push(EdictChange::Changed { edict, fields });
            }
        }
        changes
    }
}

/// Whether field `def`, as [`named`] lists it, holds slot `slot`.
fn covers(progs: &Progs<'_>, def: Def, slot: usize) -> bool {
    named(progs, def).is_some_and(|(_, etype, first)| {
        slot >= first && slot < first.saturating_add(etype.slot_count())
    })
}

#[cfg(test)]
mod tests;
