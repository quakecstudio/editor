# Readiness: P2 L1 VM qc-builtins re-review after P2-150 (P2-R1B)

- **Issue:** #676 (`P2-R1B`)
- **Reviewer:** an independent reviewer agent session (TEST-9); did not write qc-builtins, #654,
  #659, #680 or #682
- **Date:** 2026-11-03
- **Commit measured:** `391e6106afa47071e79092c45a97b5d126c996ea` (`docs/676-p2-r1b-review`:
  `main` at `ce1d22e2` with this review's tests)
- **Verdict:** Not ready. Every number meets the bar and the minimums, but the review found two
  defects, P2-159 (no fuzzing of the parsers #680 and #682 added, TEST-5) and P2-160 (the
  README does not claim BUILTIN-18, so TEST-3 does not count it, and `mock::Cvars` lost its
  rustdoc). Each blocks readiness until it is fixed and re-reviewed (TEST-9).

#659 measured the crate on `40f17a2a` (Readiness run
[41574437732](https://github.com/quakecstudio/editor/actions/runs/41574437732)). Since then
three merges changed `qc-builtins`: #680 added `tokenize.rs` (vkQuake's `tokenize`,
`tokenizebyseparator`, `argv` and their index builtins, BUILTIN-18.23, BUILTIN-18.24) and the
`Unimplemented` choice, #682 added `localization.rs` (QuakeSpasm's `LOC_LoadFile`,
`LOC_GetRawString`, `LOC_Format` and `PF_VarString`, RRE-1) and the localized print and
`WriteString` paths, and #677 added `math::vector_normalize`. About 1250 lines changed, so this
review measured again.

## 1. Crates and spec sections

| Crate | Layer | Spec sections (crate README) |
| --- | --- | --- |
| `qc-builtins` | L1 | BUILTIN-1 to BUILTIN-16 in `builtins.md`, RRE-1 in `engine/rerelease.md`, DIFF-6 and DIFF-7 in `differential.md`. BUILTIN-18, which the crate implements, is not on the README's line (P2-160) |

## 2. Coverage, mutation score and spec citations

Readiness workflow run:
[41771811674](https://github.com/quakecstudio/editor/actions/runs/41771811674)
(`crates="qc-builtins"`, on the commit above; `ratchet: 7 checks passed`).

| Crate | Lines (bar 90%) | Branches (bar 80%) | Mutation (bar 90%) | Spec citations (bar 100%) |
| --- | --- | --- | --- | --- |
| `qc-builtins` | 98.0% (3190 of 3254) [min 94] | 93.9% (575 of 612) [min 84] | 98.5% (1326 of 1345) [min 97] | 100.0% (97 of 97) |

The spec number counts BUILTIN-1 to BUILTIN-16, RRE-1 and DIFF-6, DIFF-7 only. Every one of
BUILTIN-18's 24 statements is cited by a `qc-builtins` test (checked by hand with `rg` over
`crates/qc-builtins/src`), but the measurement does not count them (P2-160).

Surviving mutants (TEST-2): 19, over the four shards (4, 9, 3 and 3). They are exactly the 16
keys under `[crates.qc-builtins].equivalent_mutants` (three keys name two mutants each), so
`ratchets.toml` holds only mutants cargo-mutants still produces. The localization, tokenizer
and `vector_normalize` code added since #659 left no survivor.

| Crate | Mutant | Outcome |
| --- | --- | --- |
| `qc-builtins` | `checked_field`'s guard and `<`, `rint`'s `>`, `component_5_1`'s `<`, the directive reader's three `<`, `wide_bits`' `\|`, `to_u64`'s `<` and `^`, `integer`'s `<`, `strconv`'s `<`, `atof` `decimal`'s two `>`, `b_substring`'s `\|\|`, `b_strncmp`'s `!=`, `>` (to `>=`) and `>` (to `==`), `b_strstrofs`'s `<` | equivalent, each with its reason in `ratchets.toml`. The reviewer checked the `b_strncmp` ones against vkQuake's `PF_strncmp` (`Quake/pr_ext.c` at `27789d73`): `if (aofs < 0 \|\| (aofs && aofs > (int)strlen (a))) aofs = strlen (a);`. With `!=` changed to `==` the clamp never runs for an offset past the end, and `a.get(from..)` gives the same empty tail |

## 3. Property, fuzz and differential results

| Crate | Property tests (TEST-4) | Fuzzing (TEST-5) | Differential (TEST-6) |
| --- | --- | --- | --- |
| `qc-builtins` | none in the crate. The README states no invariant, but the localization parser and the tokenizer are decoders with stated bounds (P2-159) | `fuzz/fuzz_targets/qc_vm.rs` reaches the id builtins through `qs-differential`'s mock (id profile). No target reaches `Localization::loc_load_file`, which parses a game file (`localization/loc_english.txt`), or the vkQuake tokenizer, `sprintf` reader or `atof`: **fails** (P2-159) | id VM oracle: the nightly Differential run [41635634171](https://github.com/quakecstudio/editor/actions/runs/41635634171) at `4a574967` passes its `Differential` job. That commit predates #680 and #682; the run's `Compiler differential (generated programs)` failure is qc-codegen's E0302 refusals, not this crate |

The last nightly Fuzz run, [41626179616](https://github.com/quakecstudio/editor/actions/runs/41626179616)
at `4a574967`, passed, also before #680 and #682.

## 4. Flakes

20 consecutive runs of the crate's tests (TEST-7), from the workflow run above:

| Crate | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `qc-builtins` | 20 of 20 | 20 of 20 | 20 of 20 |

No test waits on the wall clock. The tests added here are pure, and `strftime` runs at a fixed
instant (BUILTIN-18.7).

## 5. Independent review (TEST-9)

- **Reviewer and time spent:** an independent agent session, one working session. It read
  `localization.rs` against QuakeSpasm's `Quake/common.c` and `Quake/pr_cmds.c` at
  `c9c8ee98` (`COM_HashString`, `LOC_LoadFile`, `LOC_GetRawString`, `LOC_ParseArg`,
  `LOC_HasPlaceholders`, `LOC_Format`, `PF_GetStringArg`, `PF_VarString`), and `tokenize.rs`
  and the `strncmp` dispatch against vkQuake's `Quake/pr_ext.c` and `Quake/common.c` at
  `27789d73` (`tokenizeqc`, `PF_tokenizebyseparator`, `PF_ArgV`, `PF_argv_start_index`,
  `COM_ParseEx`, `PF_strncmp`). It also re-checked the equivalence reasons against the
  surviving mutants and checked the crate README and rustdoc against the definition of done.
- **Tests added** (seven, all passing on the code as it is):
  - `crates/qc-builtins/src/localization/tests.rs`:
    `reviewer_a_probe_wraps_past_the_last_slot` (RRE-1.2, two keys hashing to the last slot
    and a missing one), `reviewer_the_empty_key_and_a_doubled_dollar` (RRE-1.3: `$` alone,
    `$$k`), `reviewer_the_text_ends_at_its_first_zero` (RRE-1.7: a zero in the file, an `=` on
    a comment line, values that end the line or the file).
  - `crates/qc-builtins/src/tokenize.rs`: `reviewer_a_high_byte_before_a_word_is_its_start`
    (BUILTIN-18.23: tokenizeqc's unsigned skip against COM_Parse's signed one, quoted ends),
    `reviewer_split_with_an_empty_separator_and_at_the_cap` (BUILTIN-18.24: an empty
    separator, exactly 64 tokens, the first listed separator wins).
  - `crates/qc-builtins/src/tests.rs`: `reviewer_tokenizebyseparator_reads_seven_separators`
    (BUILTIN-18.24, every parameter slot a separator, none) and
    `reviewer_argv_truncates_its_float_index` (BUILTIN-18.23: -0.5, -1.5, NaN, 3e9 and
    out-of-range negatives through the dispatch).
- **Defects found:**
  - P2-159: the localization file parser and the vkQuake string builtins have no fuzz target
    and no property test (TEST-5, TEST-4). This blocks readiness.
  - P2-160: the README's `**Spec sections:**` line leaves out BUILTIN-18, so the per-crate
    TEST-3 measurement skips its 24 statements. The README also claims RRE-1.10, which qs-vfs
    implements, and its module list is out of date. #682 put `use` lines around the rustdoc
    of the public `mock::Cvars`, so that rustdoc now sits on a `use` item and `Cvars` has
    none. This blocks readiness.

  No behaviour defect was found: every case the new tests try matches the C at the pinned
  commits.

## 6. Ratchets

No change in this review. The measured numbers (lines 98, branches 93, mutation 98, spec 100)
are above the minimums `ratchets.toml` holds under `P2-R1B`, and its 16 equivalent mutants
are exactly the survivors. The minimums are raised by the re-review that passes, after
P2-159 and P2-160:

```toml
[crates.qc-builtins]
readiness = "P2-R1B"
lines = 94
branches = 84
mutation = 97
spec = 100
```

## 7. Known gaps

| Gap | Issue | Why it does not block |
| --- | --- | --- |
| No fuzz target or property test for the localization parser and the vkQuake string builtins | P2-159 | it blocks: the review stays open until it is fixed |
| BUILTIN-18 not claimed in the README; the `Cvars` rustdoc detached | P2-160 | it blocks: the review stays open until it is fixed |
