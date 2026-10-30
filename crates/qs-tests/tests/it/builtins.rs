// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-crate tests of `qc-builtins`: a minimal progs that calls a builtin, run through
//! [`qc_vm::Machine`] with [`qc_builtins::MockBuiltins`], proving the dispatcher reads
//! parameters and writes the result (BUILTIN-1.3, BUILTIN-1.4, PROGS-18).

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_builtins::MockBuiltins;
use qc_edict::system::global_by_name;
use qc_progs::opcode::Opcode;
use qc_progs::{Function, Progs, ProgsFile, Statement, write};
use qc_types::{PROGS_OFS_PARM0, PROGS_OFS_RETURN, Slot};
use qc_vm::{Machine, NoObserver, ServerState};

/// A global slot holding the function value the entry's `CALL0` calls.
const CALL_TARGET_SLOT: u32 = 60;

/// Builds a progs whose entry function (index 1) calls builtin `number` with `CALL0`, then is
/// done. Function 2 is the builtin (`first_statement = -number`). The globals have 92 slots, the
/// minimum the edict layout needs (EDICT-2.4); slot `CALL_TARGET_SLOT` holds function 2.
fn progs_calling_builtin(number: u32) -> Vec<u8> {
    let mut globals = vec![Slot::ZERO; 92];
    globals[usize::try_from(CALL_TARGET_SLOT).unwrap()] = Slot::from_i32(2);

    let statements = vec![
        Statement {
            op: u32::from(Opcode::Call0.number()),
            a: i32::try_from(CALL_TARGET_SLOT).unwrap(),
            b: 0,
            c: 0,
        },
        Statement {
            op: u32::from(Opcode::Done.number()),
            a: i32::from(PROGS_OFS_RETURN),
            b: 0,
            c: 0,
        },
    ];

    let builtin_first = i32::try_from(number).unwrap().checked_neg().unwrap();
    let functions = vec![
        Function::default(),
        Function {
            first_statement: 0,
            ..Function::default()
        },
        Function {
            first_statement: builtin_first,
            ..Function::default()
        },
    ];

    let file = ProgsFile {
        entityfields: 105,
        statements,
        functions,
        globals,
        strings: vec![0],
        ..ProgsFile::default()
    };
    write(&file).unwrap()
}

// spec BUILTIN-6.8: fabs clears the sign bit; run end to end through the VM.
#[test]
fn fabs_through_the_vm() {
    let bytes = progs_calling_builtin(43);
    let progs = Progs::read(&bytes).unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine
        .set_global(i32::from(PROGS_OFS_PARM0), Slot::from_f32(-3.5))
        .unwrap();

    let mut builtins = MockBuiltins::default();
    let mut observer = NoObserver;
    machine
        .execute_program(&progs, ServerState::Active, &mut builtins, &mut observer, 1)
        .unwrap();

    let result = machine
        .global(i32::from(PROGS_OFS_RETURN))
        .unwrap()
        .as_f32();
    assert_eq!(result.to_bits(), 3.5_f32.to_bits());
}

// spec BUILTIN-6.1: makevectors writes v_forward through the VM; zero angles give '1 0 -0'.
#[test]
fn makevectors_writes_system_globals() {
    let bytes = progs_calling_builtin(1);
    let progs = Progs::read(&bytes).unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    let mut builtins = MockBuiltins::default();
    let mut observer = NoObserver;
    machine
        .execute_program(&progs, ServerState::Active, &mut builtins, &mut observer, 1)
        .unwrap();

    let v_forward = global_by_name("v_forward").unwrap();
    let slot = i32::try_from(v_forward.slot).unwrap();
    assert_eq!(
        machine.global(slot).unwrap().as_f32().to_bits(),
        1.0_f32.to_bits()
    );
    assert_eq!(
        machine.global(slot + 1).unwrap().as_f32().to_bits(),
        0.0_f32.to_bits()
    );
    assert_eq!(
        machine.global(slot + 2).unwrap().as_f32().to_bits(),
        (-0.0_f32).to_bits()
    );
}

// spec BUILTIN-1.2: a PF_Fixme number fails with id's "unimplemented bulitin".
#[test]
fn fixme_number_fails() {
    let bytes = progs_calling_builtin(5);
    let progs = Progs::read(&bytes).unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    let mut builtins = MockBuiltins::default();
    let mut observer = NoObserver;
    let error = machine
        .execute_program(&progs, ServerState::Active, &mut builtins, &mut observer, 1)
        .unwrap_err();
    assert_eq!(error.message(), "unimplemented bulitin");
}
