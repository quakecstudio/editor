# Fuzz targets

The cargo-fuzz targets for QuakeC Studio, run on nightly Rust with `cargo xtask fuzz`
(`make test-fuzz`) and by the nightly `Fuzz` workflow (`.github/workflows/fuzz.yml`,
TEST-5). This crate is outside the workspace (`docs/conventions.md`), so it has its own lock
file; each target is a `[[bin]]` in `Cargo.toml` with a module in `fuzz_targets/`.

A crash fails the run and the crashing input is uploaded as the `fuzz-artifacts` artifact;
minimise it into a regression test in the crate it found (`crates/<crate>`), as the no-panic
bar of `docs/conventions.md` and `docs/testing.md` requires.

## Targets

| Target | What it fuzzes | What it checks |
| --- | --- | --- |
| `parser` | `qc-syntax::parse_with` under the vanilla and the FTEQCC profiles, with grammar-aware mutation over both dialects' tokens (P1-22, P2-51) | Under each profile: the tree tiles the input and gives it back; nodes nest; diagnostics point inside the input; parsing is deterministic |
| `qc-preproc` | `qc-preproc` under the FTEQCC profile, the input split into two compiled files and an included one, preprocessed twice with `restart` between; and the `#if` evaluator under both profiles (P2-51) | No panic; every token and diagnostic lies inside the file it is in |
| `qc-progs` | The progs.dat reader and writer (PROGS-15, PROGS-22) | No panic; an accepted file writes back and rereads unchanged |
| `qc-vm` | `qc-vm` against the id VM oracle, on well-formed random progs (FND-10, P1-15) | `qc-vm` never panics or runs away; with the oracle, the two traces agree (DIFF-9) |
| `qc-project` | The `progs.src` project graph reader | No panic on any bytes |
| `qc-sema` | The semantic model (`Db::module_index`, `Db::analyze_module`, both profiles, with and without autoprototyping) and `qc_engines::missing_builtins`, on one to three files (P2-54) | No panic; every definition, reference and diagnostic inside its file; editing a file and re-resolving equals resolving from scratch |
| `qs-bsp` | The BSP, `.lit` and entity-lump readers and writer | No panic; face samples lie in the lighting lump; an edited entity lump rereads |
| `qs-map` | The `.map` reader | No panic on any bytes |
| `qs-mdl` | The MDL reader | No panic on any bytes |
| `qs-pak` | The PAK reader | No panic on any bytes |
| `qs-wad` | The WAD2 reader | No panic on any bytes |
| `qs-wav` | The WAV reader | No panic on any bytes |
| `xtask-vectors` | The VM vector parser | No panic on any bytes |

## The `qc-vm` differential target

`fuzz_targets/qc_vm.rs` is the fuzzing side of the differential VM suite (FND-10). Where the
end-to-end differential runner (`cargo xtask differential`, P1-13) compares the id VM oracle
and `qc-vm` on the progs the oracle compilers built from the corpus, this target builds an
endless supply of fresh *well-formed* progs from fuzzer bytes and compares the two VMs on
each.

- It builds a version 6 progs both VMs accept (`qs_differential::generate`): id's header CRC,
  the system globals and fields named at their fixed slots (EDICT-2.1, EDICT-3.1), and a
  `StartFrame` function whose body is a run of safe opcodes (arithmetic, comparison, boolean,
  bit, store, and forward-only `IF`, `IFNOT`, `GOTO`, ending at `DONE`) over the program's own
  scratch globals. The program never leaves its globals, never runs away and always reaches
  `DONE`; only the operand *values* are arbitrary, so the two VMs' arithmetic, NaN handling
  (VM-4.3) and float-to-int conversions are compared.
- It runs the progs in QuakeC Studio's VM through the differential host loop
  (`qs_differential::run_scenario`, DIFF-1.1), which must never panic (VM-19).
- When `QS_VM_ORACLE` names the id VM oracle binary (`cargo xtask oracles` builds it to
  `target/oracles/bin/`), it runs the same scenario in the oracle and compares the two traces
  statement by statement (`qs_differential::compare`, DIFF-9). Any `Comparison::Failure` is a
  divergence DIFF-10 does not expect, so a defect in `qc-vm`, the host or the oracle
  (DIFF-10.3), and fails the run.

Without the oracle (a plain `cargo xtask fuzz` or a pull-request build, which does not build
the C oracle) the target still runs `qc-vm` over every input and fails on a panic or runaway.
The nightly `Fuzz` workflow builds the oracle and sets `QS_VM_ORACLE`, so it does the full
comparison. The acceptance of P1-15, a 24-hour run with no divergence or crash before the P1
gate, is that nightly job's; the gate review (#49) reads its result.

## Corpus and regression

Each target keeps its corpus under `fuzz/corpus/<target>/`, which is git-ignored and
persisted across nightly runs by the workflow's `rust-cache` (`save-if` on `main`). A crash
the fuzzer finds is minimised and added as a regression *test* in the crate it found, as
`docs/conventions.md` and `docs/testing.md` require, not left only in the corpus: for the
`qc-vm` target that is a case in `crates/qs-differential/src/generate/tests.rs`
(`regression_body_arith_branch_goto_runs_clean` is the first), which replays the minimised
input on every `cargo test` run and so is checked on every change, not just nightly.
