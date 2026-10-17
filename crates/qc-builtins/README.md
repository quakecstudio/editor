# qc-builtins

The vanilla QuakeC builtins and a deterministic mock engine for them. id's engine exposes 78
builtins to QuakeC, numbers 1 to 78, dispatched by number from `pr_builtins`; this crate holds
their table, the pure math and string builtins that need no engine, a deterministic mock
engine implementing the fixture of `docs/spec/differential.md` for unit and differential tests,
and an `Engine` trait a real engine (`qs-sim`, P3-24) implements.

- `table`: the number, name, prototype and mock class of builtins 1 to 78, with the `PF_Fixme`
  gaps (5, 33, 39, 42, 50, 60 to 66, 71) absent (BUILTIN-1, BUILTIN-4, BUILTIN-5).
- `math`: the pure math and string builtins, exact without an engine: `makevectors`,
  `normalize`, `vlen`, `vectoyaw`, `vectoangles`, `random`, `rint`, `floor`, `ceil`, `fabs`,
  `ftos`, `vtos`, and `anglemod`, with id's float-to-int rule (BUILTIN-2.1, BUILTIN-2.2,
  BUILTIN-6, BUILTIN-7, BUILTIN-15.5).
- `mock`: the deterministic mock engine fixture (the precache lists and model bounds, the
  lightstyles, the cvars, the `changelevel` flag, the command buffer, the message buffer sizes
  and the DIFF-6.2 random source) that two independent runners share (DIFF-6, DIFF-7).
- `MockBuiltins`: implements `qc_vm::Builtins` for `MockBuiltins<MockEngine>`, so a
  `qc_vm::Machine` can run a progs that calls builtins; it reads each builtin's arguments from
  the parameter slots and writes its result to `OFS_RETURN`, the system globals or the edicts
  (BUILTIN-1.3, BUILTIN-1.4, PROGS-18). A number with no builtin fails with id's
  `unimplemented bulitin` (BUILTIN-1.2). Every vanilla builtin has a body: the pure math and
  strings (BUILTIN-6, BUILTIN-7), the entity, placing, precaching, printing, command, setting,
  sound, particle, lightstyle, static-entity, network-message and error builtins against the
  mock fixture (BUILTIN-8 to BUILTIN-13, BUILTIN-16), the class world builtins by DIFF-7
  (BUILTIN-14, BUILTIN-15), and the argument checks of BUILTIN-2 with the runtime errors of
  DIFF-6.6 (`invalid entity`, `invalid string`, `invalid field`, `invalid lightstyle`).
- `Engine` and `BuiltinError`: the engine-side effects a real engine provides, and the typed
  errors a builtin raises (BUILTIN-3). The class world builtins (BUILTIN-4.4) are answered by
  DIFF-7 over the mock; a caller using `NoEngine` gets a typed engine-only error.

## Layer

L1 VM. It depends on `qc-vm`, `qc-edict` (both L1) and `qc-progs`, `qc-types` (both L0), and
nothing higher.

## Spec sections

Implements `docs/spec/builtins.md` (BUILTIN-1 to BUILTIN-17) and the mock engine of
`docs/spec/differential.md` (DIFF-6, DIFF-7). The table, the mock classes, the pure math and
string builtins and the class state, output and world builtin bodies against the mock fixture
are complete.
