# Builtins

**Crates:** `qc-builtins` (L1). **Specs:** BUILTIN-1 to BUILTIN-17; DIFF-6, DIFF-7.

## Context and design philosophy

`qc-vm` executes QuakeC but knows nothing of the engine; every `CALL` to a negative
`first_statement` goes through the `qc_vm::Builtins` trait. `qc-builtins` (L1, layers.toml)
supplies the vanilla builtin table and a deterministic mock implementation of all of them, so a
progs can be run without a server: by the differential harness (qs-differential), by the
agent's planned `vm.run` (P4-02, #113) and by tests.

The bodies are ported from id's `pr_cmds.c`, `sv_main.c`, `mathlib.c` and `common.c` at
id-Software/Quake@bf4ac42 (crates/qc-builtins/src/dispatch.rs:5-7, math.rs:5-7). The mock engine
around them is "a design of this project, not a port" (crates/qc-builtins/src/mock.rs:9),
specified by docs/spec/differential.md DIFF-6 and DIFF-7 so that the oracle's C runner and this
crate answer the same way. Builtins that need a real world (BUILTIN-4.4: hulls, the area tree,
the PVS) get DIFF-7's fixed answers here; the real ones belong to the preview host's `qs-sim`
(P3-24, #238).

## Components

### The table

`table::BUILTINS` lists the 65 vanilla builtins with number, `defs.qc` name, 1.01 prototype, C
function and mock `Class { Pure, State, Output, World }` (crates/qc-builtins/src/table.rs,
BUILTIN-1, BUILTIN-4, BUILTIN-5). Numbers 5, 33, 39, 42, 50, 60 to 66 and 71 are `PF_Fixme`
gaps (table.rs:11-12, BUILTIN-1.2). `lookup` is a linear scan (table.rs:357-359).
`BUILTIN_MAX_NUMBER` = 78. `qc-engines` (L3) reads the table as its vanilla target
(crates/qc-engines/src/lib.rs:35, 197).

### Dispatch

`MockBuiltins<E: Engine = MockEngine>` wraps an engine; `Default` builds
`MockEngine::new("test", 0, 1)` (dispatch.rs:46-52). Its `Builtins::call` (impl only for
`MockEngine`, dispatch.rs:71):

1. Fails a gap or unknown number with id's `unimplemented bulitin` (dispatch.rs:92, BUILTIN-1.2).
2. Matches the number to a `b_*` body. Each reads its arguments from the parameter slots
   whatever argc is (BUILTIN-1.3), checks them as BUILTIN-2 and DIFF-6.6 fix (`invalid entity`,
   `invalid string`, `invalid field`, `invalid lightstyle`), and writes `OFS_RETURN`, system
   globals or edict fields (BUILTIN-1.4, PROGS-18).
3. Maps a `BuiltinError` to a `RunError` with `ErrorKind::Builtin { message }` through
   `Context::run_error` (dispatch.rs:648-657, BUILTIN-3, DIFF-8.2).

`engine_string` resolves negative values the mock created (EDICT-13.1, EDICT-13.3).

System globals and fields are addressed by name through `qc_edict::system` on each call
(dispatch.rs:480-524). Entities are freed and allocated through `EdictStore`.

### Math and strings

`math` holds the pure bodies: `makevectors` (through `AngleVectors`), `vlen`, `normalize`,
`vectoyaw`, `vectoangles`, `random` (`rand() & 0x7fff`), `rint`, `floor`, `ceil`, `fabs`,
`anglemod`, `ftos`, `vtos`, and `q_atof` (BUILTIN-6, BUILTIN-7, `Q_atof` for `cvar`).
`float_to_int` gives `BUILTIN_FLOAT_TO_INT_OVERFLOW` = i32::MIN for a value with no i32
(BUILTIN-2.2), computing through f64 (math.rs:36-38). `ftos` and `vtos` rely on Rust's `{:.1}`
matching C's `%5.1f` rounding (math.rs:219-223). The math.txt vectors run through id's
functions in the oracle (builtins.md, Test vectors).

### The mock engine

`MockEngine` (mock.rs) has public fields for every piece of state the DIFF-6 fixture defines:
cvars, model and sound precache lists, model bounds, lightstyles, the command buffer, the sizes
of the datagram, reliable and signon buffers, the LCG random state (DIFF-6.2), engine strings
created as -1, -2, ... in order (mock.rs:279-288), checkclient state, client activity and the
clock. `EngineStrings::take_temp_change` lets the differential host emit TRACE string records
(mock.rs:212-214, DIFF-3.9, TRACE-4.10; P2-61, #455, closed).

## Error model

`BuiltinError { Runtime, Host, Fatal, EngineOnly { number, name } }` (engine.rs). The kinds
follow BUILTIN-3.1 to BUILTIN-3.3; `EngineOnly` names a builtin that needs a real engine
(BUILTIN-4.4). A console message (BUILTIN-3.4) is not an error: the body returns `Ok`.

## Verification

- `crates/qc-builtins/src/tests.rs` runs each builtin through dispatch and checks its result,
  its state changes and its errors against BUILTIN and DIFF statements.
- `crates/qs-tests/tests/it/builtins.rs` runs builtins through `qc_vm::Machine`.
- The math vectors (`docs/spec/vectors/vm/math.txt`) and the builtin argument checks run
  through id's functions in the oracle (`cargo xtask oracles`), and the differential runner
  compares the mock with the oracle's mock on the corpus.
- L1 readiness passed (`docs/readiness/p2-r1.md`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Mock engine shipped in the library | `MockEngine` in `src/mock.rs` | Test-only fixture | DIFF-6: the differential host, fuzzing and the planned `vm.run` (P4-02) all need it |
| World builtins answered by fixed rules | DIFF-7 answers over the mock | Mark engine-only and fail | differential.md DIFF-7; real answers need qs-sim (BUILTIN-4.4, P3-24) |
| Errors carry id's verbatim text | `ErrorKind::Builtin { message }` | Typed variants per error | DIFF-6.6, DIFF-8.2: traces compare message bytes |
| Host and fatal collapse to a runtime error at the VM | `to_run_error` | Distinct ErrorKinds | dispatch.rs:650-652 states it; BUILTIN-3.3 says a fatal error "ends the run with a typed error ... as a runtime error does". Host (BUILTIN-3.2) is the open part |
| Generic `MockBuiltins<E: Engine>` | Type parameter defaulting to `MockEngine` | Concrete type | [inferred] intended seam for another engine; unused today |
| Builtins depend on qc-vm, not the reverse | qc-builtins depends on qc-vm (both L1) | Table inside qc-vm | [inferred] qc-vm stays engine-free; docs/layers.md allows same-layer edges |
| Name lookups at call time | `qc_edict::system::field_by_name` per access | Cached slot constants | [inferred] one source for slot numbers (EDICT-2, EDICT-3) |

## Open questions

### Resolved
1. Engine strings set by builtins appear in the differential trace (P2-61, #455).
2. Argument checks shared with the oracle (P1-31); state, output and world bodies (P1-59).

### Deferred
1. **Host errors.** BUILTIN-3.2 says a host error does not reset the call depth;
   `to_run_error` collapses it (dispatch.rs:648-657) and qc-vm resets the depth for every error.
2. **`Engine` seam.** Only `has_world` and `next_rand` (engine.rs:69-81); `has_world` is called
   only from tests; `NoEngine` and `EngineOnly` are unreachable through the VM
   (dispatch.rs:167-176). Whether qs-sim (P3-24) implements `Engine` or `Builtins` directly is
   undecided [inferred].
3. **`traceon` / `traceoff`** are no-ops (dispatch.rs:250-257, comment cites BUILTIN-16.4 and
   DIFF-6.7) because qc-vm has no trace-flag setter.
4. **BUILTIN-17** (rerelease prototypes) is claimed by README.md:39-42 and lib.rs:23 but no code
   or test cites it; all three statements are in the coverage baseline.
5. **`coredump` / `eprint`** return `Ok` without reading arguments (dispatch.rs:162).
6. **`sound` message size** uses `BUILTIN_SOUND_MAX_BYTES` minus flag savings, commented as an
   approximation of id's variable size (dispatch.rs:1144-1147).
7. **`objerror`** skips the free when `self` is invalid (dispatch.rs:1595).
8. **`resolve_string`** for a non-negative value delegates the bound to `progs.string`
   (dispatch.rs:364-371), unlike qc-vm's, which checks `offset < numstrings` first.
9. **`setspawnparms`** is a range check only; parms are left unchanged (dispatch.rs:772-781,
   EDICT-11.9).
10. **Duplicates.** LIMIT constants (mock.rs:120-203), `float_to_int` (math.rs:36 vs qc-vm
    ops.rs:588), `link_edict` (dispatch.rs:813 vs qs-differential host/frames.rs:167).
11. **Lossy lightstyles.** `set_lightstyle` stores a lossy UTF-8 `String` (mock.rs:404).
12. **Style.** `_public` wrappers exist only for dispatch (math.rs:232-234, 283-285); unused header
    constants kept by `let _header = ...` (dispatch.rs:979-983, 1006-1010, 1032); fixed-array
    indexing in several places (dispatch.rs:284, 876, 901-903, 1401, 1505, 1533).
13. **Tests.** `CALL_TARGET_SLOT` 60 lies in `v_forward` (tests.rs:694); tautology at
    tests.rs:1530-1548; misnamed test at tests.rs:2805-2825.
14. **Wording.** README.md:3-4 and table.rs:59-63 say 78 builtins; the table has 65.

## References

- docs/spec/builtins.md, docs/spec/differential.md (DIFF-6 to DIFF-8, DIFF-10), docs/spec/limits.md
- docs/porting.md; docs/adr/0002-license-model.md; docs/adr/0013-differential-coverage-bar.md
- docs/readiness/p2-r1.md; ratchets.toml `[crates.qc-builtins]`
- crates/qc-builtins/README.md
- Plan: P1-16, P1-31, P1-59, P1-64, P1-65 (plan/backlog/p1.yaml); P2-61 (#455); P3-24 (#238);
  P4-02 (#113)
