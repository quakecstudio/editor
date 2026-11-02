# qc-vm

The QuakeC virtual machine: id's `PR_ExecuteProgram` as a no-panic interpreter over a loaded
`progs.dat`.

- **Layer:** L1 (VM). Depends only on L0 and L1 crates: `qc-progs`, `qc-edict`, `qc-types`
  and `qc-trace`.
- **Purpose:** run compiled QuakeC the way id's engine does, so the IDE's preview, the agent
  and the differential tests all agree with the game.

## What it does

`Machine` holds the state id's `pr_exec.c` keeps between runs (`vm.md` VM-1.1): the globals,
the edicts (`qc-edict`), the call stack, the local stack, the current function and statement,
and the argument count of the last call. `Machine::execute_program` is id's
`PR_ExecuteProgram`:

- the statement loop with the runaway loop guard (VM-3, LIMIT-4);
- every vanilla opcode: float and vector arithmetic, comparisons, logical and bitwise
  operators, global stores, entity field loads, field addresses and pointer stores, jumps,
  `STATE` (VM-4 to VM-13, VM-17);
- calls, entering and leaving QC functions with the call and local stacks, parameter and
  return slots, and the stack-depth and local-stack limits (VM-14 to VM-16, LIMIT-2, LIMIT-3);
- builtin dispatch through the `Builtins` trait (VM-14.7, LIMIT-5);
- nested runs a builtin starts (VM-18);
- structured errors with a QuakeC stack trace, returned as `RunError` values, never panics
  (VM-19); each divergence where id's C is undefined is a `Divergence` variant (VM-19.7).

The `convert` module holds C's conversions between `int`, `float` and `double` as id's x86
build performs them (VM-9.1, VM-9.2), which `qc-builtins` shares (BUILTIN-2.1, BUILTIN-2.2).

The VM runs untrusted `progs.dat`, so every operand, global slot, field slot, jump target and
call index is bounds- and type-checked exactly as the spec fixes id's undefined C (VM-1.4,
VM-2.2, VM-2.4, VM-3.3, VM-7.3, VM-11.4, VM-12.5, VM-14.3, VM-15.6): where id reads or writes
out of bounds, this VM returns a runtime error before changing any state.

The `trace` module's `Trace` is a per-statement trace hook (an `Observer`) that builds a
`qc_trace::Trace` in the shared VM trace format (`trace.md`, TRACE-4), so the differential
runner can compare this VM against the id VM oracle statement by statement.

## Spec sections implemented

- **Spec sections:** [VM-1, VM-2, VM-3, VM-4, VM-5, VM-6, VM-7, VM-8, VM-9, VM-10, VM-11,
  VM-12, VM-13, VM-14, VM-15, VM-16, VM-17, VM-18, VM-19](../../docs/spec/vm.md) (the whole
  statement loop, opcodes, calls, returns, STATE, nested runs and errors) and
  [LIMIT-2, LIMIT-3, LIMIT-4, LIMIT-5](../../docs/spec/limits.md) (call depth, local stack,
  runaway guard, builtin numbers).

Ported from `id-Software/Quake` at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`: `WinQuake/pr_exec.c`
(`PR_ExecuteProgram`, `PR_EnterFunction`, `PR_LeaveFunction`, `PR_RunError`,
`PR_StackTrace`), with the structures of `WinQuake/pr_comp.h` and `WinQuake/progs.h`.

## Tests

`cargo test -p qc-vm` runs the hand-written VM vectors in `docs/spec/vectors/vm/` (arith,
compare, bitwise, store and the non-nested flow vectors) and checks every statement's change
set against the recorded answer, plus dedicated tests for nested runs (VM-18). The vectors
whose presentation order differs from the trace record order (nested runs) and those needing
id's builtins or the simulation (`builtins.txt`, `math.txt`, `edicts.txt`, `frames.txt`) are
reconciled by the differential runner (P1-13) and `qc-builtins` (P1-16, P1-31, P1-32).
