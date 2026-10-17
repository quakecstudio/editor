# qs-differential

QuakeC Studio's side of the differential VM runner (`docs/spec/differential.md`, P1-13).

A differential run executes one scenario in two VMs, the id VM oracle
(`oracles/vm/scenario.c`, built by `cargo xtask oracles`) and QuakeC Studio's `qc-vm`, and
compares the two traces statement by statement (DIFF-1). Neither runner links the other: this
spec and the ones it cites are the whole contract between them.

This crate is the QuakeC Studio runner. It reads a scenario file, drives `qc-vm` with
`qc-edict` and the `qc-builtins` mock engine through the host loop, and compares two traces by
DIFF-9. It is a test-layer crate and is never part of the shipped app; it is shipped-quality
all the same (no panics, every value checked), because the differential suite is a merge gate
(QA-1).

## Layer

Tests. It builds on the L1 VM crates (`qc-vm`, `qc-edict`, `qc-builtins`), the L0 formats
below them (`qc-progs`, `qc-trace`, `qc-types`) and the L1 porting example
(`qs-porting-example`, for the EXAMPLE-1 string escapes EDICT-8.2 reuses), and nothing depends
on it (`layers.toml`).

## Spec sections

- `docs/spec/differential.md` DIFF-2: the scenario file (`scenario` module).
- `docs/spec/edicts.md` EDICT-8: spawning a map's entities, the port of `ED_LoadFromFile`,
  `ED_ParseEdict`, `ED_ParseEpair` and `COM_Parse` (`edict8` module).
- `docs/spec/differential.md` DIFF-3 to DIFF-5 and DIFF-8: the host loop that sets up the map,
  lets the clients enter, runs the server frames and shuts the server down after an error
  (`host` module), driving `qc-vm` with `qc-edict` and the `qc-builtins` mock engine and
  writing the trace of `docs/spec/trace.md` (TRACE-1 to TRACE-4).

The trace comparator (DIFF-9), the expected divergences (DIFF-10) and statement coverage
(DIFF-11) are tracked as a follow-up to P1-13 (issue #378, P1-62).

## Tests

The scenario reader's number vectors run every text of
`docs/spec/vectors/scenario/numbers.txt` through the reader and check the verdict and value,
the acceptance criterion of P1-13 for DIFF-2.7 to DIFF-2.9; `cargo xtask oracles` runs the
same vectors through the id VM oracle.

The EDICT-8 port has unit tests for the `COM_Parse` tokenizer, the `atof`/`atoi` conversions,
the skill and deathmatch inhibition, and the spawn loop's branches (an angle rename, a
classless entity, each tokenizer error), each citing the statement it pins. The host loop has
end-to-end tests that run a scenario through `run_scenario` and check the trace it writes: a
map spawn, the settle frames, a server frame, a client entering, a refused progs, and a bad
entity lump, citing EDICT-8 and DIFF-3 to DIFF-5. The differential result against the id oracle
runs once the trace comparator exists (issue #378); until then the port is proven against the
EDICT-8 behaviour of `docs/spec/edicts.md`, written from id's source at the pinned commit.
