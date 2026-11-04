# Readiness: P2 L1 VM qc-builtins re-review after P2-150 (P2-R1B)

- **Issue:** #676 (`P2-R1B`)
- **Reviewer:** an independent reviewer agent session (TEST-9); did not write qc-builtins,
  #654, #659, #680, #682, #702 or #703, nor the first round of this review
- **Date:** 2026-11-04
- **Commit measured:** `551b1fc6` (`docs/676-p2-r1b-rereview`: `main` at `cf169599` with this
  round's tests)
- **Verdict:** Not ready. Every number meets the bar and the minimums, and the two defects of
  the first round are fixed, but this round found P2-168: vkQuake's `stof` (C's `atof`) gives
  NaN, or the wrong float, for a hexadecimal number that C rounds correctly. It blocks
  readiness until it is fixed and re-reviewed (TEST-9).

The first round (merged in #698, `c52883e3`; its report is this file at that commit) measured
`391e6106` and found P2-159 (#699, no fuzzing or property test of the readers of outside text)
and P2-160 (#700, BUILTIN-18 unclaimed in the README, the `Cvars` rustdoc detached). #702 closed
#700 and #703 closed #699. Neither changed the crate's shipped code: #702 edits the README and
moves two `use` lines in `mock.rs`, and #703 adds `src/properties.rs` (test only), a proptest
dev-dependency and the `qc-builtins` fuzz target. This round measured the crate again because
the README's new BUILTIN-18 claim changes the spec count.

## 1. Crates and spec sections

| Crate | Layer | Spec sections (crate README) |
| --- | --- | --- |
| `qc-builtins` | L1 | BUILTIN-1 to BUILTIN-16 and BUILTIN-18 in `builtins.md`, RRE-1 in `engine/rerelease.md` less RRE-1.10 (on the `**Not this crate's:**` line, `qs-vfs`'s), DIFF-6 and DIFF-7 in `differential.md` |

#700 is met: the README claims BUILTIN-18 and describes each module, RRE-1.10 is on a
`**Not this crate's:**` line, and the `/// The settings the mock keeps ...` rustdoc sits on
`pub struct Cvars` again (`crates/qc-builtins/src/mock.rs`).

## 2. Coverage, mutation score and spec citations

Readiness workflow run:
[41906216139](https://github.com/quakecstudio/editor/actions/runs/41906216139)
(`crates="qc-builtins"`, on the commit above; every job green, `ratchet: 7 checks passed`).

| Crate | Lines (bar 90%) | Branches (bar 80%) | Mutation (bar 90%) | Spec citations (bar 100%) |
| --- | --- | --- | --- | --- |
| `qc-builtins` | 98.2% (3277 of 3336) [min 94] | 94.4% (584 of 618) [min 84] | 98.5% (1326 of 1345) [min 97] | 100.0% (120 of 120) |

The spec number now counts BUILTIN-18: 120 statements against the first round's 97.

Surviving mutants (TEST-2): 19 over the four shards (4, 9, 3 and 3), from the run's
`measured-mutation-*` artifacts. They are the same 19 as in the first round, exactly the 16
keys under `[crates.qc-builtins].equivalent_mutants` (three keys name two mutants each), so
`ratchets.toml` holds only mutants cargo-mutants still produces. The shipped code is unchanged
since the first round, so no new mutant exists.

| Crate | Mutant | Outcome |
| --- | --- | --- |
| `qc-builtins` | `checked_field`'s guard and `<`, `rint`'s `>`, `component_5_1`'s `<`, the directive reader's three `<`, `wide_bits`' `\|`, `to_u64`'s `<` and `^`, `integer`'s `<`, `strconv`'s `<`, `atof` `decimal`'s two `>`, `b_substring`'s `\|\|`, `b_strncmp`'s `!=`, `>` (to `>=`) and `>` (to `==`), `b_strstrofs`'s `<` | equivalent, each with its reason in `ratchets.toml`, as the first round checked |

## 3. Property, fuzz and differential results

| Crate | Property tests (TEST-4) | Fuzzing (TEST-5) | Differential (TEST-6) |
| --- | --- | --- | --- |
| `qc-builtins` | `src/properties.rs`: seven properties, one per invariant the README's Properties section lists (RRE-1.7 twice, RRE-1.9, BUILTIN-18.23, BUILTIN-18.24, BUILTIN-18.8, BUILTIN-18.19). The `atof` property covers decimal input only, which is how P2-168 went unseen | `fuzz/fuzz_targets/qc_builtins.rs`: the localization file and `PF_VarString`, both tokenizers, `sprintf` and `stof` through `MockBuiltins` on the vkQuake profile, checking no panic and the stated bounds. Fuzz run [41880839404](https://github.com/quakecstudio/editor/actions/runs/41880839404) on #703's branch (`6688ced1`: the target as merged, the crate less #702's README and `use` lines): 659667 runs in 601 s from an empty corpus, no crash; every other target and the oracle fuzzing also passed. No scheduled Fuzz run on `main` has run since #703 merged. The target checks bounds, not values, so it cannot see P2-168 | id VM oracle: the `Differential` job of run [41836249897](https://github.com/quakecstudio/editor/actions/runs/41836249897) at `bdae8fea`, whose `qc-builtins`, `qc-vm` and `qs-differential` sources equal the measured commit's less this review's tests, passed. The id oracle runs the id profile only; nothing differential checks the vkQuake builtins or the localization (no vkQuake or QuakeSpasm oracle, ADR-0005), so the C was read by hand |

## 4. Flakes

20 consecutive runs of the crate's tests (TEST-7), from the workflow run above:

| Crate | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `qc-builtins` | 20 of 20 | 20 of 20 | 20 of 20 |

No test waits on the wall clock: the tests added here are pure functions of their input, and
`strftime` runs at the mock's fixed instant (BUILTIN-18.7).

## 5. Independent review (TEST-9)

- **Reviewer and time spent:** an independent agent session, one working session. It read the
  diffs of #702 and #703, the fuzz target and the properties against the statements they cite,
  and read again, with the C open, QuakeSpasm's `LOC_LoadFile` and `LOC_GetRawString`
  (`Quake/common.c` at `c9c8ee98`) and vkQuake's `tokenizeqc`, `PF_tokenizebyseparator`,
  `PF_ArgV` (`Quake/pr_ext.c` at `27789d73`) and `COM_ParseEx` (`Quake/common.c` at
  `27789d73`). It then tried `atof` against a correctly rounded reference (Python's
  `float.fromhex`, and a simulation of `hexadecimal`'s per-digit `mul_add` over 200000 random
  mantissas).
- **Tests added** (seven, all passing on the code as it is):
  - `crates/qc-builtins/src/localization/tests.rs`:
    `rereview_a_quote_or_an_equals_inside_a_value` (RRE-1.7: an unescaped quote ends an
    unquoted value with no trim; a later `=` is the value's),
    `rereview_a_vertical_tab_starts_the_key` (RRE-1.7: only blanks start a line, `q_isspace`
    trims before the `=`), `rereview_an_empty_value_and_an_unclosed_quote` (RRE-1.7: white space
    to the line's end, an unclosed quote trimmed as an unquoted value, ending the file).
  - `crates/qc-builtins/src/tokenize.rs`: `rereview_a_comment_is_part_of_the_next_word`
    (BUILTIN-18.23: a word's start is before the comment `COM_Parse` skips; an unclosed block
    comment), `rereview_a_quoted_word_at_the_com_token_limit` (BUILTIN-18.23: 4095 bytes fit,
    4096 end the list; an unclosed quote), `rereview_split_at_a_separator_as_long_as_the_text`
    (BUILTIN-18.24).
  - `crates/qc-builtins/src/tests.rs`: `rereview_atof_hexadecimal_edges` (BUILTIN-18.19: `0x`
    with no digit, `-0x.p1`, case, a fraction, leading white space).
- **Defects found:**
  - P2-168: `atof::hexadecimal` accumulates every hex digit into one f64, rounding after each
    (`value.mul_add(16.0, digit)`), and scales once at the end (`value *
    2.0_f64.powi(scale)`). C99 (N1256) 7.20.1.3 paragraph 5: "If the subject sequence has the
    hexadecimal form and FLT_RADIX is a power of 2, the value resulting from the conversion is
    correctly rounded." On main at `cf169599`: `0x1.` then 300 `0` digits then `p0` gives NaN
    (C: 1.0), as do `0x1`, 300 zeros, `p-1200` (C: 1.0) and `0x1`, 256 zeros, `p-1100` (C:
    2^-76); and `0x1.000001000000081` reads as the f64 `0x1.0000010000000p+0`, whose float is
    1.0, where the correctly rounded `0x1.0000010000001p+0` gives the float
    `0x1.000002p+0`, so `stof` returns the wrong float. BUILTIN-18.19 says only "a hexadecimal
    one is exact up to 13 digits" and records no divergence past it. This blocks readiness.

  No other behaviour defect was found: every case the new tests try matches the C at the
  pinned commits.

## 6. Ratchets

No change in this round. The measured numbers (lines 98, branches 94, mutation 98, spec 100)
are above the minimums `ratchets.toml` holds under `P2-R1B`, and its 16 equivalent mutants are
exactly the survivors. The re-review that passes, after P2-168, commits the values the run
printed (or that run's own, if P2-168 changes them):

```toml
[crates.qc-builtins]
readiness = "P2-R1B"
lines = 98
branches = 94
mutation = 98
spec = 100
```

## 7. Known gaps

| Gap | Issue | Why it does not block |
| --- | --- | --- |
| A hexadecimal `stof` is not correctly rounded and can be NaN | P2-168 | it blocks: the review stays open until it is fixed |
| TEST-5's 24 hours: the nightly Fuzz workflow gives each target 600 s, and #699's acceptance asked for 24 hours "or the sum of its nightly runs" | none | the same standing as every earlier review (`p2-r1.md`, `p2-r5.md`): each nightly run gives every target 600 s |
| No oracle for the vkQuake builtins or the localization | none | ADR-0005 keeps id's engine the oracle; QuakeSpasm and vkQuake are ported from, not run (ADR-0018). The ports were read against the C by two independent reviews |
