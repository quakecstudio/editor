// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_exec.c, pr_stack, pr_depth, localstack, localstack_used, pr_xfunction, pr_xstatement, pr_argc
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, PR_LoadProgs

//! The machine state id's `pr_exec.c` keeps, and bounds-checked access to the globals.
//!
//! Implements VM-1.1 (the state that persists between runs) and VM-1.3 and VM-1.4 (how an
//! opcode reaches a global slot, and the error on a slot outside the globals).

use qc_edict::{EdictStore, LayoutError};
use qc_progs::Progs;
use qc_types::Slot;

/// One frame of the call stack, id's `prstack_t`: the statement to return to and the function
/// it belongs to (VM-1.1, VM-15.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    /// The statement current in the caller when it called, to return to its next statement.
    pub(crate) statement: i32,
    /// The caller's function index.
    pub(crate) function: i32,
}

/// Reading or writing a global slot that is outside the globals (VM-1.4).
///
/// id checks no operand (`PARANOID` is off), so such an access reads or writes outside the
/// globals, which is undefined in C; this VM returns this error before changing any state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalAccessError {
    /// The global slot the opcode tried to reach.
    pub slot: i32,
}

/// The whole machine: the globals, the edicts and the execution stacks (VM-1.1).
///
/// All of it persists from one [`Machine::execute_program`](crate::Machine::execute_program)
/// call to the next; only the runaway counter and the exit depth belong to one call (VM-2.3,
/// VM-18.1). The current function, statement and `pr_argc` are the ones id keeps in
/// `pr_xfunction`, `pr_xstatement` and `pr_argc`.
#[derive(Clone, Debug)]
pub struct Machine {
    /// Every global slot, slots 0 to `numglobals - 1` (PROGS-12). id's `pr_globals`.
    pub(crate) globals: Vec<Slot>,
    /// The server's edicts (EDICT-1.4).
    pub(crate) edicts: EdictStore,
    /// The call stack, id's `pr_stack`; its length is the call depth `pr_depth`.
    pub(crate) stack: Vec<Frame>,
    /// The local stack, id's `localstack`, [`LIMIT_LOCALSTACK_SIZE`](crate::LIMIT_LOCALSTACK_SIZE)
    /// slots shared by every active QC function (VM-15.2).
    pub(crate) localstack: Vec<Slot>,
    /// Slots of the local stack in use, id's `localstack_used`.
    pub(crate) localstack_used: usize,
    /// The current function index, id's `pr_xfunction`.
    pub(crate) xfunction: i32,
    /// The current statement index, id's `pr_xstatement`.
    pub(crate) xstatement: i32,
    /// The argument count of the last call, id's `pr_argc` (VM-14.1).
    pub(crate) argc: usize,
    /// The statement trace flag, id's `pr_trace`: set by the `traceon` builtin (BUILTIN-16.4)
    /// and cleared at the start of each run (VM-2.3). This VM traces every statement through
    /// its own hook, so the flag changes nothing observable (DIFF-6.7).
    pub(crate) trace_flag: bool,
}

impl Machine {
    /// A fresh machine for a loaded progs, id's `SV_SpawnServer` after `PR_LoadProgs`.
    ///
    /// The globals start with the progs' values (PROGS-12), the edict store is set up from
    /// the progs' field and global counts (EDICT-1.8), and the stacks are empty. `maxclients`
    /// is the client slot count (EDICT-5.1); the VM test environment uses 0.
    ///
    /// # Errors
    ///
    /// [`LayoutError`] when the progs cannot back an edict store (too few globals or fields,
    /// or too many fields; EDICT-2.4, EDICT-3.3, EDICT-3.5).
    pub fn new(progs: &Progs<'_>, maxclients: usize) -> Result<Self, MachineError> {
        let layout = qc_edict::EdictLayout::for_progs(progs)?;
        let edicts = EdictStore::new(layout, maxclients)?;
        let globals: Vec<Slot> = progs.globals().iter().collect();
        Ok(Self {
            globals,
            edicts,
            stack: Vec::new(),
            localstack: vec![Slot::ZERO; crate::LIMIT_LOCALSTACK_SIZE],
            localstack_used: 0,
            xfunction: 0,
            xstatement: 0,
            argc: 0,
            trace_flag: false,
        })
    }

    /// The globals, slots 0 to `numglobals - 1`.
    #[must_use]
    pub fn globals(&self) -> &[Slot] {
        &self.globals
    }

    /// The edict store.
    #[must_use]
    pub const fn edicts(&self) -> &EdictStore {
        &self.edicts
    }

    /// The edict store, for writing.
    pub const fn edicts_mut(&mut self) -> &mut EdictStore {
        &mut self.edicts
    }

    /// The argument count of the last call, id's `pr_argc`, which builtins read (VM-14.1).
    #[must_use]
    pub const fn argc(&self) -> usize {
        self.argc
    }

    /// The current QC call depth, id's `pr_depth`: the number of active QC frames (LIMIT-2.1).
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Slots of the local stack in use, id's `localstack_used`.
    #[must_use]
    pub const fn localstack_used(&self) -> usize {
        self.localstack_used
    }

    /// Sets a global slot for tests, ignoring an out-of-range slot.
    #[cfg(test)]
    pub(crate) fn set_global_test(&mut self, slot: i32, value: Slot) {
        if self.set_global(slot, value).is_err() {
            // A test only sets slots it knows exist; ignore an out-of-range slot.
        }
    }

    /// Whether the statement trace flag is set, id's `pr_trace` (VM-2.3, BUILTIN-16.4).
    #[must_use]
    pub const fn tracing(&self) -> bool {
        self.trace_flag
    }

    /// Sets or clears the statement trace flag, as `traceon` and `traceoff` do (BUILTIN-16.4).
    pub const fn set_tracing(&mut self, on: bool) {
        self.trace_flag = on;
    }

    /// Reads one global slot, bounds-checked (VM-1.3, VM-1.4).
    ///
    /// A builtin reads a parameter slot (`OFS_PARM0` up) and a system global through this
    /// (VM-14.1, PROGS-18); the index is `self.globals().len()` at most.
    ///
    /// # Errors
    ///
    /// [`GlobalAccessError`] when `slot` is negative or not less than `numglobals`.
    pub fn global(&self, slot: i32) -> Result<Slot, GlobalAccessError> {
        usize::try_from(slot)
            .ok()
            .and_then(|index| self.globals.get(index).copied())
            .ok_or(GlobalAccessError { slot })
    }

    /// Writes one global slot, bounds-checked (VM-1.3, VM-1.4).
    ///
    /// A builtin writes its result to `OFS_RETURN` and a system global (such as `v_forward` or
    /// the `trace_*` globals) through this; the write is checked so a hand-built progs cannot
    /// make it reach outside the globals.
    ///
    /// # Errors
    ///
    /// [`GlobalAccessError`] when `slot` is negative or not less than `numglobals`; nothing is
    /// written.
    pub fn set_global(&mut self, slot: i32, value: Slot) -> Result<(), GlobalAccessError> {
        match usize::try_from(slot)
            .ok()
            .and_then(|index| self.globals.get_mut(index))
        {
            Some(cell) => {
                *cell = value;
                Ok(())
            }
            None => Err(GlobalAccessError { slot }),
        }
    }

    /// Reads the three slots of a vector global, `slot`, `slot + 1`, `slot + 2` (VM-1.3).
    ///
    /// All three must be inside the globals, checked before any is read (VM-1.4).
    ///
    /// # Errors
    ///
    /// [`GlobalAccessError`] naming the first slot outside the globals.
    pub fn global3(&self, slot: i32) -> Result<[Slot; 3], GlobalAccessError> {
        let x = self.global(slot)?;
        let y = self.global(add_slot(slot, 1)?)?;
        let z = self.global(add_slot(slot, 2)?)?;
        Ok([x, y, z])
    }

    /// Writes the three slots of a vector global, `slot`, `slot + 1`, `slot + 2` (VM-1.3).
    ///
    /// All three must be inside the globals, checked before any is written (VM-1.4), so a
    /// failure leaves every slot unchanged. A builtin writes a vector result (`makevectors`'
    /// `v_forward`, `trace_endpos`) through this.
    ///
    /// # Errors
    ///
    /// [`GlobalAccessError`] naming the first slot outside the globals; nothing is written.
    pub fn set_global3(&mut self, slot: i32, value: [Slot; 3]) -> Result<(), GlobalAccessError> {
        // Check all three slots are in range before writing any (VM-1.4).
        let s1 = add_slot(slot, 1)?;
        let s2 = add_slot(slot, 2)?;
        self.global(slot)?;
        self.global(s1)?;
        self.global(s2)?;
        self.set_global(slot, value[0])?;
        self.set_global(s1, value[1])?;
        self.set_global(s2, value[2])?;
        Ok(())
    }
}

/// `slot + offset`, as an `i32`, failing when it overflows (so it is reported as out of range).
///
/// A statement operand is at most `i16::MAX` (LIMIT-1.1), so a small offset never overflows in
/// practice; the check keeps the no-panic guarantee for a hand-built file.
pub(crate) fn add_slot(slot: i32, offset: i32) -> Result<i32, GlobalAccessError> {
    slot.checked_add(offset).ok_or(GlobalAccessError { slot })
}

/// Why a machine could not be created from a progs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineError {
    /// The progs cannot back an edict store (EDICT-2.4, EDICT-3.3, EDICT-3.5).
    Layout(LayoutError),
    /// `maxclients` is out of range (EDICT-5.1).
    Edict(qc_edict::EdictError),
}

impl From<LayoutError> for MachineError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<qc_edict::EdictError> for MachineError {
    fn from(error: qc_edict::EdictError) -> Self {
        Self::Edict(error)
    }
}

impl core::fmt::Display for MachineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Layout(error) => write!(f, "{error}"),
            Self::Edict(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for MachineError {}
