# Readiness: P1 C tools (P1-RG)

- **Issue:** #56 (`P1-RG`)
- **Reviewer:** a separate Claude agent session started for this review; did not write the layer's code
- **Date:** 2026-10-16
- **Commit measured:** e11be0f561c5b00d4ecfb7f54439e54f270f036a
- **Verdict:** Not ready

The layer cannot be measured against the bar with the tools that exist, and where it can be
measured it is below the bar. The review also found four defects in the oracle and its driver
and one gap in the spec the oracle implements. Each is a new issue that this review now depends
on (section 7).

## 1. Crates and spec sections

The C tools layer has no Cargo crate. It is the code of P1-11, P1-26 and P1-32:

| Component | What it is | Spec sections |
| --- | --- | --- |
| `oracles/vm/*.c`, `oracles/vm/*.h` (2,882 lines) | Our VM oracle harness around id's `pr_exec.c` | DIFF-1 to DIFF-8, DIFF-10.1, TRACE-3 to TRACE-5, the BUILTIN and EDICT readings in `oracles/README.md` |
| `oracles/qcc/` | What id's qcc needs to compile on C99 and Windows | none |
| `xtask/src/oracles.rs`, `xtask/src/oracles/` (2,739 lines of Rust in the `xtask` crate) | Fetch by checksum, extract id's functions, compile, run the vectors, id 1.01 and the rerelease | VM-5 "Reference build", DIFF-9.6, LIC-7 |
| id's `pr_exec.c` and extracted functions, FTEQCC, id's qcc | Upstream code built unchanged | not measured: not ours (they are the oracle) |

## 2. Coverage, mutation score and spec citations

Readiness workflow run on `xtask`, the only crate the layer has code in:
[38884689754](https://github.com/quakecstudio/editor/actions/runs/38884689754). The workflow
measures Cargo crates only; nothing in it measures `oracles/`.

| Component | Lines (bar 90%) | Branches (bar 80%) | Mutation (bar 90%) | Spec citations (bar 100%) |
| --- | --- | --- | --- | --- |
| `xtask` (whole crate, CI) | 71.6% (6,008 of 8,387): fails | 88.4% (1,005 of 1,136) | not measured: the baseline run failed | 0 of 0 statements listed |
| `xtask` oracle module (local, `cargo llvm-cov -p xtask`) | 41.0% (793 of 1,933) | not measured | not measured | not measured |
| `oracles/vm/*.c` (local, clang source-based coverage) | 80.6% (1,390 of 1,724): fails | 64.5% (581 of 901): fails | not measured: no tool | 0 of 63 DIFF statements: fails |

- **xtask mutation:** cargo-mutants copies the tree to a directory without `.git`, and three
  xtask tests need a git checkout (`attribution::tests::the_repository_passes`,
  `licenses::tests::the_repository_passes`,
  `corpus::verify::tests::the_repository_tracks_no_corpus_files`, each failing on
  `git ls-files`), so the unmutated baseline fails and no mutant is tested.
- **The xtask oracle module per file** (local, after this review's tests): `oracles.rs` 0%,
  `compile.rs` 0%, `id101.rs` 0%, `vectors.rs` 47.0%, `rerelease.rs` 56.0%, `extract.rs` 100%,
  `sha256.rs` 100%. The build, run and compare code runs only in `cargo xtask oracles` in CI.
- **Our C, per file** (lines, branches): `host.c` 86.2%, 71.3%; `trace.c` 91.5%, 75.0%;
  `engine.c` 71.8%, 64.3%; `builtins.c` 75.8%, 56.9%; `scenario.c` 76.9%, 57.4%. Measured on
  macOS arm64 by building `vm-oracle` with `clang -fprofile-instr-generate -fcoverage-mapping`
  at `-O0` (the other flags as `compile.rs`), putting it in place of
  `target/oracles/bin/vm-oracle`, and running `cargo xtask oracles` (every vector) and
  `cargo xtask oracles id101` (both scenarios). The unexercised code is mostly the refusals and
  error exits, which no test runs.
- **Spec citations:** all 63 DIFF statements are in `docs/spec/coverage-baseline.tsv`, with
  no citing test. TRACE-5.1 to TRACE-5.7 are cited by `qc-trace`'s tests, which test the Rust
  encoder and decoder, not `trace.c`.

No surviving mutants are listed: no mutation run reached a mutant.

## 3. Property, fuzz and differential results

| Component | Property tests (TEST-4) | Fuzzing (TEST-5) | Differential (TEST-6) |
| --- | --- | --- | --- |
| `oracles/vm` | none: the `trace.c` encoder is checked only by decoding and re-encoding the two id 1.01 scenario traces (TRACE-5.7, `id101.rs`), not by a property over generated runs | none: no target for the scenario file, the progs loader or the init file | 264 vectors, 0 divergences on Linux and Windows x86-64; 262 and 2 expected divergences (VM-9.2, DIFF-10.1) on macOS arm64; 14 vectors are `oracle: no`. id 1.01 compiles with id's qcc and both scenarios trace, the same digest on two runs |
| `xtask` oracle module | none | none for the vector parser | not applicable |
| FTEQCC (P1-26) | | | the six rerelease codebases compile as version 6 with vanilla opcodes only and matching `.lno` files |

CI evidence: [CI run 38883839106](https://github.com/quakecstudio/editor/actions/runs/38883839106)
on e11be0f: `Unit Tests (linux)`, `(windows)` and `(macos)`, step "Build and check the
oracles"; `Test Assets (linux)`, steps "Compile and trace id 1.01 with the oracles" and "Compile
the rerelease with FTEQCC". The local run on macOS arm64 gave the same vector counts as CI's
macOS job.

TEST-8 (performance) has no budget for the oracles.

## 4. Flakes

| Component | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `xtask` unit tests (run 38884689754) | 20 of 20 | 20 of 20 | 20 of 20 |
| `cargo xtask oracles` (the vectors through the oracle) | not measured | not measured | not measured |

The xtask unit tests do not run the oracles, so the flake job says nothing about them. The
oracle job has passed on all 8 runs of CI on `main` since it landed (#280 to #292), one run per
commit with the build often restored from cache; that is not the 20-run job of TEST-7.

No test waits on the wall clock: `rg "sleep|Instant::now|SystemTime"` over the xtask oracle
module and `rg "sleep|time\(|clock\(" oracles/vm` find none outside a test string; the oracle's clock is the scenario's
`frametime` (DIFF-5).

## 5. Independent review (TEST-9)

- **Reviewer and time spent:** a separate Claude agent session, one working session on
  2026-10-16.
- **Tests added** (passing, kept in this PR):
  - `xtask/src/oracles/extract.rs`: `copies_one_line_and_same_line_brace_definitions`,
    `ignores_directives_and_braces_inside_block_comments`,
    `handles_escaped_quotes_and_char_literals`, `skips_an_assignment_naming_the_function`
  - `xtask/src/oracles/vectors.rs`: `rejects_values_outside_32_bits`,
    `rejects_malformed_string_escapes_and_trailing_text`,
    `keeps_a_quoted_error_message_and_a_bare_error`, `keeps_a_well_formed_after_line`,
    `rejects_bad_operands_and_function_fields`
- **Tests written that fail** (kept out of this PR so CI stays green; each is the regression
  test of its issue, in the appendix below): `skips_a_comment_line_naming_the_function`,
  `skips_a_declaration_with_a_trailing_comment`, `refuses_a_malformed_error_line`,
  `refuses_a_malformed_after_line`.
- **Defects found:**
  - **P1-36**, `vm-oracle` accepts malformed scenario and init files. The scenario reader
    stops at a NUL byte, so `progs progs.dat\nframes 1\nmap te\0st\nbogus 1\n` runs (exit 0)
    although DIFF-2.3 says an unknown key is refused; `frames 0000000000000000001` is refused
    (exit 2) although it is decimal digits in range (`parse_integer` refuses more than 18
    digits). The vector-mode init file reads numbers with `atoi` and `strtoul`:
    `global 100x bits zz` and `seed abc` run (exit 0), and `bits 1ffffffff` is truncated.
  - **P1-37**, spec gap: DIFF-2.3 does not say which number texts `frametime`, `model` and
    `input` accept. `vm-oracle` accepts `.1` and refuses `+0.1` (exit 2, "frametime takes a
    number"); Rust's `f64` parsing, which the P1-13 runner would use, accepts both, and `inf`.
  - **P1-38**, `cargo xtask oracles` loosens malformed vector lines: `after: depth=x
    localstack=2` parses as no `after:` check at all, and `error "open`, `errors` and
    `error "a # b"` (cut at the `#`) each accept any error message.
    `crates/qs-tests/tests/it/spec_vectors.rs` also accepts `error "open`.
  - **P1-39**, `extract.rs` takes a column-0 line in a block comment that names the function
    before `(`, or a declaration followed by a `//` comment, as the start of the definition,
    and copies on into the next function. id's pinned files do not trip it today (the build
    passes), but id's comment headers often show a call at column 0.
  - **P1-35**, the measurement gap in sections 2 to 4.
- **Probes that found nothing:** a 30-byte progs and a progs whose statement table lies past
  the end are refused (exit 2); a trace path in a missing directory gives exit 3; CR LF line
  ends, an empty entities file and a lone CR inside a line behave as DIFF-2.1 says.

## 6. Ratchets

None committed: the layer does not pass, and it has no crate table to ratchet. P1-35 makes the
layer measurable; the next review records its minimums in `ratchets.toml` with
`readiness = "P1-RG"`.

## 7. Known gaps

Every gap found blocks readiness; `P1-RG` now depends on each issue (`plan/backlog/p1.yaml`,
`plan/backlog/readiness.yaml`; P1-37 through P1-36).

| Gap | Issue | Why it does not block |
| --- | --- | --- |
| No measurement of `oracles/` against the bar; xtask lines 71.6%, xtask mutation not runnable, our C below 90% lines and 80% branches, 0 of 63 DIFF statements cited, no fuzzing, no 20-run oracle flake job | P1-35 | It blocks |
| Scenario and init files: NUL byte, 19-digit integers, unchecked init values | P1-36 | It blocks |
| DIFF-2.3 number syntax | P1-37 | It blocks |
| Vector checker loosens `after:` and `error` lines | P1-38 | It blocks |
| Function extraction from comments and commented declarations | P1-39 | It blocks |

No exception to the bar is recorded.

## Appendix: the failing regression tests

In `xtask/src/oracles/extract.rs`, `mod tests` (P1-39):

```rust
#[test]
fn skips_a_comment_line_naming_the_function() {
    let source = "/*\n==========\nf\n\nf (entity e)\n==========\n*/\nvoid f (int e)\n{\n}\n";
    assert_eq!(function(source, "f").unwrap().line, 8);
}

#[test]
fn skips_a_declaration_with_a_trailing_comment() {
    let source = "void f (void); // declared\nvoid g (void)\n{\n}\nvoid f (void)\n{\n}\n";
    assert_eq!(function(source, "f").unwrap().line, 5);
}
```

In `xtask/src/oracles/vectors.rs`, `mod tests` (P1-38):

```rust
#[test]
fn refuses_a_malformed_error_line() {
    for bad in ["error \"open", "errors", "error \"a # b\""] {
        let text = format!("== v\nexpect:\n  {bad}\n");
        assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
    }
}

#[test]
fn refuses_a_malformed_after_line() {
    for bad in ["depth=x localstack=2", "depth=0", "localstack=2"] {
        let text = format!("== v\nafter: {bad}\n");
        assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
    }
}
```

The scenario probes of P1-36, run as `vm-oracle scenario <file> <trace>` beside a copy of the
id 1.01 `progs.dat` that `cargo xtask oracles id101` builds:

```text
printf 'progs progs.dat\nframes 1\nmap te\0st\nbogus 1\n'      exit 0, should be 2
printf 'progs progs.dat\nframes 0000000000000000001\n'          exit 2, should be 0
printf 'progs progs.dat\nframes 1\nframetime +0.1\n'            exit 2 (P1-37: unspecified)
printf 'progs progs.dat\nframes 1\nframetime .1\n'              exit 0 (P1-37: unspecified)
```

And of the vector-mode init file, as `vm-oracle vector <progs.dat> <init> <trace>`:

```text
printf 'edicts 3\nglobal 100x bits zz\nseed abc\nentry 1\n'     exit 0, should be 2
printf 'edicts 3\nglobal 100 bits 1ffffffff\n'                   exit 0, should be 2
```
