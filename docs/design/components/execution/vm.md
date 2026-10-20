# VM and edicts

**Crates:** `qc-vm`, `qc-edict` (L1). **Specs:** VM-1 to VM-19, EDICT-1 to EDICT-3, EDICT-6, EDICT-7, LIMIT-2 to LIMIT-5.

## Context and design philosophy

The live preview, the agent's `vm.run`, the differential harness and the debugger all need to
run QuakeC the way the game does. This leaf is the interpreter (`qc-vm`) and the entity memory
it executes against (`qc-edict`), both L1 (layers.toml), ported from id's `pr_exec.c` and
`pr_edict.c` at id-Software/Quake@bf4ac42 (crates/qc-vm/src/lib.rs:5, crates/qc-edict/src/lib.rs:5-6).

Three rules shape it:

1. **Behaviour, not C.** No globals: id's `pr_stack`, `pr_depth`, `localstack`, `pr_xfunction`
   and friends become fields of `Machine` (crates/qc-vm/src/machine.rs:5-6). Every error is a
   `RunError` value; the crate has no panics (docs/conventions.md, no-panic lints).
2. **Where id's C is undefined, the spec fixes an answer.** id checks no operand (`PARANOID` is
   off, VM-1.4); this VM checks every access and raises an `ErrorKind::Divergence` tagged with
   the statement that fixes it. VM-19.4 lists them and docs/spec/differential.md DIFF-10.1
   marks them expected.
3. **Proven against id.** The VM vectors run through id's VM in CI (`cargo xtask oracles`), and
   fuzz/fuzz_targets/qc_vm.rs compares traces with the oracle (vm.md, Test vectors).

## Components

### Machine state

`Machine::new(&Progs, maxclients)` is id's `SV_SpawnServer` after `PR_LoadProgs`: it copies the
globals, builds an `EdictLayout` from the progs header and an `EdictStore` of 600 edicts with
`maxclients + 1` in use (VM-1.1, EDICT-1.4, EDICT-1.8). It fails with `MachineError::Layout` or
`MachineError::Edict`. The local stack is a fixed `LIMIT_LOCALSTACK_SIZE` (2048) slots. Public
accessors expose globals, edicts, argc, depth and local-stack use; `global`, `set_global`,
`global3`, `set_global3` are bounds-checked and the three-slot setter checks all three before
writing (machine.rs:211-222).

### Running

`execute_program(progs, server, builtins, observer, function)` is the outermost
`PR_ExecuteProgram`. It calls `run_inner`; on an error it reports the error to the observer once
and clears the call stack, leaving the local stack as is (crates/qc-vm/src/exec.rs:214-237,
VM-19.1, VM-19.3). A builtin starts a nested run through `Context::execute_program`, which calls
`run_inner` directly so the error record is written once by the outermost run (TRACE-4.9).

Each run has its own runaway counter (exec.rs:274, LIMIT-4.1, LIMIT-4.2). The loop fetches a
statement, checks its index (VM-3.3) and opcode (VM-3.4), dispatches to `ops::execute_statement`
and acts on the returned `Control` (`Next { builtin }`, `Jump`, `Return`). `Return` leaves the
function before the observer's step so the restored locals are part of the RETURN step
(exec.rs:347-358); id does the leave inside OP_DONE.

`ServerState { Loading, Active }` is passed in because `ADDRESS` on the world is legal only
while loading (VM-12.2).

### Opcodes

`ops.rs` is one function per opcode family of id's switch (VM-4 to VM-17), matched exhaustively
on `Opcode` (ops.rs:54-143). Float-to-int follows x86 truncation with the integer-indefinite
value `VM_FLOAT_TO_INT_INVALID` (ops.rs:612, VM-9.2). String comparisons resolve values outside
the string table through `Builtins::engine_string` (ops.rs:368-383, VM-7.3, PROGS-11.6). STATE
sets `nextthink` to `time + 0.1` added in double precision, writes `frame` only when it differs
by float comparison, and writes `think` (ops.rs:1043-1100, VM-17.1 to VM-17.4). Float comparisons that must be exact carry
`#[expect(clippy::float_cmp, reason = "spec ...")]` (ops.rs:309, 317, 346, 1071).

### Seams

- `Builtins { call, engine_string }` - the VM checks the builtin number (1 to 78, VM-14.4,
  LIMIT-5) and sets argc before `call`. `engine_string` is required, with no default
  (exec.rs:59-64, #397), so every implementation states how it resolves engine strings.
  `NoBuiltins` fails every call with a VM-14.4 divergence and resolves no string.
- `Observer { run, step, exit, error }` - `NoObserver` does nothing; `trace::Trace` records a
  `qc_trace::Trace` with a full-snapshot change set per record (trace.rs:85-134). It is "a format
  of this project, not a port" (trace.rs:9-10).
- `Context` - what a builtin gets: the machine, the progs, the server state, nested runs and
  `run_error` to build a `RunError` at the current statement.

### Edicts (qc-edict)

`EdictLayout::for_progs` derives `entityfields` and `edict_size = 96 + 4 * entityfields` from the
header, refusing too few globals (fewer than 92, EDICT-2.4), too few fields (fewer than 105,
EDICT-3.3) and too many (EDICT-3.5), in that order (crates/qc-edict/src/layout.rs:96-110). It
converts edict numbers to entity values and pointers (EDICT-1.5, EDICT-1.6, EDICT-1.9).

`EdictStore` holds all 600 edicts eagerly (store.rs:131). `edict_num` and `num_for_edict` are
id's `EDICT_NUM` / `NUM_FOR_EDICT` (EDICT-1.7). `alloc(now)` is `ED_Alloc`: scan from
`maxclients + 1`, reuse a free edict whose `freetime` is below 2 or more than 0.5 s old (double
precision against the f32 freetime), else grow, else `NoFreeEdicts` (EDICT-6). `free(n, now)` is
`ED_Free`: clear the listed fields, set `nextthink` to -1, record `freetime` as f32 (EDICT-7).
`system` holds the 54 system globals and 77 system fields from `progdefs.q1` with lookup by
slot and name (EDICT-2.1 to 2.3, EDICT-3.1, 3.2).

EDICT-4, 5, 8 to 13 (world entity, clients, map spawning, entry points, think timing, who writes
each system variable) are running-server behaviour owned by the preview host's `qs-sim` (P3-24,
#238) and stay in the coverage baseline (crates/qc-edict/README.md:31-35).

## Error model

`RunError { kind, function, statement, trace }`; `message()` is id's text or, for a
divergence, this VM's text with the statement ID appended (crates/qc-vm/src/error.rs:148).
`ErrorKind` covers id's errors (`NullRunFunction`, `NullCallFunction`, `StackOverflow`,
`LocalsStackOverflow`, `Runaway`, `BadBuiltin`, `BadOpcode`, `AssignmentToWorld`), spec-fixed
`Divergence { statement, detail }` and `Builtin { message }` for an error a builtin raised.
`statement` is -1 for an error before the first statement (TRACE-4.7).

## Verification

- Unit tests in `crates/qc-vm/src/tests.rs` and `src/tests/` (adversarial inputs, string
  opcodes) and in `crates/qc-edict/src/tests.rs`.
- The VM vectors in `docs/spec/vectors/vm/` run through this VM (`src/tests/vectors.rs`) and
  through id's VM in CI (`cargo xtask oracles`); `crates/qs-tests/tests/it/spec_vectors.rs`
  checks that they are well formed and cover every vanilla opcode.
- Fuzz target `qc_vm` compares traces with the oracle when `QS_VM_ORACLE` is set, and the
  differential runner compares whole corpus scenarios (oracles-differential).
- L1 readiness passed (`docs/readiness/p2-r1.md`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Port id's interpreter | Direct port of `PR_ExecuteProgram` and helpers | Write from a description | ADR-0002; docs/porting.md |
| Fix undefined behaviour as errors | `ErrorKind::Divergence` tagged with the spec statement | Mirror id's out-of-bounds reads | VM-19.4, DIFF-10.1; no-panic rule |
| Builtins behind a trait | `Builtins` trait object passed per run | Built-in table inside qc-vm | [inferred] keeps qc-vm free of engine state; qc-builtins (mock) and the preview host (P3-24) supply different implementations |
| `engine_string` with no default | Required trait method | Default returning `None` | #397: every implementer must decide how engine strings resolve (exec.rs:59-64) |
| Tracing through an observer | `Observer` trait; `Trace` in its own module | Trace inside the loop | [inferred] the VM pays nothing with `NoObserver`; one format shared with the oracle (TRACE-1) |
| RETURN leaves before the step record | Leave, then observe | id's order (leave inside DONE) | TRACE-3.1: the restore must appear in RETURN's change set (exec.rs:347-358) |
| Error record once, by the outermost run | `execute` vs `run_inner` | Every run emits its own | TRACE-4.9 |
| Eager 600-edict store | All edicts allocated at `Machine::new` | Grow on demand | [inferred] mirrors id's fixed array (EDICT-1.4) |
| 32-bit header size | `EDICT_HEADER_SIZE` = 96 | Host-dependent size (108 on 64-bit) | EDICT-1.3: the 32-bit x86 build is the reference |
| Running-server edict behaviour elsewhere | qs-sim (P3-24) | In qc-edict | crates/qc-edict/README.md:31-35 |

## Open questions

### Resolved
1. Engine strings in string opcodes (P1-63): resolved through `Builtins::engine_string`.

### Deferred
1. **Host error depth.** `execute` clears the stack on every error (exec.rs:214-237); a host
   error (BUILTIN-3.2) should leave the depth for the shutdown `ClientDisconnect` runs. Today
   qc-builtins collapses host into `ErrorKind::Builtin` (crates/qc-builtins/src/dispatch.rs:648-657).
2. **Trace flag.** `trace_flag` is `pub(crate)` with only a test getter
   (crates/qc-vm/src/machine.rs:62-65, 144-148); no builtin can set it.
3. **Divergence tags.** Every undefined case should raise an `ErrorKind::Divergence` tagged with
   the statement that fixes it (VM-19.4). `address_divergence` tags every ADDRESS error
   VM-12.5 (ops.rs:897-907), though VM-12.5 fixes STOREP and VM-12.3 says id checks nothing on
   ADDRESS; `string_divergence` tags `NOT_S` errors VM-7.3 (ops.rs:476-485,
   tests/string_opcodes.rs:201-215), where VM-8.3 defers to VM-7.3's resolution.
4. **`leave_function` ignores impossible failures** with empty if-bodies and a 0-locals fallback
   (exec.rs:473-476, 489-491).
5. **Unannotated zero comparisons** `a == 0.0` (ops.rs:499, 512, 560) carry no `#[expect]`; clippy
   does not flag comparisons with zero, so this is a style question.
6. **`ErrorKind::is_executed_statement`** special-cases Runaway and a Divergence whose statement
   is "VM-3.3" by string compare (error.rs:128-134).
7. **Duplicated slot constants** in ops.rs:1104-1108 and store.rs:29-32 instead of
   `qc_edict::system`.
8. **Duplicated machinery across crates**: four float-to-int
   implementations (qc-vm ops.rs:588, qc-builtins math.rs:36, two in qs-differential), three
   hand-written v6 encoders (qc-vm tests/harness.rs and two in qs-differential), two change-set
   observers (qc-vm trace.rs, qs-differential host/trace.rs).
9. **Trace snapshots.** Every record diffs all globals and fields; a newly allocated edict's
   fields are diffed against zero (trace.rs:105-108).
10. **Test quality.** adversarial.rs:192-215 does not reuse the aborted machine; tests.rs:111-117
    asserts only `is_err()`; harness.rs:59 panics on an unknown opcode name; `StubBuiltins` is
    duplicated between tests.rs and tests/vectors.rs; qc-edict tests.rs has near-duplicate
    too-many-fields tests (527-537, 616-625) and an unfinished comment (500-501).
11. **Doc defects.** machine.rs:131-136 misplaced rustdoc; exec.rs:67-68 garbled; ops.rs:444-445
    odd rustdoc; store.rs:76 vs 90 vs 127 maxclients range; qc-edict lib.rs:5-6 lists
    `ED_FindField` and `ED_FindGlobal`, which the crate does not implement.
12. **Stale prose.** crates/qc-edict/README.md:41-45 (differential "until qc-vm drives this
    store"); vm.md:17-19 and edicts.md:20-22 "every statement is in the baseline".

## References

- docs/spec/vm.md, docs/spec/edicts.md, docs/spec/limits.md, docs/spec/differential.md (DIFF-10.1)
- docs/porting.md; docs/adr/0002-license-model.md; docs/adr/0009-test-readiness-bar.md;
  docs/adr/0013-differential-coverage-bar.md
- docs/readiness/p1-r1.md, docs/readiness/p2-r1.md; ratchets.toml `[crates.qc-vm]`,
  `[crates.qc-edict]`
- crates/qc-vm/README.md, crates/qc-edict/README.md
- Plan: P1-09 (qc-edict), P1-10 (qc-vm), P1-15 (fuzz against the oracle), P1-63 (engine
  strings), P1-65 (coverage) in plan/backlog/p1.yaml; P3-24 (#238) qs-sim; P4-02 (#113) `vm.run`
