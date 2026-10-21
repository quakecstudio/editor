// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;
use proptest::sample::select;

use super::{
    DefType, EntityRef, Etype, FieldRef, FunctionRef, LIMIT_MAX_PARMS, PROGS_OFS_NULL,
    PROGS_OFS_PARM0, PROGS_OFS_RETURN, PROGS_PARM_SLOTS, PROGS_RESERVED_OFS, PROGS_TYPE_COUNT,
    PROGS_VECTOR_SLOTS, PointerRef, Slot, StringRef, TooFewSlots, Value, ofs_parm,
};

fn any_etype() -> impl Strategy<Value = Etype> {
    select(Etype::ALL.to_vec())
}

fn any_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Void),
        any::<i32>().prop_map(|v| Value::String(StringRef(v))),
        any::<u32>().prop_map(|bits| Value::Float(f32::from_bits(bits))),
        any::<[u32; 3]>().prop_map(|b| Value::Vector(b.map(f32::from_bits))),
        any::<i32>().prop_map(|v| Value::Entity(EntityRef(v))),
        any::<i32>().prop_map(|v| Value::Field(FieldRef(v))),
        any::<i32>().prop_map(|v| Value::Function(FunctionRef(v))),
        any::<i32>().prop_map(|v| Value::Pointer(PointerRef(v))),
    ]
}

fn slots_of(value: Value) -> Vec<Slot> {
    let mut slots = vec![Slot(0); value.etype().slot_count()];
    value.write(&mut slots).unwrap();
    slots
}

// spec PROGS-16.1: the eight type numbers are ev_void 0 to ev_pointer 7, in etype_t order.
#[test]
fn type_numbers_follow_etype_t() {
    let expected = [
        (0, Etype::Void, "ev_void"),
        (1, Etype::String, "ev_string"),
        (2, Etype::Float, "ev_float"),
        (3, Etype::Vector, "ev_vector"),
        (4, Etype::Entity, "ev_entity"),
        (5, Etype::Field, "ev_field"),
        (6, Etype::Function, "ev_function"),
        (7, Etype::Pointer, "ev_pointer"),
    ];
    assert_eq!(Etype::ALL.len(), usize::from(PROGS_TYPE_COUNT));
    for (index, (number, etype, name)) in expected.into_iter().enumerate() {
        assert_eq!(Etype::ALL[index], etype);
        assert_eq!(etype.number(), number);
        assert_eq!(etype as u16, number);
        assert_eq!(Etype::from_number(number), Some(etype));
        assert_eq!(etype.name(), name);
    }
}

// spec PROGS-16.2: slot counts are id's type_size {1,1,1,3,1,1,1,1}, pointer fixed at 1.
#[test]
fn slot_counts_are_type_size() {
    let type_size: [usize; 8] = [1, 1, 1, 3, 1, 1, 1, 1];
    let counts: Vec<usize> = Etype::ALL.iter().map(|t| t.slot_count()).collect();
    assert_eq!(counts, type_size);
    assert_eq!(Etype::Vector.slot_count(), PROGS_VECTOR_SLOTS);
    assert_eq!(Etype::Pointer.slot_count(), 1);
}

// spec PROGS-16.3: a type number above 7 is not a vanilla type, and is kept, not rejected.
#[test]
fn unknown_type_numbers_are_kept() {
    for number in [8, 9, 255, 0x7fff, u16::MAX] {
        assert_eq!(Etype::from_number(number), None);
        let def = DefType::from_number(number);
        assert_eq!(def, DefType::Unknown(number));
        assert_eq!(def.number(), number);
        assert_eq!(def.etype(), None);
    }
    assert_eq!(DefType::from_number(7), DefType::Known(Etype::Pointer));
    assert_eq!(DefType::from_number(0).etype(), Some(Etype::Void));
}

// spec PROGS-16.4: QuakeC cannot declare a pointer; every other type is declarable.
#[test]
fn only_pointer_is_not_declarable() {
    for etype in Etype::ALL {
        assert_eq!(etype.is_declarable(), etype != Etype::Pointer, "{etype:?}");
    }
}

// spec PROGS-17.1: a slot is the same 4 bytes read as f32 or i32, never converted.
#[test]
fn slot_reinterprets_bits() {
    let one = Slot::from_f32(1.0);
    assert_eq!(one.as_i32(), 0x3f80_0000);
    assert_eq!(Slot::from_i32(0x3f80_0000).as_f32(), 1.0);
    assert_eq!(Slot::from_i32(-1).0, u32::MAX);
    assert_eq!(Slot::from_i32(i32::MIN).as_f32().to_bits(), 0x8000_0000);
    assert_eq!(Slot::from_f32(-0.0).as_i32(), i32::MIN);
    assert_eq!(Slot::from_le_bytes([0, 0, 0x80, 0x3f]).as_f32(), 1.0);
    assert_eq!(Slot::from_f32(1.0).to_le_bytes(), [0, 0, 0x80, 0x3f]);
}

// spec PROGS-17.2: a string is an i32 byte offset into the string table; 0 is the empty string.
#[test]
fn string_is_byte_offset() {
    assert_eq!(StringRef::EMPTY, StringRef(0));
    let value = Value::read(Etype::String, &[Slot::from_i32(42)]).unwrap();
    assert_eq!(value, Value::String(StringRef(42)));
    assert_eq!(slots_of(Value::String(StringRef::EMPTY)), [Slot(0)]);
}

// spec PROGS-17.3: a float is the f32 in the slot; a vector is three float slots, x first.
#[test]
fn float_and_vector_layout() {
    let slots = [
        Slot::from_f32(1.0),
        Slot::from_f32(2.0),
        Slot::from_f32(3.0),
    ];
    assert_eq!(
        Value::read(Etype::Float, &slots).unwrap(),
        Value::Float(1.0)
    );
    assert_eq!(
        Value::read(Etype::Vector, &slots).unwrap(),
        Value::Vector([1.0, 2.0, 3.0])
    );
    assert_eq!(slots_of(Value::Vector([1.0, 2.0, 3.0])), slots);
}

// spec PROGS-17.4: an entity is a byte offset, entity number times edict size; 0 is the world.
#[test]
fn entity_is_byte_offset() {
    assert_eq!(EntityRef::WORLD, EntityRef(0));
    assert_eq!(EntityRef::from_number(0, 400), Some(EntityRef::WORLD));
    assert_eq!(EntityRef::from_number(3, 400), Some(EntityRef(1200)));
    assert_eq!(EntityRef(1200).number(400), Some(3));
    assert_eq!(EntityRef(1201).number(400), None);
    assert_eq!(EntityRef(1200).number(0), None);
    assert_eq!(EntityRef::from_number(i32::MAX, 2), None);
    let value = Value::read(Etype::Entity, &[Slot::from_i32(1200)]).unwrap();
    assert_eq!(value, Value::Entity(EntityRef(1200)));
}

// spec PROGS-17.5: a field is a slot index, so .origin_y is one more than .origin.
#[test]
fn field_is_slot_index() {
    let origin = FieldRef(10);
    assert_eq!(origin.component(0), Some(FieldRef(10)));
    assert_eq!(origin.component(1), Some(FieldRef(11)));
    assert_eq!(origin.component(2), Some(FieldRef(12)));
    assert_eq!(origin.component(3), None);
    assert_eq!(FieldRef(i32::MAX).component(1), None);
    let value = Value::read(Etype::Field, &[Slot::from_i32(11)]).unwrap();
    assert_eq!(value, Value::Field(FieldRef(11)));
}

// spec PROGS-17.6: a function is an index into the functions table; 0 is the null function.
#[test]
fn function_is_index() {
    assert_eq!(FunctionRef::NULL, FunctionRef(0));
    let value = Value::read(Etype::Function, &[Slot::from_i32(5)]).unwrap();
    assert_eq!(value, Value::Function(FunctionRef(5)));
}

// spec PROGS-17.7: a pointer is an i32 byte offset into the edict array.
#[test]
fn pointer_is_byte_offset() {
    let value = Value::read(Etype::Pointer, &[Slot::from_i32(1296)]).unwrap();
    assert_eq!(value, Value::Pointer(PointerRef(1296)));
    assert_eq!(
        slots_of(Value::Pointer(PointerRef(-4))),
        [Slot(0xffff_fffc)]
    );
}

// spec PROGS-18.1: slot 0 is OFS_NULL, 1 to 3 the return value, parameter n at 4 + 3n.
#[test]
fn reserved_slots() {
    assert_eq!(PROGS_OFS_NULL, 0);
    assert_eq!(PROGS_OFS_RETURN, 1);
    assert_eq!(PROGS_OFS_RETURN + PROGS_PARM_SLOTS, PROGS_OFS_PARM0);
    let parms: Vec<u16> = (0..8).map(|n| ofs_parm(n).unwrap()).collect();
    assert_eq!(parms, [4, 7, 10, 13, 16, 19, 22, 25]);
}

// spec PROGS-18.2: system globals start at slot 28, just after the last parameter's slots.
#[test]
fn system_globals_start_at_reserved_ofs() {
    assert_eq!(PROGS_RESERVED_OFS, 28);
    assert_eq!(
        ofs_parm(LIMIT_MAX_PARMS - 1).unwrap() + PROGS_PARM_SLOTS,
        PROGS_RESERVED_OFS
    );
}

// spec LIMIT-1.4: at most 8 parameters, each with 3 slots in the call area.
#[test]
fn eight_parameters_at_most() {
    assert_eq!(LIMIT_MAX_PARMS, 8);
    assert!(ofs_parm(7).is_some());
    assert_eq!(ofs_parm(8), None);
    assert_eq!(ofs_parm(u16::MAX), None);
}

// spec PROGS-16.2: ev_void takes one slot of id's type_size table; it has no value to write.
#[test]
fn void_occupies_one_slot_and_writes_nothing() {
    let mut slots = [Slot(0xdead_beef)];
    Value::Void.write(&mut slots).unwrap();
    assert_eq!(slots, [Slot(0xdead_beef)]);
    assert_eq!(Value::read(Etype::Void, &slots).unwrap(), Value::Void);
}

// spec PROGS-16.2: a value of each type takes its type_size slots, so fewer is an error and
// nothing is written.
#[test]
fn too_few_slots_is_an_error() {
    for etype in Etype::ALL {
        let needed = etype.slot_count();
        let short = vec![Slot(0); needed - 1];
        let expected = TooFewSlots {
            etype,
            needed,
            found: needed - 1,
        };
        assert_eq!(Value::read(etype, &short), Err(expected));
    }
    let mut two = [Slot(7), Slot(8)];
    let err = Value::Vector([1.0, 2.0, 3.0]).write(&mut two).unwrap_err();
    assert_eq!(err.to_string(), "ev_vector needs 3 slots, found 2");
    assert_eq!(two, [Slot(7), Slot(8)]);
    assert!(Value::Float(1.0).write(&mut []).is_err());
}

// spec PROGS-16.2: a value takes only its type_size slots; the slots after them are not read
// or written.
#[test]
fn extra_slots_are_ignored_and_untouched() {
    let mut slots = [Slot(9); 4];
    Value::Float(2.0).write(&mut slots).unwrap();
    assert_eq!(slots[1..], [Slot(9); 3]);
    assert_eq!(
        Value::read(Etype::Float, &slots).unwrap(),
        Value::Float(2.0)
    );
}

proptest! {
    // spec PROGS-17.1: reading a slot as either view and writing it back keeps every bit.
    #[test]
    fn slot_views_round_trip(bits in any::<u32>()) {
        let slot = Slot(bits);
        prop_assert_eq!(Slot::from_f32(slot.as_f32()), slot);
        prop_assert_eq!(Slot::from_i32(slot.as_i32()), slot);
        prop_assert_eq!(Slot::from_le_bytes(slot.to_le_bytes()), slot);
    }

    // spec PROGS-17.1: writing a value then reading it back gives the same bits, NaNs included.
    #[test]
    fn value_round_trips_through_slots(value in any_value()) {
        let slots = slots_of(value);
        let back = Value::read(value.etype(), &slots).unwrap();
        prop_assert_eq!(back.etype(), value.etype());
        prop_assert_eq!(slots_of(back), slots);
    }

    // spec PROGS-17.1: reading any slots as any type and writing back reproduces them.
    #[test]
    fn slots_round_trip_through_values(etype in any_etype(), bits in any::<[u32; 3]>()) {
        let slots: Vec<Slot> = bits.into_iter().map(Slot).take(etype.slot_count()).collect();
        let value = Value::read(etype, &slots).unwrap();
        prop_assert_eq!(value.etype(), etype);
        let mut out = slots.clone();
        value.write(&mut out).unwrap();
        prop_assert_eq!(out, slots);
    }

    // spec PROGS-16.3: classifying a type number and asking for it back is the identity.
    #[test]
    fn def_type_keeps_its_number(number in any::<u16>()) {
        let def = DefType::from_number(number);
        prop_assert_eq!(def.number(), number);
        prop_assert_eq!(def.etype().is_some(), number < PROGS_TYPE_COUNT);
    }

    // spec PROGS-17.4: an entity number converts to an offset and back.
    #[test]
    fn entity_number_round_trips(number in 0..600_i32, edict_size in 1..4096_i32) {
        let entity = EntityRef::from_number(number, edict_size).unwrap();
        prop_assert_eq!(entity.number(edict_size), Some(number));
    }
}
