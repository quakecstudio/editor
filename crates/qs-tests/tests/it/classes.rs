// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC classes compiled by `qc-codegen` and run in `qc-vm` (FTEF-9.21 to FTEF-9.23): a
//! spawned class's entity holds its members, its class's name and its virtual functions, set by
//! the spawn function the compiler writes.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, panic or index is a failed test"
)]

use std::path::PathBuf;

use qc_builtins::MockBuiltins;
use qc_codegen::compile_module;
use qc_edict::EdictNumber;
use qc_progs::{Progs, ProgsFile, write};
use qc_sema::{Db, FileId, ModuleInput};
use qc_vm::{Machine, NoObserver, ServerState};

use crate::shape_stairs::system_defs;

/// A parent and a child class, each with a data member, a constructor and a virtual function
/// the child gives again; `t` spawns the child.
const CLASSES: &str = "\
#pragma target fte_5768
entity() spawn = #14;
class Base { float m_a; vector m_v; float m_c; float m_z; float m_s; void Base(void); virtual void(void) Draw; nonvirtual void(float f) Set; };
void Base::Set(float f) { m_s = f; };
void Base::Base(void) { m_a = 1; m_v = '4 5 6'; m_c = m_v[1]; m_z = m_v_z; };
void Base::Draw(void) { m_a = 5; };
class Child : Base { float m_b; void Child(void); virtual void(void) Draw; };
void Child::Child(void) { m_b = m_a + 1; };
void Child::Draw(void) { m_b = 9; };
entity made;
entity before;
float order;
void t() { before = self; made = spawn(Child); Child c = (Child)made; order = c.m_b; c.Draw(); c.Set(7); };
void() main = {};
void() StartFrame = {};
void() PlayerPreThink = {};
void() PlayerPostThink = {};
void() ClientKill = {};
void() ClientConnect = {};
void() PutClientInServer = {};
void() ClientDisconnect = {};
void() SetNewParms = {};
void() SetChangeParms = {};
";

/// The text at `ofs` of the string table.
fn string(progs: &ProgsFile, ofs: i32) -> String {
    let rest = &progs.strings[usize::try_from(ofs).unwrap()..];
    let end = rest.iter().position(|&b| b == 0).unwrap();
    String::from_utf8_lossy(&rest[..end]).into_owned()
}

/// The word of the global named `name`.
fn word(progs: &ProgsFile, name: &str) -> u32 {
    let def = progs
        .global_defs
        .iter()
        .find(|d| string(progs, d.s_name) == name)
        .unwrap_or_else(|| panic!("no global {name}"));
    progs.globals[usize::try_from(def.ofs).unwrap()].bits()
}

/// The number of the function named `name`.
fn function(progs: &ProgsFile, name: &str) -> u32 {
    let at = progs
        .functions
        .iter()
        .position(|f| string(progs, f.s_name) == name)
        .unwrap_or_else(|| panic!("no function {name}"));
    u32::try_from(at).unwrap()
}

// spec FTEF-9.22: the spawn function names the class, fills each virtual member's field with
// the most derived class's function and calls the child's constructor before its parent's
// spec FTEF-9.23: `spawn(CLASS)` calls `spawn`, then the class's spawn function with the new
// entity as `self`, and is the new entity
// spec FTEF-9.21: each data member and virtual member is an entity field of its own, a vector's
// components at its words
#[test]
fn a_spawned_class_runs_its_spawn_function() {
    let mut db = Db::new();
    db.set_dialect(qc_syntax::Dialect::Fteqcc);
    let files = [("defs.qc", system_defs()), ("t.qc", CLASSES.to_owned())];
    let mut input = ModuleInput::default();
    for (i, (name, text)) in files.iter().enumerate() {
        db.set_file(
            FileId::new(i),
            PathBuf::from(name),
            text.as_bytes().to_vec(),
        );
        input.compiled.push(FileId::new(i));
    }
    let compiled = compile_module(&mut db, &input, |id| {
        files
            .get(id.index())
            .map_or_else(String::new, |(n, _)| (*n).to_owned())
    })
    .unwrap();
    let file = &compiled.progs;
    let bytes = write(file).unwrap();
    let progs = Progs::read(&bytes).unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    let mut builtins = MockBuiltins::default();
    let mut observer = NoObserver;
    let t = i32::try_from(function(file, "t")).unwrap();
    machine
        .execute_program(&progs, ServerState::Active, &mut builtins, &mut observer, t)
        .unwrap();

    let made = machine
        .global(i32::try_from(word_ofs(file, "made")).unwrap())
        .unwrap();
    let spawned = machine.edicts().edict_num(1).unwrap();
    assert_eq!(
        made.bits(),
        u32::from_ne_bytes(spawned.bits().to_ne_bytes())
    );
    let field = |name: &str| {
        let ofs = usize::try_from(word(file, name)).unwrap();
        machine.edicts().field(EdictNumber::new(1), ofs).unwrap()
    };
    assert_eq!(field("Base::__mm_a").as_f32().to_bits(), 1.0_f32.to_bits());
    // The child's constructor ran first, when `m_a` was still 0.
    let order = machine
        .global(i32::try_from(word_ofs(file, "order")).unwrap())
        .unwrap();
    assert_eq!(order.as_f32().to_bits(), 1.0_f32.to_bits());
    assert_eq!(string(file, field("classname").as_i32()), "Child");
    assert_eq!(field("Base::__mDraw").bits(), function(file, "Child::Draw"));
    assert_ne!(word(file, "Base::__mm_a"), word(file, "Child::__mm_b"));
    // A vector member's words, and its components read by index and by name.
    let v = usize::try_from(word(file, "Base::__mm_v")).unwrap();
    for (i, expected) in [4.0_f32, 5.0, 6.0].into_iter().enumerate() {
        let slot = machine.edicts().field(EdictNumber::new(1), v + i).unwrap();
        assert_eq!(slot.as_f32().to_bits(), expected.to_bits());
    }
    assert_eq!(field("Base::__mm_c").as_f32().to_bits(), 5.0_f32.to_bits());
    // spec FTEF-9.24: a method called through an object runs with the object as `self`, the
    // virtual one the object's own function, and `self` is restored after it.
    assert_eq!(field("Child::__mm_b").as_f32().to_bits(), 9.0_f32.to_bits());
    assert_eq!(field("Base::__mm_s").as_f32().to_bits(), 7.0_f32.to_bits());
    let this = machine
        .global(i32::try_from(word_ofs(file, "self")).unwrap())
        .unwrap();
    let before = machine
        .global(i32::try_from(word_ofs(file, "before")).unwrap())
        .unwrap();
    assert_eq!(this.bits(), before.bits());
    assert_eq!(field("Base::__mm_z").as_f32().to_bits(), 6.0_f32.to_bits());
}

/// The offset of the global named `name`.
fn word_ofs(progs: &ProgsFile, name: &str) -> u32 {
    progs
        .global_defs
        .iter()
        .find(|d| string(progs, d.s_name) == name)
        .unwrap_or_else(|| panic!("no global {name}"))
        .ofs
}
