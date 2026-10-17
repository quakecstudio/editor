# Readiness: P1 C tools (P1-RG)

- **Issue:** #56 (`P1-RG`)
- **Reviewer:** an independent Kiro agent session (`review-56`) started for this review; did not write the layer's code
- **Date:** 2026-10-17
- **Commit measured:** `b670ff6` (`main` at the time of the readiness run; the Readiness workflow resolved it as `f3dc54e1fb89da6852db922499a0233137369e1d`)
- **Verdict:** Ready

The layer is measurable against the bar and clears it. The five defects the first review
(PR #294, "not ready") found are fixed and their issues closed: P1-35 (measurement, #295),
P1-36 (strict scenario and init reading, #296), P1-37 (DIFF-2.8 number syntax, #297), P1-38
(malformed after and error lines, #298) and P1-39 (function extraction from comments, #299).
This review re-measured the layer with the Readiness workflow, probed the oracle against the
DIFF-2 refusals the earlier probes did not reach, and found no new defect that blocks.

## 1. Crates and spec sections

The C tools layer's own code is the VM oracle's C; its Rust driver is in `xtask` (the tooling
crate, measured separately). The layer is measured as the `oracles` crate
(`oracles/README.md`, Readiness).

| Crate | Layer | Spec sections |
| --- | --- | --- |
| `oracles` (`oracles/vm/*.c`, `oracles/vm/*.h`) | c-tools | DIFF-1, DIFF-2, DIFF-3, DIFF-4, DIFF-5, DIFF-6, DIFF-7, DIFF-8, DIFF-10, TRACE-1, TRACE-2, TRACE-3, TRACE-4, TRACE-5 (`oracles/README.md`) |
| `oracles/qcc/` | c-tools | none (id's qcc built unchanged, with a C99 and Windows shim) |
| id's `pr_exec.c`, extracted id functions, FTEQCC, id's qcc | c-tools | not measured: not ours (they are the oracle, built unchanged) |

The Rust that drives the oracles is `xtask/src/oracles.rs` and `xtask/src/oracles/`, in the
`xtask` tooling crate; `cargo mutants` measures it there. It is not part of the c-tools layer
and is not ratcheted by this review (see section 7).

## 2. Coverage, mutation score and spec citations

Readiness workflow run, `crates="oracles"`:
[38991237014](https://github.com/quakecstudio/editor/actions/runs/38991237014) (success). The
oracles' C is measured with clang-18 source-based coverage on every vector, number vector and
probe (`cargo xtask oracles`) and on the id 1.01 scenarios (`cargo xtask oracles id101`),
counting the files of `oracles/vm/` and not id's.

| Crate | Lines (bar 90%) | Branches (bar 80%) | Mutation (bar 90%) | Spec citations (bar 100%) |
| --- | --- | --- | --- | --- |
| `oracles` | 95.6% (1858 of 1943): ok | 88.8% (883 of 994): ok | waived by an exception (below) | 100.0% (85 of 85): ok |

- **Mutation** is waived by the exception recorded in `ratchets.toml` and explained in
  `oracles/README.md` (Readiness): cargo-mutants, the bar's mutation tool (TEST-2), mutates
  Rust only, and the oracles are C; every vector, probe and scenario test states the trace or
  exit status the spec gives, written from the spec without the C, so a fault in the C fails
  an expectation. No surviving-mutant list applies.
- **Spec citations** cover all 85 active statements of the DIFF and TRACE sections the oracle
  implements, cited by the scenario tests, vectors, number vectors and probes
  (`cargo xtask readiness spec oracles` reported 100.0% (85 of 85)).

No surviving mutants (the C has no mutation run; the Rust driver is measured as `xtask`,
section 7).

## 3. Property, fuzz and differential results

| Crate | Property tests (TEST-4) | Fuzzing (TEST-5) | Differential (TEST-6) |
| --- | --- | --- | --- |
| `oracles` | not applicable: the oracle is a trace runner, not an encoder or decoder with a round-trip invariant; its binary trace is round-tripped by `qc-trace`'s strict decoder in the scenario tests | `oracles/vm/fuzz.c` has a libFuzzer target for each reader of a file: `scenario` (DIFF-2), `init` (vector mode) and `progs` (the loader). Nightly `Fuzz` workflow run [38912876264](https://github.com/quakecstudio/editor/actions/runs/38912876264): each target 600s, all passed, no crash. This review also ran each locally (Homebrew LLVM clang, address and UB sanitizers): scenario 180s, init 120s, progs 120s, all clean. The 24 h run (TEST-5) is a known gap (section 7) | `cargo xtask oracles` on the review's machine (macOS arm64): 262 vectors passed, 2 expected divergences (VM-9.2, DIFF-10.1), 0 failed, 14 `oracle: no`; 107 scenario number vectors passed; 69 input file probes passed; 189 scenario tests passed. id 1.01 compiles with id's qcc and both scenarios trace. CI run links in the Readiness run above and in the merged PRs (#280 to #336) |

TEST-8 (performance): no budget applies to the oracles (`docs/perf-budgets.md` lists none for
`oracles/`).

## 4. Flakes

20 consecutive runs of `cargo xtask oracles` on each OS (TEST-7), from the Readiness run
[38991237014](https://github.com/quakecstudio/editor/actions/runs/38991237014):

| Crate | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `oracles` | 20 of 20 | 20 of 20 | 20 of 20 |

No test waits on the wall clock: the oracle's only clock is the scenario's `frametime`
(DIFF-5); `rg "sleep|Instant::now|SystemTime"` over `xtask/src/oracles/` and
`rg "sleep|time\(|clock\("` over `oracles/vm/` find none outside test strings and the
`frametime` handling.

## 5. Independent review (TEST-9)

- **Reviewer and time spent:** an independent Kiro agent session, one working session on
  2026-10-17. Built the oracle locally and probed its scenario and vector readers against the
  DIFF-2 text.
- **Tests added** (passing, kept in this PR; six new probes in `xtask/src/oracles/probes.rs`
  that run through the built oracle in CI on all three OSes, each a permanent regression test):
  - `map Test` and a 64-byte `map` name: DIFF-2.2 refuses a name that is not 1 to 63 bytes
    from `a` to `z`, `0` to `9` and `_`.
  - `submodels 255`: DIFF-2.2 refuses an integer above 254.
  - `frames 0` then `frames 1`: DIFF-2.9 refuses a repeat of a key that may appear once.
  - `input 1 1 button1 1`: DIFF-2.6 refuses an input field other than button0, button2,
    impulse or v_angle.
  - `input 1 1 impulse 256`: DIFF-2.6 refuses an impulse outside 1 to 255.
  These fill gaps the earlier probes (NUL bytes, leading zeros) and the number vectors
  (frametime, DIFF-2.8) did not cover: directive-level refusals DIFF-2.9 and DIFF-2.6 name.
- **Defects found:** none new. Every spec-grounded probe the review ran against the built
  oracle agreed with DIFF-2: map-name validation, the frametime boundary (`>0` and `<=1`),
  every DIFF-2.8 number form (`.1`, `1.`, `.`, `-.`, `.e5`, `1..2`, `1e`, `1e+`, `1e1.0`,
  `1E-1`, hexadecimal, a leading `+`, trailing junk), submodels and clients ranges, the
  repeated-once keys, and every `input` field and range. A 600-byte comment line is read and
  skipped, which is correct: the 512-byte and eight-word limits apply to the `vector` init
  file, not to scenario files, whose parser reads lines by length (`oracles/vm/scenario.c`).
- **One observation out of this layer:** a Readiness run that includes `xtask` (the tooling
  crate that drives the oracles) reports one surviving mutant,
  `xtask/src/main.rs: replace main -> ExitCode with Default::default()`, which no test kills
  because `fn main` cannot be unit-tested in Rust and `ExitCode::default()` is `SUCCESS`
  (the logic is in the tested `run`). `xtask` is the tooling layer, not c-tools, so it does
  not block P1-RG; filed as P1-55 (section 7).

## 6. Ratchets

The minimums committed to `ratchets.toml` in this pull request, with `readiness = "P1-RG"`
(mutation is below the bar and waived by the exception, so its minimum is 0):

```toml
[crates.oracles]
readiness = "P1-RG"
lines = 95
branches = 88
mutation = 0
spec = 100
reason = "P1-35: no mutation tool of the bar runs on the oracles' C"
exceptions = { mutation = "cargo mutants ... mutates Rust only, and the oracles are C ... (oracles/README.md, Readiness)" }
```

`cargo xtask ratchet` in the Readiness run printed these minimums for the passing run
("7 checks passed").

## 7. Known gaps

| Gap | Issue | Why it does not block |
| --- | --- | --- |
| TEST-5's full 24 h fuzz of the three oracle readers (scenario, init, progs) has not been run; the nightly `Fuzz` workflow runs each for 10 minutes and this review ran each for 2 to 3 minutes locally, all clean | #46 (`type:test`, the fuzz harness) and the nightly `Fuzz` workflow | The harness, seeds and a clean short run exist and run nightly; the 24 h run is a duration, not a capability, and the readers are small and sanitizer-clean. A crash would become a regression test (`oracles/README.md`, Fuzzing). Recorded, not waived |
| The `xtask` tooling crate (the oracle's Rust driver) has one surviving mutant in `fn main` | P1-55 (`type:test`) | `xtask` is the tooling layer, not the c-tools layer this issue covers; it is measured separately and is not ratcheted here |

Exceptions to the bar: `oracles` mutation, in `oracles/README.md` (Readiness) and under
`exceptions` in `ratchets.toml`, for the reason in section 2.
