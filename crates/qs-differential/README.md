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

Tests. It builds on the L1 VM crates (`qc-vm`, `qc-edict`, `qc-builtins`) and the L0 formats
below them (`qc-progs`, `qc-trace`, `qc-types`), and nothing depends on it (`layers.toml`).

## Spec sections

- `docs/spec/differential.md` DIFF-2: the scenario file (`scenario` module).

The host loop (DIFF-3 to DIFF-5, DIFF-8), the trace comparator (DIFF-9) and statement coverage
(DIFF-11) are tracked as follow-up issues to P1-13.

## Tests

The scenario reader's number vectors run every text of
`docs/spec/vectors/scenario/numbers.txt` through the reader and check the verdict and value,
the acceptance criterion of P1-13 for DIFF-2.7 to DIFF-2.9. `cargo xtask oracles` runs the
same vectors through the id VM oracle.
