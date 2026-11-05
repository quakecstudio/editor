// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_progs::Progs;
use qc_types::Slot;

use super::{RawState, read_state};
use crate::world::tests::{ledge, player_from, player_progs};
use crate::{LoadOptions, QcValue};

/// A raw state with nothing in it but `num_edicts` edicts, whose entity values name edict 3
/// and whose strings read `text`.
struct Fixed {
    edicts: usize,
}

impl RawState for Fixed {
    fn global(&self, _slot: usize) -> Slot {
        Slot::ZERO
    }

    fn field(&self, _edict: usize, _slot: usize) -> Slot {
        Slot::ZERO
    }

    fn num_edicts(&self) -> usize {
        self.edicts
    }

    fn entity(&self, _value: Slot) -> usize {
        3
    }

    fn string(&self, _progs: &Progs<'_>, _value: Slot) -> Vec<u8> {
        b"text".to_vec()
    }
}

// spec DIFF-16.2: the state is every named global and every field of every edict below
// num_edicts, the world's own reading of each
#[test]
fn the_state_is_what_inspect_reads_of_every_edict() {
    let run = player_from(
        &ledge(),
        player_progs("self.health = 42;", "", ""),
        LoadOptions::default(),
    );
    let progs = Progs::read(&run.progs).unwrap();
    let state = run.world.state(&progs);
    assert_eq!(state.globals, run.world.named_globals(&progs));
    assert_eq!(state.edicts.len(), run.world.num_edicts());
    assert_eq!(state.edicts[1], run.world.edict_fields(&progs, 1).unwrap());
}

// spec DIFF-16.2: entities and strings are read through the source, so two sources whose raw
// values differ read the same
#[test]
fn entities_and_strings_are_read_through_the_source() {
    let run = player_from(&ledge(), player_progs("", "", ""), LoadOptions::default());
    let progs = Progs::read(&run.progs).unwrap();
    let state = read_state(&progs, &Fixed { edicts: 2 });
    assert_eq!(state.edicts.len(), 2);
    let field = |name: &str| {
        state.edicts[1]
            .iter()
            .find(|v| v.name == name)
            .map(|v| v.value.clone())
    };
    assert_eq!(field("owner"), Some(QcValue::Entity(3)));
    assert_eq!(field("classname"), Some(QcValue::String(b"text".to_vec())));
    assert_eq!(field("health"), Some(QcValue::Float(0.0)));
}
