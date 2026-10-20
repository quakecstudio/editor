# Corpora, corpus checks, fuzzing and the cross-crate tests

**Crates:** `corpus/`, `fuzz/`, `qs-tests` (tests); the corpus commands of `xtask`. **Specs:** none of its own; TEST-3 and TEST-5 of `docs/testing.md`.

## Context and design philosophy

The layers above the formats are only as trustworthy as the real code they have been run on.
This leaf holds that evidence: the pinned conformance corpus of QuakeC codebases, maps and
open test assets; the xtask commands that fetch it and run each layer's crates over it; the
cargo-fuzz targets; and the single cross-crate test binary `qs-tests`, which carries the
spec-citation ratchet.

Observed principles, each traced to a source:

- Nothing in the corpus is committed. Every entry is fetched at a pinned commit and checked
  by SHA-256, and the cache is never inside the source tree (`corpus/README.md:3-5`, Policy;
  LIC-4, LIC-5, LIC-7).
- Policy is code: `cargo xtask corpus verify` fails on a tracked `.pak`, on tracked Quake
  content outside `fixture_dirs`, and on a manifest that does not list QuakeC 1.06 and id's
  paks as `never` (`xtask/src/corpus/verify.rs:307-312, 373-394`).
- Licenses come from the item's own files, pinned by hash, read conservatively as `-only`
  when no "or later" grant is given (`corpus/README.md`, Policy).
- Each layer's readiness note names its corpus evidence (`docs/testing.md`, Layer notes): L0
  round-trips every corpus file of its format, L2 round-trips the whole corpus losslessly and
  checks recovery by deleting each token, L3 checks golden indexes.
- A fuzz crash becomes a regression test in the crate it found, not only a corpus entry
  (`fuzz/README.md:59-67`; TEST-5).
- Cross-crate tests are modules of one binary, `crates/qs-tests/tests/it/`, never new files
  under `tests/` (CLAUDE.md, Testing; CONV-11).

## Components

### The corpus (`corpus/`)

`corpus/manifest.toml` holds 19 `[[source]]`, 24 `[[codebase]]`, 26 `[[map]]`, 2 `[[asset]]`
and 1 `[[tree]]` entries, plus `never`, `fixture_dirs` and `blocked_urls`. Each entry has a
`use` of `vendor`, `fetch-at-test` or `never`, a license, `evidence` files and checksums.
`corpus/projects/xonotic/quakec.toml` is a project file for opening Xonotic whole.
`corpus/language_coverage.py` checks that `docs/spec/language.md` covers every construct id
1.01 uses (P1-06), with its own unittest.

### Corpus tooling (`xtask/src/corpus.rs`, `xtask/src/corpus/`)

| Command | Module | What it does |
| --- | --- | --- |
| `corpus verify [--online]` | `corpus/verify.rs` | Manifest policy and repository scan offline; `--online` downloads every pinned file and compares SHA-256 |
| `corpus fetch [<id>...]` | `corpus/fetch.rs`, `corpus/archive.rs` | Downloads archives by checksum into the cache, writes codebases, trees and maps; own tar.gz and zip readers |
| `corpus build [<id>...]` | `corpus/build.rs` | Compiles id 1.01 with id's qcc and the rerelease with FTEQCC; round-trips progs.dat through `qc-progs` and `.lno` through `qc-lno` (PROGS-22.1, LNO-6.2, LNO-4.1) |
| `assets fetch`, `assets precache` | `corpus/assets.rs` | Fetches the pinned test-asset zips, installs stand-ins, checks the rerelease's precaches are covered (QA-4) |

Every subcommand runs the offline manifest checks first (`corpus.rs:63, 93`). The cache is
`--dest`, else `$QS_TEST_ASSETS`, else `quakecstudio/test-assets` under `$XDG_CACHE_HOME` or
`~/.cache`.

### Per-layer corpus checks (xtask)

| Command | Layer | Check | Plan |
| --- | --- | --- | --- |
| `lex-corpus` | L2 | Lexes top-level `.qc`, preprocesses the files progs.src lists, fails on any error | P1-20 |
| `parse-corpus [--vanilla] [--fteqcc] [--deletions]` | L2 | Lossless round trip; optional error-free and local-recovery checks | P1-21, P2-51 |
| `project-corpus [--files] [--fteqcc] [--progs] [--expect-warning]` | L2 | Opens codebases with `qc-project`; checks file order against qcc's progs.dat and FTEQCC's printed order | P1-23, P1-33 |
| `map-corpus` | L0 | Every `.map` reads and writes back byte for byte | P5-02 |
| `sema-corpus [--write]` | L3 | Golden index per module (`docs/spec/vectors/sema/`), optional definition differential against a progs.dat | P2-54 |
| `fteqcc-coverage` | L2/L3 | Inventory of rerelease non-vanilla constructs against `docs/spec/dialect-fteqcc-rerelease.md` | P2-01, P2-33 |
| `fteqcc-full-coverage` | L2/L3 | Inventory of full-FTEQCC constructs in nuclide/freecs/freehl against `docs/spec/dialect-fteqcc.md` | P2-25 |
| `fteqcc-profile` | L3 | Every module analyses without error under the FTEQCC profile; the vanilla parser rejects each extension | P2-03, P2-26, P2-32, P2-38 |

These run in the `Test Assets (linux)` job of `.github/workflows/ci.yml` (lines 426-600),
except `fteqcc-coverage` and `fteqcc-full-coverage`, which no workflow runs.

### Fuzzing (`fuzz/`, `xtask/src/fuzz.rs`)

`fuzz/` is a cargo-fuzz package outside the workspace with its own lock file and 13 targets
(`fuzz/Cargo.toml`): `parser`, `qc-preproc`, `qc-progs`, `qc-project`, `qc-sema`, `qc-vm`
(differential against the oracle when `QS_VM_ORACLE` is set), `qs-bsp`, `qs-map`, `qs-mdl`,
`qs-pak`, `qs-wad`, `qs-wav` and `xtask-vectors`. `cargo xtask fuzz [<target>...] --time
<duration>` runs them one after another on nightly for the host triple (`xtask/src/fuzz.rs`).
The nightly `Fuzz` workflow runs each for 10 minutes (`.github/workflows/fuzz.yml`, `cargo
xtask fuzz --time 10m`). The C oracle's own libFuzzer targets belong to the
`oracles-differential` leaf.

### Cross-crate tests (`crates/qs-tests`)

| Module | What it checks |
| --- | --- |
| `spec_coverage.rs` | The spec-citation ratchet: every active statement in `docs/spec/*.md` has a `// spec` citation in `crates/`, `apps/` or `xtask/` (not `xtask/fixtures/`), the baseline only shrinks, `requirements.tsv` is current |
| `spec_vectors.rs` | VM vector files are well formed, cite active statements and cover every vanilla opcode |
| `scenario_numbers.rs` | `docs/spec/vectors/scenario/numbers.txt` against DIFF-2.7 to DIFF-2.9 |
| `builtins.rs`, `map_entities.rs`, `diagnostics_unified.rs`, `deep_nesting.rs`, `paths.rs` | Cross-crate behaviour: builtins through the VM, entity lump edits into a BSP, editor and build diagnostics equal, 100000-deep nesting, app directories |

## Error model

- xtask commands return `Result<bool, String>`; a failed check prints `FAIL` lines and returns
  `Ok(false)` (`xtask/src/main.rs:138-148`).
- The archive readers turn every malformed input into an `io::Error`, never a panic
  (`corpus/archive.rs:5-6`).
- The fuzz targets panic on a broken invariant by design, and are outside the workspace lints
  (`fuzz/Cargo.toml` has no `[lints]` table).

## Verification

- Unit tests of the corpus tooling in `xtask/src/corpus/` (manifest policy, archive readers,
  fetch), and `corpus/language_coverage.py`'s own unittest.
- The per-layer corpus checks run in the `Test Assets (linux)` job of `ci.yml`.
- Each fuzz target is run nightly for 10 minutes (`fuzz.yml`); a crash found becomes a
  regression test in the crate (`fuzz/README.md`).
- `qs-tests` runs in every `cargo test`, so the spec-citation ratchet gates every pull
  request.

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Corpus storage | Fetch by commit and SHA-256 into a cache outside the tree | Vendor the corpus | LIC-7; `corpus/README.md` Policy |
| QuakeC 1.06 and id's paks | `never` entries, blocked URLs | Fetch at test time | LIC-4, LIC-5 |
| License reading | Conservative `-only` when no "or later" grant | Assume "or later" | `corpus/README.md` Policy |
| Archive formats | Hand-written tar.gz and zip readers, `miniz_oxide` for inflate | A tar or zip crate | [inferred] minimal dependencies (CLAUDE.md, Dependencies) |
| Hashing | Hand-written SHA-256 in `xtask/src/oracles/sha256.rs` | A hashing crate | `sha256.rs:5-6`: the workspace has no hashing crate |
| Fuzz package | Outside the workspace, own lock, no lints | Workspace member | [inferred] cargo-fuzz needs nightly and libfuzzer-sys |
| Cross-crate tests | One binary, `qs-tests/tests/it`, modules per area | A file per test under `tests/` | CONV-11; CLAUDE.md Testing |
| Citation detection | Textual: a line starting `// spec ` in any `.rs` under the citing trees | Only inside `#[cfg(test)]` | `spec_coverage.rs:135-157` [inferred: simplicity] |
| Fuzz duration in CI | 10 minutes per target nightly; 24 h recorded as a known gap | 24 h runs | `docs/gates/p1.md:83, 90` |

## Open questions

### Resolved
1. The 24-hour TEST-5 run is a recorded, non-blocking gap carried by each readiness report
   (`docs/gates/p1.md:90`).

### Deferred
1. **TEST-5 not met as written.** No 24-hour fuzz run has happened; the nightly job runs 10
   minutes per target (`docs/gates/p1.md:90`; `fuzz.yml`).
2. **Citations outside tests count.** `spec_coverage.rs:137-157` counts any `// spec` line,
   and such lines exist in non-test code (`qs-differential/src/scenario.rs:670, 688`, qc-vm
   `ops.rs`, qc-edict `store.rs:296`, qc-builtins `dispatch.rs:251`), so a statement can be
   "cited" without a test.
3. **Cache-in-tree check is lexical.** `corpus.rs:176-184` joins the cwd and tests
   `starts_with(root)` without canonicalisation; `..` or symlinks are not resolved (LIC-7).
4. **Partial writes.** `corpus/fetch.rs:195-202` checks each path with `safe_member` just
   before writing it, so earlier files are on disk when a later unsafe path aborts.
5. **gzip CRC not checked.** `corpus/archive.rs:72-89` checks ISIZE only; integrity rests on
   per-file SHA-256 pins. No limit on total decompressed size for tar.gz
   (`archive.rs:180-183`).
6. **Name-keyed behaviour.** `sema-corpus` and `fteqcc-profile` pick the dialect and
   autoprototyping from the directory name (`sema_corpus.rs:98-103`,
   `fteqcc_profile.rs:78-81`).
7. **sema-corpus differences per module.** `used < listed.len()` is checked per module
   (`sema_corpus.rs:375-381`), so a multi-module codebase with a difference listed for one
   module fails on the others. `render`, `differential` and `listed_differences` have no unit
   tests (`sema_corpus.rs:421-472`).
8. **Not run in CI.** `fteqcc-coverage` and `fteqcc-full-coverage` appear in no workflow.
   `fteqcc_full_coverage.rs:23-25` says the FTEQCC profile does not exist yet; it does.
9. **Precache scanner.** `corpus/assets.rs:260-370` is its own byte scanner, not `qc-lexer`,
   and scans `.qc` only, not `.qh` (`assets.rs:456`); scenario-content problems still print
   the asset as fetched (`assets.rs:60-70`).
10. **lex-corpus scope.** Only the top level is lexed (`lex_corpus.rs:21-26`);
    `progs_src_names` ignores `/* */` comments and `#` lines (`lex_corpus.rs:62-72`).
11. **Fuzz target gaps.** `qc_vm` skips a non-zero oracle exit rather than flagging it
    (`fuzz/fuzz_targets/qc_vm.rs:121-127`, citing DIFF-10.1); the `qc-sema` incremental check
    edits only by copying the first file's text (`qc_sema.rs:85-92`), weaker than the README
    claims; `xtask_vectors` includes xtask's private module by `#[path]`
    (`xtask_vectors.rs:15-17`).
12. **README drift.** `crates/qs-tests/README.md:10-14` lists 3 of 8 modules; `fuzz/README.md`
    and `qc_vm.rs:22-23` describe regression handling differently; `xtask/README.md:9-24` omits
    most corpus commands.
13. **Duplication.** Five recursive directory walkers (`parse_corpus.rs:50`,
    `fteqcc_coverage.rs:846`, `fteqcc_full_coverage.rs:294`, `map_corpus.rs:19`,
    `project_corpus/mod.rs:38`); two `in_spec`; `analyze` in `sema_corpus.rs:140` and
    `fteqcc_profile.rs:105`; an xxd decoder in `qs-tests/map_entities.rs`,
    `fuzz/.../qs_bsp.rs` and `qs_mdl.rs`; a second VM vector parser in `spec_vectors.rs`.
14. **Test scratch locations.** Corpus tests use `std::env::temp_dir()` (`corpus.rs:250-259`,
    `fetch/tests.rs:65-86`) instead of `target/xtask-tests` through `test_dirs`.

## References

- `corpus/README.md`, `corpus/manifest.toml`, `docs/test-assets.md`, `fuzz/README.md`,
  `crates/qs-tests/README.md`, `xtask/README.md`
- `docs/testing.md` (TEST-3, TEST-5, Layer notes), `docs/spec/README.md`,
  `docs/spec/coverage-baseline.tsv`, `docs/spec/requirements.tsv`
- Requirements: LIC-4, LIC-5, LIC-7, QA-2, QA-4, FND-9, FND-10, TEST-3, TEST-5
- Plan: P1-06, P1-14, P1-15, P1-20 to P1-23, P1-33, P2-01, P2-25, P2-33, P2-51, P2-54,
  P5-02
