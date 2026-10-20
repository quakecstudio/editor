# Oracles and the differential harness

**Crates:** `oracles/`, `qs-differential` (tests); `cargo xtask oracles`, `cargo xtask differential`. **Specs:** DIFF-1 to DIFF-11, TRACE, EDICT-8.

## Context and design philosophy

The project's core principle is that its foundations model what the Quake engine executes,
proven against id's own code (CLAUDE.md, Project overview). This leaf is the proof machinery:
the C oracles built from id's source and a pinned FTEQCC, the xtask commands that build and
check them, and the `qs-differential` crate that runs QuakeC Studio's VM beside id's and
compares the traces.

Observed principles, each traced to a source:

- The oracles are separate programs, run only in CI and by hand, never linked into the editor
  (`.claude/rules/layering.md`, "C programs are separate processes"; `oracles/README.md`).
- id's C is never committed. `oracles/sources.toml` pins every upstream file by commit and
  SHA-256, and xtask fetches it at build time (LIC-7; CLAUDE.md "What not to do" 5).
- Expectations are written from the spec, not from the C, so a fault in the C fails an
  expectation (`xtask/src/oracles/scenarios.rs:9-10`; `ratchets.toml:36`).
- The VM differential's hard bar is zero divergences; statement coverage is reported, not
  gated (ADR-0013; DIFF-11 intro).
- FTEQCC ports are proven by oracle differentials (ADR-0017, Decision: "the oracle
  differentials stay as the proof").

Engine and harness behaviour lives in `docs/spec/differential.md` (DIFF-1 to DIFF-11),
`docs/spec/trace.md` (TRACE), `docs/spec/edicts.md` (EDICT-8 map spawning) and the VM vectors
under `docs/spec/vectors/vm/`. This document links to them and does not restate them.

## Components

### The C oracles (`oracles/`)

| Path | Role |
| --- | --- |
| `oracles/sources.toml` | Pins three upstreams: id-Software/Quake `bf4ac424`, Quake-Tools `c0d1b91c`, fte-team/fteqw `f937b9d8` (sources.toml:9, 28, 46), and lists id functions to extract unchanged |
| `oracles/vm/host.c` | VM oracle host: arena, engine strings, run wrapper, error paths, progs checks, crash handler, vector mode, `main` |
| `oracles/vm/trace.c` | Binary trace writer (TRACE-5), written from the spec independently of `qc-trace` |
| `oracles/vm/scenario.c` | Scenario mode: DIFF-2 reader, DIFF-3 map setup, DIFF-4 clients, DIFF-5 frames, DIFF-8 errors |
| `oracles/vm/engine.c`, `builtins.c` | Mock engine (cvars, buffers, model bounds, rand) and the two 79-entry builtin tables |
| `oracles/vm/quakedef.h`, `oracle.h` | Stand-in for id's `quakedef.h` with the build's redirections; the oracle's own interfaces and exit codes |
| `oracles/vm/fuzz.c` | libFuzzer entry points for the scenario reader, init reader and progs loader |
| `oracles/qcc/` | Shims so id's qcc builds on C99 compilers and Windows |

### The xtask oracle commands (`xtask/src/oracles.rs`, `xtask/src/oracles/`)

`cargo xtask oracles` fetches and verifies the pinned sources, extracts id's functions with
`#line` directives (`oracles.rs:264-279`), builds `qcc`, `vm-oracle` and `fteqcc` with the
system C compiler at the reference-build flags of `docs/spec/vm.md` (`oracles/compile.rs:6-9`),
skips the rebuild when an input stamp is unchanged (`oracles.rs:288-310`), then runs, in
order and stopping at the first error (`oracles.rs:141-151`):

| Check | Module | What it proves | Plan |
| --- | --- | --- | --- |
| VM vectors | `oracles/vectors.rs` | The oracle's trace matches every vector in `docs/spec/vectors/vm/` | P1-11, P1-31 |
| Scenario numbers | `oracles/numbers.rs` | Refusal or run per vector of `docs/spec/vectors/scenario/numbers.txt` | P1-37 |
| Probes | `oracles/probes.rs` | Malformed scenario and init files exit and refuse as specified | P1-36 |
| Scenarios | `oracles/scenarios.rs` and `scenarios/*.rs` | More than 150 DIFF-1 to DIFF-10 and TRACE cases (`scenarios/tests.rs:171-197`) | P1-35 |
| Builtins | `oracles/builtins.rs` | FTEQCC and qcc builtin-initialiser outcomes against the qc-sema/qc-syntax profile | P2-69, P2-70 |

Separate subcommands: `oracles id101 <dir>` (id 1.01 compiled by qcc, digest check, two
scenarios run twice), `oracles rerelease <dir>` (six rerelease codebases compiled by FTEQCC,
digests of progs.dat and progs.lno), `oracles fuzz` (libFuzzer targets of `fuzz.c` with
ASan/UBSan), `oracles json <trace>`.

Recorded oracle outputs in `xtask/fixtures/oracles/` (`vector-traces.json`,
`scenario-outcomes.json`, `builtin-outcomes.json`, written by `record.py` and
`--record`) let the unit tests replay every vector, scenario and builtin case without a C
compiler.

### The differential runner (`crates/qs-differential`, `xtask/src/differential.rs`)

`cargo xtask differential [--fast|--full]` builds the oracles and runs the `differential`
binary with `cargo run` (`differential.rs:39-58`), because `qs-differential` is a tests-layer
crate nothing may depend on (`differential.rs:9-11`; `layers.toml:23`).

| Module | Role | Spec |
| --- | --- | --- |
| `scenario.rs` | Byte-level DIFF-2 scenario reader, mirroring `oracles/vm/scenario.c` limits | DIFF-2 |
| `host.rs`, `host/frames.rs` | Host loop: spawn server, enter clients, run frames, shut down; ported from sv_main.c, host.c, host_cmd.c, sv_phys.c | DIFF-3 to DIFF-5, DIFF-8 |
| `edict8.rs`, `host/spawner.rs` | Port of ED_LoadFromFile, ED_ParseEdict, ED_ParseEpair, COM_Parse over a `SpawnHost` trait | EDICT-8 |
| `host/trace.rs` | Trace builder adding string records and host error records | TRACE-4, DIFF-3.9 |
| `compare.rs` | Trace comparator and coverage helpers | DIFF-9, DIFF-10, DIFF-11 |
| `suite.rs` | Entity lump from a .map, initial globals, `run_and_compare`, `CodebaseReport` | DIFF-9, DIFF-11 |
| `runner.rs`, `runner/fixtures.rs` | Per-codebase loop over four hand-written scenarios; fast = id 1.01, full = id 1.01 plus six rerelease codebases (`runner.rs:59-68`) | DIFF-1 |
| `generate.rs` | Deterministic well-formed progs plus scenario from fuzzer bytes, for `fuzz/fuzz_targets/qc_vm.rs` | FND-10 |
| `bin/differential.rs` | CLI; spawns `<oracle> scenario <scn> <trace>` per scenario; exit 0/1/2 | DIFF-1 |

Where it runs: the fast subset in the `Test Assets (linux)` job of `ci.yml` through
`cargo xtask ci --skip` (`docs/ci.md:144`), and the full corpus nightly in
`.github/workflows/differential.yml`.

## Error model

- xtask commands return `Result<bool, String>`: `Ok(false)` is a failed check, `Err` a
  command that could not run (`xtask/src/main.rs:138-148`).
- The VM oracle exits 0 (traced), 2 (refused), 3 (I/O or uncaught error), 4 (crash)
  (`oracles/README.md:127-130`); a refused run that wrote a trace is an error
  (`oracles/scenarios.rs:745-747`).
- `qs-differential` returns `SuiteError {Refused, OracleTrace, Map, Progs}` (`suite.rs`) and
  `RunRefused {Progs, Edict, Trace}` (`host.rs`); every divergence `compare` returns is a
  `Comparison::Failure` (`compare.rs:155-163`).

## Verification

- The xtask oracle modules replay the recorded outputs in `xtask/fixtures/oracles/` in
  `cargo test`, so every vector, scenario and builtin case runs without a C compiler; CI's
  `test-assets` job builds the oracles and runs `cargo xtask oracles` for real.
- `qs-differential`'s unit tests cover the scenario reader, the host loop, the spawner, the
  comparator and the suite (`src/*/tests.rs`).
- `cargo xtask oracles fuzz` runs the C oracle's libFuzzer targets nightly.
- The C oracle's line coverage is measured by `cargo xtask readiness`; it has a mutation
  exception (`ratchets.toml`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| VM differential bar | Zero divergences; coverage reported, not gated | Flat 90% coverage; drive a running world | ADR-0013: the mock is specified not to drive the running world (DIFF-5.8, DIFF-1.5) |
| Oracle source handling | Fetch by commit and SHA-256, extract functions unchanged | Vendor id's C | LIC-7; CLAUDE.md "What not to do" 5 |
| FTEQCC's role | Test-only oracle and, since ADR-0017, porting upstream for engine/qclib | Fact-only use (ADR-0014) | ADR-0017 |
| Engine oracle (P3-23) | id's own engine | Pinned QuakeSpasm | ADR-0005:37, 51 (ADR-0004:128-129 says QuakeSpasm; see Open Questions) |
| Oracle expectations | Written from the spec, not the C | Record the C's output as truth | `oracles/scenarios.rs:9-10` [inferred: so the C is under test too] |
| Unit tests without a C compiler | Replay recorded oracle outputs from `xtask/fixtures/oracles/` | Build the C in every `cargo test` | [inferred] keeps PR CI fast (memory: lean CI) |
| Running the runner from xtask | `cargo run -p qs-differential --bin differential` | Link qs-differential into xtask | `differential.rs:9-11`: tests-layer crate may not be depended on |
| Expected divergences | Handled by construction: an agreeing error record ends the comparison | A classified `Expected` result | `compare.rs:172-179` [inferred] |
| Mutation score for the C | Exception (mutation = 0) | Port a C mutation tool | `ratchets.toml:30-37`, `oracles/README.md` Readiness |

## Open questions

### Resolved
1. Coverage gate for the VM differential removed (ADR-0013, 2026-10-17).
2. FTEQW engine/qclib is a porting upstream (ADR-0017, 2026-10-20).

### Deferred
1. **DIFF-11.2 boundary.** The spec counts "only the records before the first divergence";
   `records_up_to` counts "up to and including" the diverging carrier (`suite.rs:202-216`),
   and a Header divergence counts the first record (`suite.rs:195`). The test pins the
   including behaviour (`suite/tests.rs:208-215`). Code or spec must change.
2. **DIFF-9.6 expected divergences.** `compare` has no expected classification
   (`compare.rs:155-163`). The VM-9.2/BUILTIN-2.2 and BUILTIN-6.9 rows of DIFF-10.1 would be
   reported as failures if the oracle ran off x86 or with another libm; only
   `oracles/vectors.rs:200-210` treats VM-9.2 as expected. Safe only while the reference
   oracle runs on x86-64 Linux [inferred].
3. **ED_Alloc error text.** `host/spawner.rs:44-48` maps `NoFreeEdicts` to
   `SpawnError::ParseError`, so a full edict array reports "ED_ParseEdict: parse error"
   instead of EDICT-6.2's `ED_Alloc: no free edicts`.
4. **Entity keys.** `edict8.rs:298-302` stores the `atoi` result for an `ev_entity` key, not
   id's `EDICT_NUM(atoi)` value; acknowledged in a comment as almost always 0 for a map. Not a
   DIFF-10 row.
5. **Client think window.** `host/frames.rs:230, 236` passes `self.scenario.frametime` to
   `run_think`, not the frame length it was called with, so a settle frame uses the scenario
   frametime for a client's think. Not checked against DIFF-5.
6. **Start-map errors.** An error in `start_map`, including the settle frames, returns
   `Ended::Error` without `shut_down` (`host.rs:197-199, 253-259`); `bad_movetype` does run
   `shut_down` though its doc cites DIFF-8.6 "no shutdown" (`host/frames.rs:27-43`).
7. **Doc drift.** `docs/ci.md:184` says the Differential workflow enforces the 90% coverage bar;
   `docs/porting.md:211-212` states the bar without the ADR-0013 exception. The crate README
   says nothing depends on it (`README.md:20-21`), but `fuzz/fuzz_targets/qc_vm.rs` uses
   `qs_differential::generate`; README lines 42-45 and 67-69 read as written at different times.
8. **Engine oracle ADR conflict.** ADR-0004:128-129 and :163 name a pinned QuakeSpasm; ADR-0005
   names id's engine. ADR-0004 carries no "overridden" link.
9. **Duplication.** Four copies of the x86 float-to-int rule (`edict8.rs:648-661`,
   `host.rs:453-464`, plus qc-vm and qc-builtins); three progs.dat v6 encoders
   (`generate.rs`, `host/fixture.rs`, qc-vm `tests/harness.rs`); two change-set observers
   (`host/trace.rs` and qc-vm `trace.rs`, intentional per `host/trace.rs:9-14`); two
   `link_edict`; `EXIT_REFUSED` and minimal-progs builders in `numbers.rs`, `probes.rs`,
   `scenarios.rs:782`.
10. **Stale spec citations in `generate.rs`.** VM-17, VM-11.4, VM-11.1, VM-6.1/7.3 are cited
    for behaviour those statements do not describe (`generate.rs:24, 91, 204, 209, 443`); doc
    says "one server frame" while FRAMES=4 (`generate.rs:74, 81`).
11. **Paths and output.** `bin/differential.rs:93` uses a hard-coded relative
    `target/differential`, not `qs-paths`; `runner.rs:21-24` and the oracle modules print with
    `println!` rather than `tracing`. Whether the CLAUDE.md paths and diagnostics rules apply
    to test tooling is not stated.
12. **Process seam.** `xtask/src/differential.rs:39-58` calls `Command` directly and has no
    test module; `download.rs:35-43` likewise. The oracle build stamp hashes only
    `oracles.rs`, `extract.rs` and `compile.rs` of the xtask sources (`oracles.rs:291-296`).
13. **Recorded fixture coupling.** `NOT_RECORDED` in `scenarios/tests.rs:21-31` must equal the
    cases `record.py` drops; the two lists live in different languages.
14. **Weak tests.** Several `host/tests.rs` cases assert only `Ended::Normally`
    (`host/tests.rs:121-152, 290-319`); `edict8/tests.rs` has no test for EDICT-8.3 short
    vectors, the `light` rename, the trailing-space key trim or `ev_entity` conversion; the
    runner tests' fake oracle re-runs the qs runner, so they prove plumbing only.
15. **qcc success test.** `oracles/id101.rs:119-128` judges qcc success by the absence of
    `ERROR` in stdout and the presence of progs.dat, not by exit status.
16. **Exit status 4.** `oracle.h:13-17` defines 0, 2 and 3; status 4 is a literal at
    `host.c:581`, and an uncaught error exits 3, the I/O status (`host.c:368-372`).
17. **Planned suites.** The compiler differential against id's qcc on the corpus (P2-09, #65;
    TEST-6's 90% bar applies to it, ADR-0013); id's engine as the engine oracle, tracing every
    server frame (P3-23, #237; ADR-0005); the server-simulation differential owning
    running-world statement coverage (P3-25, #239; ADR-0013); the map compiler oracles and their
    suite once the license review allows (P6-03, P6-07; P4-22).
18. **No readiness table for `qs-differential`.** It is a tests-layer crate with no
    `ratchets.toml` entry and no readiness report; whether TEST-1 to TEST-3 apply to it is not
    stated in `docs/testing.md`.

## References

- Specs: `docs/spec/differential.md`, `docs/spec/trace.md`, `docs/spec/edicts.md` (EDICT-8),
  `docs/spec/vm.md` (Reference build), `docs/spec/vectors/vm/`, `docs/spec/vectors/scenario/`
- ADRs: `docs/adr/0013-differential-coverage-bar.md`, `docs/adr/0017-port-fteqcc.md`,
  `docs/adr/0005-*.md`, `docs/adr/0004-preview-host-base.md`
- `oracles/README.md`, `crates/qs-differential/README.md`, `docs/porting.md`, `docs/ci.md`
- Plan: P1-11 to P1-15, P1-26, P1-31, P1-35 to P1-37, P2-61, P2-69, P2-70; planned P2-09 (#65),
  P3-23, P3-25, P6-03, P6-07
- Readiness: `docs/readiness/p1-rg.md`, `docs/readiness/p2-r1.md`
