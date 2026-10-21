// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the edict store, citing the EDICT statement each pins.
//!
//! The spawn, remove and reuse rows of `docs/spec/edicts.md` "Allocation and freeing" are the
//! acceptance vectors (EDICT-6, EDICT-7); the layout, numbering, system global and system
//! field rows of "Layout" are here too (EDICT-1, EDICT-2, EDICT-3).

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap, assert or index is a failed test with a message, and \
              the f64-to-f32 casts reproduce the engine's rounding"
)]

use qc_progs::limits::LIMIT_MAX_EDICTS;
use qc_types::{EntityRef, Etype, Slot};

use crate::system::{
    SYSTEM_FIELDS, SYSTEM_GLOBALS, field_at_slot, field_by_name, global_at_slot, global_by_name,
};
use crate::{
    EDICT_HEADER_SIZE, EDICT_MAX_FIELDS, EDICT_SYS_FIELDS_END, EDICT_SYS_GLOBALS_END, EdictError,
    EdictLayout, EdictStore, LayoutError,
};

/// The id 1.01 field count the spec's vectors use: 105 system fields, no mod fields.
const FIELDS_105: usize = 105;
/// A global slot count that holds the system globals.
const GLOBALS_128: usize = 128;

fn layout_105() -> EdictLayout {
    EdictLayout::new(FIELDS_105, GLOBALS_128).expect("105 fields and 128 globals are valid")
}

fn store(maxclients: usize) -> EdictStore {
    EdictStore::new(layout_105(), maxclients).expect("maxclients in range")
}

// ---- EDICT-1: entity memory and numbering ----

// spec EDICT-1.1: an edict is 96 bytes of header plus 4 bytes per field slot; 105 slots is 516.
// spec EDICT-1.3: the header size is fixed at 96, the 32-bit x86 value.
#[test]
fn edict_size_is_header_plus_fields() {
    let layout = layout_105();
    assert_eq!(EDICT_HEADER_SIZE, 96);
    assert_eq!(layout.entityfields(), 105);
    assert_eq!(layout.edict_size(), 96 + 4 * 105);
    assert_eq!(layout.edict_size(), 516);
}

// spec EDICT-1.4: the array has 600 edicts; num_edicts starts at maxclients + 1 and never
// shrinks.
#[test]
fn array_size_and_num_edicts_start() {
    assert_eq!(LIMIT_MAX_EDICTS, 600);
    let one = store(1);
    assert_eq!(one.num_edicts(), 2);
    let eight = store(8);
    assert_eq!(eight.num_edicts(), 9);
    // Every edict 0..600 exists, free or not (the array holds all of them).
    assert!(one.fields(0).is_some());
    assert!(one.fields(599).is_some());
    assert!(one.fields(600).is_none());
}

// spec EDICT-1.5: an entity value is the edict number times the edict size; edict 3 is 1548,
// edict 0 is 0 (the world).
#[test]
fn entity_value_is_number_times_edict_size() {
    let layout = layout_105();
    assert_eq!(layout.entity_value(0), Some(EntityRef(0)));
    assert_eq!(layout.entity_value(3), Some(EntityRef(1548)));
    assert_eq!(layout.entity_value(1), Some(EntityRef(516)));
}

// spec EDICT-1.6: a pointer to field slot f of edict n is n * edict_size + 96 + 4 * f; slot 46
// of edict 2 is 1032 + 96 + 184 = 1312.
#[test]
fn pointer_value_adds_header_and_field_offset() {
    let layout = layout_105();
    let pointer = layout.pointer_value(2, 46).expect("slot 46 exists");
    assert_eq!(pointer.0, 1032 + 96 + 184);
    assert_eq!(pointer.0, 1312);
    // A slot not below entityfields names no pointer.
    assert!(layout.pointer_value(2, 105).is_none());
}

// spec EDICT-1.7: EDICT_NUM fails "bad number <n>" for n negative or >= 600; 599 is edict 599.
#[test]
fn edict_num_rejects_out_of_range() {
    let store = store(1);
    assert_eq!(
        store.edict_num(600),
        Err(EdictError::BadNumber { number: 600 })
    );
    assert_eq!(
        store.edict_num(-1),
        Err(EdictError::BadNumber { number: -1 })
    );
    assert_eq!(store.edict_num(599), Ok(EntityRef(599 * 516)));
}

// spec EDICT-1.7: NUM_FOR_EDICT fails "bad pointer" for a value at or above num_edicts.
#[test]
fn num_for_edict_rejects_at_or_above_num_edicts() {
    let store = store(1); // num_edicts is 2
    let at_num_edicts = store.layout().entity_value(2).expect("edict 2 value");
    assert_eq!(
        store.num_for_edict(at_num_edicts),
        Err(EdictError::BadPointer)
    );
    let below = store.layout().entity_value(1).expect("edict 1 value");
    assert_eq!(store.num_for_edict(below), Ok(1));
}

// spec EDICT-1.7: the messages are id's "EDICT_NUM: bad number 600" and
// "NUM_FOR_EDICT: bad pointer".
#[test]
fn numbering_error_messages_match_id() {
    assert_eq!(
        EdictError::BadNumber { number: 600 }.to_string(),
        "EDICT_NUM: bad number 600"
    );
    assert_eq!(
        EdictError::BadPointer.to_string(),
        "NUM_FOR_EDICT: bad pointer"
    );
    assert_eq!(
        EdictError::NoFreeEdicts.to_string(),
        "ED_Alloc: no free edicts"
    );
}

// spec EDICT-1.8: every map gets a zero-filled array, no edict free, num_edicts maxclients + 1.
#[test]
fn fresh_store_is_zero_filled_and_in_use() {
    let store = store(1);
    assert_eq!(store.num_edicts(), 2);
    for number in 0..LIMIT_MAX_EDICTS {
        assert_eq!(
            store.is_free(number),
            Some(false),
            "edict {number} free flag"
        );
        assert!(
            store
                .fields(number)
                .expect("edict exists")
                .iter()
                .all(|s| *s == Slot(0)),
            "edict {number} not zero-filled"
        );
    }
}

// spec EDICT-1.9: a value names edict n when it is n * edict_size for whole n in 0..600,
// whether or not n is below num_edicts or free; any other value names none.
#[test]
fn entity_value_names_an_edict_only_on_a_whole_multiple() {
    let store = store(1); // num_edicts is 2
    // Edict 10 names an edict even though it is at or above num_edicts.
    assert_eq!(store.layout().edict_number(EntityRef(10 * 516)), Some(10));
    // Edict 599 names one; 600 does not (past the array).
    assert_eq!(store.layout().edict_number(EntityRef(599 * 516)), Some(599));
    assert_eq!(store.layout().edict_number(EntityRef(600 * 516)), None);
    // A value that is not a whole multiple names none.
    assert_eq!(store.layout().edict_number(EntityRef(4)), None);
    // A negative value names none.
    assert_eq!(store.layout().edict_number(EntityRef(-1)), None);
}

// ---- EDICT-2: system globals ----

// spec EDICT-2.1: the 64 system globals at slots 28 to 91, with these names and types.
#[test]
fn system_globals_are_the_slots_of_edict_2_1() {
    // 54 named globals span the 64 slots 28 to 91 (five are vectors of 3 slots each).
    assert_eq!(SYSTEM_GLOBALS.len(), 54);
    let slots: usize = SYSTEM_GLOBALS.iter().map(|v| v.etype.slot_count()).sum();
    assert_eq!(slots, 64);
    assert_eq!(
        global_by_name("self").map(|v| (v.slot, v.etype)),
        Some((28, Etype::Entity))
    );
    assert_eq!(
        global_by_name("time").map(|v| (v.slot, v.etype)),
        Some((31, Etype::Float))
    );
    assert_eq!(
        global_by_name("parm1").map(|v| (v.slot, v.etype)),
        Some((43, Etype::Float))
    );
    assert_eq!(
        global_by_name("trace_ent").map(|v| (v.slot, v.etype)),
        Some((78, Etype::Entity))
    );
    assert_eq!(
        global_by_name("SetChangeParms").map(|v| (v.slot, v.etype)),
        Some((91, Etype::Function))
    );
    // The last system global is at slot 91; none is below 28 or above 91.
    assert!(SYSTEM_GLOBALS.iter().all(|v| v.slot >= 28 && v.slot <= 91));
    assert_eq!(global_at_slot(28).map(|v| v.name), Some("self"));
    assert_eq!(global_at_slot(91).map(|v| v.name), Some("SetChangeParms"));
}

// spec EDICT-2.2: end_sys_globals (slot 92) is not a system global; the engine does not use it.
#[test]
fn end_sys_globals_is_not_a_system_global() {
    assert_eq!(EDICT_SYS_GLOBALS_END, 92);
    assert!(global_at_slot(92).is_none());
    assert!(global_by_name("end_sys_globals").is_none());
}

// spec EDICT-2.3: the system globals are these slots whatever the progs' names say; they are
// never resolved by name. The table is fixed, independent of any progs.
#[test]
fn system_globals_are_fixed_by_slot() {
    // global_at_slot does not consult a progs; it is the fixed table of EDICT-2.1.
    for var in SYSTEM_GLOBALS {
        assert_eq!(global_at_slot(var.slot), Some(var));
    }
    // A slot inside a vector global names none (v_forward is 59; 60 is its y component).
    assert!(global_at_slot(60).is_none());
}

// spec EDICT-2.4: a progs with fewer than 92 global slots cannot hold the system globals and
// is refused with a typed error.
#[test]
fn too_few_globals_is_refused() {
    let err = EdictLayout::new(FIELDS_105, 91).unwrap_err();
    assert_eq!(err, LayoutError::GlobalsTooFew { globals: 91 });
    // 92 is enough.
    assert!(EdictLayout::new(FIELDS_105, 92).is_ok());
}

// ---- EDICT-3: system fields ----

// spec EDICT-3.1: the 105 system fields at slots 0 to 104, with these names and types.
#[test]
fn system_fields_are_the_slots_of_edict_3_1() {
    // 77 named fields span the 105 slots 0 to 104 (the vectors take 3 slots each).
    assert_eq!(SYSTEM_FIELDS.len(), 77);
    let slots: usize = SYSTEM_FIELDS.iter().map(|v| v.etype.slot_count()).sum();
    assert_eq!(slots, 105);
    assert_eq!(
        field_by_name("modelindex").map(|v| (v.slot, v.etype)),
        Some((0, Etype::Float))
    );
    assert_eq!(
        field_by_name("origin").map(|v| (v.slot, v.etype)),
        Some((10, Etype::Vector))
    );
    assert_eq!(
        field_by_name("think").map(|v| (v.slot, v.etype)),
        Some((44, Etype::Function))
    );
    assert_eq!(
        field_by_name("nextthink").map(|v| (v.slot, v.etype)),
        Some((46, Etype::Float))
    );
    assert_eq!(
        field_by_name("flags").map(|v| (v.slot, v.etype)),
        Some((76, Etype::Float))
    );
    assert_eq!(
        field_by_name("noise3").map(|v| (v.slot, v.etype)),
        Some((104, Etype::String))
    );
    assert_eq!(field_at_slot(10).map(|v| v.name), Some("origin"));
    // A slot inside a vector field names none (origin is 10; 11 is its y component).
    assert!(field_at_slot(11).is_none());
}

// spec EDICT-3.2: end_sys_fields (slot 105) is the engine's boundary; a mod's fields follow
// from 105, and the engine never looks a system field up by name.
#[test]
fn system_fields_end_at_105() {
    assert_eq!(EDICT_SYS_FIELDS_END, 105);
    assert!(field_at_slot(105).is_none());
    // The system fields never pass slot 104.
    assert!(SYSTEM_FIELDS.iter().all(|v| v.slot <= 104));
}

// spec EDICT-3.3: a progs whose entityfields is below 105 cannot hold the system fields and is
// refused with a typed error.
#[test]
fn too_few_fields_is_refused() {
    let err = EdictLayout::new(104, GLOBALS_128).unwrap_err();
    assert_eq!(err, LayoutError::FieldsTooFew { fields: 104 });
    assert!(EdictLayout::new(105, GLOBALS_128).is_ok());
}

// spec EDICT-3.4: non-system fields the engine reads are found by exact name over the field
// definitions; the fixed table has no gravity or items2.
#[test]
fn non_system_fields_are_not_in_the_fixed_table() {
    assert!(field_by_name("gravity").is_none());
    assert!(field_by_name("items2").is_none());
}

// ---- EDICT-6: allocation (spawn) ----

// spec EDICT-6.2: with nothing to reuse, spawn takes num_edicts and grows it; two spawns from
// a fresh map give edicts 2 and 3 and num_edicts 4.
#[test]
fn alloc_after_the_clients_grows_num_edicts() {
    let mut store = store(1);
    assert_eq!(store.alloc(1.0), Ok(2));
    assert_eq!(store.alloc(1.0), Ok(3));
    assert_eq!(store.num_edicts(), 4);
}

// spec EDICT-6.4: an edict freed while freetime is below 2 can be reused at once.
// (EDICT-8.7, the map-spawn consequence, is the preview host's; the rule here is EDICT-6.4.)
#[test]
fn alloc_reuses_in_the_first_seconds() {
    let mut store = store(1);
    assert_eq!(store.alloc(1.0), Ok(2));
    assert_eq!(store.alloc(1.0), Ok(3));
    store.free(2, 1.0); // freetime 1, below 2
    assert_eq!(store.alloc(1.0), Ok(2)); // reused at once
}

// spec EDICT-6.1: allocation scans from maxclients + 1 and takes the lowest qualifying edict.
#[test]
fn alloc_takes_the_lowest_qualifying_edict() {
    let mut store = store(4); // clients 1..4, num_edicts 5
    for expected in 5..=8 {
        assert_eq!(store.alloc(10.0), Ok(expected));
    }
    store.free(8, 10.0);
    store.free(6, 10.0);
    // At 11.0 both 6 and 8 are old enough; the lower, 6, is taken.
    assert_eq!(store.alloc(11.0), Ok(6));
}

// spec EDICT-6.3: the reuse delay is strict in double precision; 0.5 later is not reused, 0.6
// is. spec EDICT-6.1: an edict freed 0 seconds ago is not reused while in the window.
#[test]
fn alloc_waits_more_than_half_a_second() {
    let mut store = store(1);
    assert_eq!(store.alloc(5.0), Ok(2));
    store.free(2, 5.0); // freetime 5
    assert_eq!(store.alloc(5.0), Ok(3)); // edict 2 freed 0 seconds ago, not reused
    assert_eq!(store.alloc(5.5), Ok(4)); // exactly 0.5 is not more than 0.5
    assert_eq!(store.alloc(5.6), Ok(2)); // 0.6 > 0.5, edict 2 reused
}

// spec EDICT-6.4: freetime of exactly 2 is not below 2, so the half-second delay applies.
#[test]
fn alloc_waits_from_time_2() {
    let mut store = store(1);
    assert_eq!(store.alloc(2.0), Ok(2));
    store.free(2, 2.0); // freetime 2, not below 2
    assert_eq!(store.alloc(2.0), Ok(3)); // 0 seconds is not more than 0.5
}

// spec EDICT-6.3: freetime is an f32, so 100000.3 rounds to 100000.296875 and the measured
// delay differs slightly. spec EDICT-7.1: free sets freetime to the clock rounded to f32.
#[test]
fn alloc_freetime_is_rounded_to_f32() {
    let mut store = store(1);
    assert_eq!(store.alloc(100_000.3), Ok(2));
    store.free(2, 100_000.3);
    // 100000.3 rounds to 100000.296875 in f32 (EDICT-6.3): compare the exact f32 of the clock.
    assert_eq!(store.freetime(2), Some(100_000.3_f64 as f32));
    // At 100000.8 the edict was freed 0.503125 seconds ago, so it is reused.
    assert_eq!(store.alloc(100_000.8), Ok(2));
}

// spec EDICT-6.2: once num_edicts is 600 and nothing qualifies, spawn fails with
// "ED_Alloc: no free edicts".
#[test]
fn alloc_fails_at_600() {
    let mut store = store(1);
    // Spawn edicts 2..599 inclusive: 598 spawns, num_edicts becomes 600.
    for _ in 0..598 {
        store.alloc(10.0).expect("spawn below 600");
    }
    assert_eq!(store.num_edicts(), 600);
    assert_eq!(store.alloc(10.0), Err(EdictError::NoFreeEdicts));
}

// spec EDICT-6.2: even with a free edict that is too recently freed, spawn at 600 fails; once
// the delay passes, it reuses that edict.
#[test]
fn alloc_at_600_waits_for_the_delay() {
    let mut store = store(1);
    for _ in 0..598 {
        store.alloc(10.0).expect("spawn below 600");
    }
    store.free(300, 10.0); // freetime 10
    // At 10.2 edict 300 was freed 0.2 seconds ago: not old enough, and num_edicts is 600.
    assert_eq!(store.alloc(10.2), Err(EdictError::NoFreeEdicts));
    // At 10.6 it is 0.6 seconds, old enough: reused.
    assert_eq!(store.alloc(10.6), Ok(300));
}

// spec EDICT-6.5: the edict handed out is zero-filled with its free flag cleared, its header
// otherwise left as it was including freetime.
#[test]
fn alloc_clears_fields_but_keeps_freetime() {
    let mut store = store(1);
    assert_eq!(store.alloc(3.0), Ok(2));
    // Dirty every field slot.
    for slot in 0..105 {
        assert!(store.set_field(2, slot, Slot::from_f32(7.0)));
    }
    store.free(2, 3.25); // freetime 3.25
    // At 4.0 the edict is reused: fields all 0, freetime still 3.25.
    assert_eq!(store.alloc(4.0), Ok(2));
    assert!(store.fields(2).unwrap().iter().all(|s| *s == Slot(0)));
    assert_eq!(store.freetime(2), Some(3.25));
    assert_eq!(store.is_free(2), Some(false));
}

// spec EDICT-6.6: spawn returns the new edict's entity value (via its number and the layout).
#[test]
fn alloc_number_maps_to_its_entity_value() {
    let mut store = store(1);
    let number = store.alloc(1.0).expect("spawn");
    let value = store
        .edict_num(i32::try_from(number).unwrap())
        .expect("value");
    assert_eq!(value, EntityRef(2 * 516));
}

// spec EDICT-5.3: a client edict is never handed out by allocation; the scan starts after the
// clients. (The store models maxclients + 1 as the scan start of EDICT-6.1.)
#[test]
fn client_edicts_are_never_reused() {
    let mut store = store(8); // clients 1..8, num_edicts 9
    store.free(3, 1.0); // free a client's edict
    // The next spawn is edict 9, not the freed client edict 3.
    assert_eq!(store.alloc(1.0), Ok(9));
}

// ---- EDICT-7: freeing (remove) ----

// spec EDICT-7.1: free sets the free flag, freetime to the clock, zeroes the named fields and
// sets nextthink to -1.
// spec EDICT-7.2: every other field keeps its value.
#[test]
fn free_clears_some_fields_and_keeps_the_rest() {
    let mut store = store(1);
    assert_eq!(store.alloc(3.0), Ok(2));
    for slot in 0..105 {
        assert!(store.set_field(2, slot, Slot::from_f32(7.0)));
    }
    store.free(2, 3.25);
    assert_eq!(store.is_free(2), Some(true));
    assert_eq!(store.freetime(2), Some(3.25));
    // Zeroed: modelindex 0, solid 9, origin 10-12, angles 19-21, model 29, frame 30, skin 31,
    // takedamage 59, colormap 77.
    for slot in [0_usize, 9, 10, 11, 12, 19, 20, 21, 29, 30, 31, 59, 77] {
        assert_eq!(
            store.field(2, slot),
            Some(Slot(0)),
            "slot {slot} should be 0"
        );
    }
    // nextthink (46) is -1.
    assert_eq!(store.field(2, 46), Some(Slot::from_f32(-1.0)));
    // Everything else keeps 7, e.g. classname 28, health 48, owner 95, absmin 1, absmax 4.
    for slot in [1_usize, 4, 28, 48, 95, 104] {
        assert_eq!(
            store.field(2, slot),
            Some(Slot::from_f32(7.0)),
            "slot {slot} should keep its value"
        );
    }
}

// spec EDICT-7.3: num_edicts does not shrink when an edict is freed.
#[test]
fn free_does_not_shrink_num_edicts() {
    let mut store = store(1);
    store.alloc(1.0).expect("spawn");
    store.alloc(1.0).expect("spawn");
    assert_eq!(store.num_edicts(), 4);
    store.free(2, 1.0);
    assert_eq!(store.num_edicts(), 4);
}

// spec EDICT-7.4: freeing an already-free edict does it all again, moving freetime later and
// restarting the reuse delay.
#[test]
fn free_again_moves_freetime_and_restarts_the_delay() {
    let mut store = store(1);
    store.alloc(3.0).expect("spawn edict 2");
    store.alloc(3.0).expect("spawn edict 3");
    store.free(2, 4.0); // freetime 4
    store.free(2, 4.4); // freed again: freetime 4.4, delay restarts
    assert_eq!(store.freetime(2), Some(4.4));
    // At 4.6 only 0.2 seconds have passed since 4.4, so edict 2 is not reused, and edict 3 is
    // in use, so allocation takes num_edicts (4).
    assert_eq!(store.alloc(4.6), Ok(4));
    // At 5.0, 0.6 seconds since 4.4: edict 2 is reused.
    assert_eq!(store.alloc(5.0), Ok(2));
}

// spec EDICT-7.1: clear_edict (ED_ClearEdict) zeroes the fields and clears the free flag, the
// inverse the allocator uses on reuse.
#[test]
fn clear_edict_zeroes_fields_and_clears_free() {
    let mut store = store(1);
    store.alloc(1.0).expect("spawn edict 2");
    for slot in 0..105 {
        assert!(store.set_field(2, slot, Slot::from_f32(7.0)));
    }
    store.free(2, 1.0);
    store.clear_edict(2);
    assert_eq!(store.is_free(2), Some(false));
    assert!(store.fields(2).unwrap().iter().all(|s| *s == Slot(0)));
}

// ---- store housekeeping ----

#[test]
fn bad_maxclients_is_refused() {
    let err = EdictStore::new(layout_105(), 17).unwrap_err();
    assert_eq!(err, EdictError::BadMaxclients { maxclients: 17 });
    // 0 (the VM test environment) and 16 are accepted.
    assert!(EdictStore::new(layout_105(), 0).is_ok());
    assert!(EdictStore::new(layout_105(), 16).is_ok());
}

#[test]
fn field_access_by_offset_rejects_out_of_range() {
    let mut store = store(1);
    assert!(store.set_field(2, 104, Slot::from_f32(1.0)));
    assert_eq!(store.field(2, 104), Some(Slot::from_f32(1.0)));
    // Slot 105 does not exist on a 105-field progs.
    assert!(!store.set_field(2, 105, Slot::from_f32(1.0)));
    assert_eq!(store.field(2, 105), None);
    // Edict 600 does not exist.
    assert_eq!(store.field(600, 0), None);
}

// spec EDICT-2.4, EDICT-3.3, EDICT-3.5: each LayoutError prints the count and the limit it
// broke, so a reader sees why the progs was refused.
#[test]
fn layout_error_messages() {
    let globals = LayoutError::GlobalsTooFew { globals: 10 };
    assert_eq!(
        globals.to_string(),
        format!(
            "progs.dat has 10 global slots, fewer than the {EDICT_SYS_GLOBALS_END} the system \
             globals need"
        )
    );
    let fields = LayoutError::FieldsTooFew { fields: 3 };
    assert_eq!(
        fields.to_string(),
        format!(
            "progs.dat has 3 field slots, fewer than the {EDICT_SYS_FIELDS_END} the system \
             fields need"
        )
    );
    let many = LayoutError::FieldsTooMany { fields: 9_000_000 };
    assert_eq!(
        many.to_string(),
        format!(
            "progs.dat has 9000000 field slots, more than the {EDICT_MAX_FIELDS} an entity \
             value can address"
        )
    );
}

// spec EDICT-1.7, EDICT-6.2, EDICT-5.1: each EdictError prints id's message with its number.
#[test]
fn edict_error_messages() {
    assert_eq!(
        EdictError::BadNumber { number: -3 }.to_string(),
        "EDICT_NUM: bad number -3"
    );
    assert_eq!(
        EdictError::BadPointer.to_string(),
        "NUM_FOR_EDICT: bad pointer"
    );
    assert_eq!(
        EdictError::NoFreeEdicts.to_string(),
        "ED_Alloc: no free edicts"
    );
    assert_eq!(
        EdictError::BadMaxclients { maxclients: 20 }.to_string(),
        "maxclients 20 is not 0 to 16"
    );
}

// spec EDICT-3.5: a field count above EDICT_MAX_FIELDS (894760) is refused with FieldsTooMany,
// the guard that keeps the edict size inside an i32 entity value; the limit itself is allowed.
#[test]
fn too_many_fields_at_the_limit() {
    assert_eq!(EDICT_MAX_FIELDS, 894_760);
    assert!(EdictLayout::new(EDICT_MAX_FIELDS, GLOBALS_128).is_ok());
    let err = EdictLayout::new(EDICT_MAX_FIELDS + 1, GLOBALS_128).unwrap_err();
    assert_eq!(
        err,
        LayoutError::FieldsTooMany {
            fields: EDICT_MAX_FIELDS + 1
        }
    );
}

// spec EDICT-1.1: for_progs reads entityfields and the global slot count from a loaded progs
// and computes the same layout as EdictLayout::new.
#[test]
fn for_progs_matches_new() {
    use qc_progs::{Progs, ProgsFile, write};
    let file = ProgsFile {
        entityfields: i32::try_from(FIELDS_105).unwrap(),
        globals: vec![Slot(0); GLOBALS_128],
        ..ProgsFile::default()
    };
    let bytes = write(&file).unwrap();
    let progs = Progs::read(&bytes).unwrap();
    let from_progs = EdictLayout::for_progs(&progs).unwrap();
    assert_eq!(from_progs, layout_105());
    assert_eq!(from_progs.entityfields(), FIELDS_105);
}

// spec EDICT-1.6: pointer_value is None for a field slot not below entityfields, and a valid
// slot gives number * edict_size + 96 + 4 * field.
#[test]
fn pointer_value_bounds() {
    let layout = layout_105();
    // Field 105 is not below entityfields (105): None.
    assert_eq!(layout.pointer_value(1, 105), None);
    // Field 0 of edict 1: 1 * 516 + 96 + 0 = 612.
    assert_eq!(layout.pointer_value(1, 0).map(|p| p.0), Some(612));
    // Field 2 of edict 0: 0 + 96 + 8 = 104.
    assert_eq!(layout.pointer_value(0, 2).map(|p| p.0), Some(104));
}

// spec EDICT-1.9: edict_number names an edict only for a value that is a whole non-negative
// multiple of the edict size below LIMIT_MAX_EDICTS; a value for edict LIMIT_MAX_EDICTS or
// above names none.
#[test]
fn edict_number_rejects_out_of_range() {
    let layout = layout_105();
    let size = layout.edict_size();
    // Edict 0 and edict 1 are in range.
    assert_eq!(layout.edict_number(EntityRef(0)), Some(0));
    assert_eq!(layout.edict_number(EntityRef(size)), Some(1));
    // Edict LIMIT_MAX_EDICTS is out of range.
    let max = i32::try_from(LIMIT_MAX_EDICTS).unwrap();
    assert_eq!(layout.edict_number(EntityRef(size * max)), None);
    // A value that is not a whole multiple names no edict.
    assert_eq!(layout.edict_number(EntityRef(size + 1)), None);
}

// spec EDICT-1.2: freetime reports the server time an edict was last freed, rounded to f32,
// and None for a number the array does not hold.
#[test]
fn freetime_reports_the_free_clock() {
    let mut store = store(1);
    store.alloc(1.0).unwrap(); // edict 2
    assert_eq!(store.freetime(2), Some(0.0));
    store.free(2, 3.5);
    assert_eq!(store.freetime(2), Some(3.5));
    assert_eq!(store.freetime(LIMIT_MAX_EDICTS), None);
}

// spec EDICT-1.4: fields and fields_mut give the edict's whole slot array, and None for a
// number the array does not hold; a write through fields_mut is read back through fields.
#[test]
fn fields_and_fields_mut() {
    let mut store = store(1);
    assert_eq!(store.fields(0).map(<[Slot]>::len), Some(FIELDS_105));
    {
        let slots = store.fields_mut(0).unwrap();
        slots[4] = Slot::from_f32(9.0);
    }
    assert_eq!(store.fields(0).unwrap()[4], Slot::from_f32(9.0));
    assert_eq!(store.fields(LIMIT_MAX_EDICTS), None);
    assert_eq!(store.fields_mut(LIMIT_MAX_EDICTS), None);
}

// spec EDICT-6.2: alloc fails with NoFreeEdicts when num_edicts is already LIMIT_MAX_EDICTS
// and every scanned edict was freed too recently to reuse; pins the no-free-edicts path and
// its message.
#[test]
fn alloc_no_free_edicts_message() {
    let mut store = store(0);
    // Grow num_edicts to LIMIT_MAX_EDICTS, freeing nothing, so none can be reused.
    while store.num_edicts() < LIMIT_MAX_EDICTS {
        store.alloc(1.0).unwrap();
    }
    let err = store.alloc(1.0).unwrap_err();
    assert_eq!(err, EdictError::NoFreeEdicts);
}

// spec EDICT-5.1: maxclients reports the client-slot count the store was built with, not a
// constant; a store with 5 client slots reports 5.
#[test]
fn maxclients_reports_the_count() {
    let five = store(5);
    assert_eq!(five.maxclients(), 5);
    assert_eq!(five.num_edicts(), 6); // maxclients + 1
    let zero = store(0);
    assert_eq!(zero.maxclients(), 0);
}

// spec EDICT-1.7: edict_num accepts edict 0 (the world) and edict 1, and rejects a negative
// number; pins the lower bound `number < 0` (edict 0 must not be rejected).
#[test]
fn edict_num_accepts_world_and_first() {
    let s = store(1);
    assert_eq!(s.edict_num(0).unwrap(), EntityRef(0));
    let size = s.layout().edict_size();
    assert_eq!(s.edict_num(1).unwrap(), EntityRef(size));
    assert_eq!(
        s.edict_num(-1).unwrap_err(),
        EdictError::BadNumber { number: -1 }
    );
}
