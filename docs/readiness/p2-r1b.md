# Readiness: P2 L1 VM qc-builtins re-review after P2-150 (P2-R1B)

- **Issue:** #659, under the re-review key `P2-R1B` (created by `plan/apply.py` when #659's
  pull request merges)
- **Reviewer:** the agent session that wrote #659's tests, measuring; the independent review
  (TEST-9) is `P2-R1B`'s own acceptance and stays open for a reviewer who did not write them
- **Date:** 2026-11-03
- **Commit measured:** `40f17a2a45649db058646d3d67204a66408b4a90` (`fix/659-rebless-builtins`, `main` after #672 with #659's
  tests)
- **Verdict:** Ready, pending the independent review

P2-150 (#654) added vkQuake's extension builtins to `qc-builtins` (BUILTIN-18): the `sprintf`
directive reader and conversions, C's `atof`, `strconv`, the C string comparisons and the
string, client and info builtins the compiler differential's mods call. #654 also changed the
crate's `equivalent_mutants` under the P2-R1 key, which `cargo xtask ratchet` refuses in the
file's history, so no Readiness run could pass after it. The P2-R1 minimums (lines 94,
branches 84, mutation 97, spec 100) were not re-measured then; a local run of cargo-mutants
27.1.0 on `main` at #672 found 86 survivors, mutation 92.6% (1072 of 1158), below the 97%
minimum.

## 1. Crates and spec sections

| Crate | Layer | Spec sections (crate README) |
| --- | --- | --- |
| `qc-builtins` | L1 | BUILTIN-1 to BUILTIN-18 in `builtins.md`; DIFF-6 and DIFF-7 in `differential.md` |

## 2. Coverage, mutation score and spec citations

Readiness workflow run: [41574437732](https://github.com/quakecstudio/editor/actions/runs/41574437732) (`crates="qc-builtins"`, on the commit above).

| Crate | Lines (bar 90%) | Branches (bar 80%) | Mutation (bar 90%) | Spec citations (bar 100%) |
| --- | --- | --- | --- | --- |
| `qc-builtins` | 97.6% (2592 of 2655) [min 94] | 92.7% (423 of 456) [min 84] | 98.1% (1137 of 1158) [min 97] | 100.0% (89 of 89) |

Local measurement before the run, `cargo mutants -p qc-builtins --profile mutants --test-tool
nextest` (cargo-mutants 27.1.0, the version the workflow pins): 1179 mutants, 1131 caught,
5 timeouts (infinite loops in the C string readers, counted as caught), 21 unviable, 22
missed, then one more killed by `the_reader_s_corners`' `%v` cases and two by
`string_comparisons`' empty needle: 19 survivors, 98.4% (1139 of 1158), to be confirmed by the
next Readiness run.

Surviving mutants (TEST-2), by file:

| Crate | Mutants | Outcome |
| --- | --- | --- |
| `qc-builtins` | `src/printf.rs`: the directive reader's `0` width, `0` flag, `h`, `q` and C length letters, `%p` at `*` width 0, a precision of 0 on a non-zero, `%S`'s `\r`, `%v` against `%V`, `wide_bits`' two words, `to_u64` above 2^63, `Out::has_room` for `%%` and plain bytes | killed by `printf::tests::the_reader_s_corners` and `the_buffer_s_end_drops_every_kind_of_byte` |
| `qc-builtins` | `src/dispatch.rs`: `ParmArgs`' count, words and strings, the `sprintf` dispatch (number 627), `infokey`'s `protocol`, `topcolor`, `bottomcolor`, `csqcactive` and `*VIP`, `min`, `max` and `bound` at equal values, `clienttype` of a slot the mock lacks, `substring` from 0, `strncmp`'s negative offset, and the parameters `strncmp` and `strstrofs` read only when passed | killed by `bodies::sprintf_reads_its_parameters`, `infokey_answers_fixed_keys`, `min_max_and_bound`, `clienttype_of_slots`, `strlen_strcat_and_substring` and `string_comparisons` |
| `qc-builtins` | `src/atof.rs`: the `+` sign and the hexadecimal exponent's sign; `src/cstring.rs`: `q_strncasecmp`'s end and `n`; `src/strconv.rs`: every row of the digit, letter and punctuation tables and the control-byte guard | killed by `atof_signs`, `strncasecmp_stops_at_a_difference_an_end_or_n` and `strconv_reaches_every_row` |
| `qc-builtins` | `src/dispatch.rs`: `strstrofs`'s `first > len` at and past the length, which this note's review found listed as equivalent and is not for an empty needle | killed by the empty-needle cases of `string_comparisons` |
| `qc-builtins` | the 19 left (16 keys, three shared by mutants of one name): `checked_field`'s bound, `rint` and `vtos` at zero and infinity (recorded since P1-R1), the directive reader's three `<` at 0, `wide_bits`' `\|` on disjoint bits, `to_u64` at exactly 2^63 and its top bit, `integer`'s padding at equality, `strconv`'s half at byte 128, `atof`'s digit guards, the `substring` and `strncmp` bounds whose equal or out-of-range case reads the same empty tail, and `strstrofs`'s `< 0` after `!= 0` | equivalent, each with its reason under `[crates.qc-builtins].equivalent_mutants`; the nine P1-R1 keys that cargo-mutants no longer produces on the P2-150 code are removed |

## 3. Property, fuzz and differential results

Unchanged from P2-R1 (`docs/readiness/p2-r1.md`, section 3): the crate is exercised through
the `qc-vm` fuzz target's mock engine and the differential suite's (DIFF-6, DIFF-7), and the
nightly Differential run holds the mods that call the new builtins to zero divergences
(DIFF-12.19).

## 4. Flakes

20 consecutive runs of the crate's tests (TEST-7), from the workflow run above:

| Crate | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `qc-builtins` | 20 of 20 | 20 of 20 | 20 of 20 |

No test waits on the wall clock: `strftime` runs at a fixed instant (BUILTIN-18.7) and the
rest are pure.

## 5. Independent review (TEST-9)

- **Reviewer and time spent:** pending under `P2-R1B`; the tests below were written by the
  measuring session
- **Tests added:** `printf::tests::the_reader_s_corners`,
  `printf::tests::the_buffer_s_end_drops_every_kind_of_byte`, `tests::atof_signs`,
  `tests::strconv_reaches_every_row`, `tests::strncasecmp_stops_at_a_difference_an_end_or_n`,
  `tests::bodies::sprintf_reads_its_parameters`, and new cases in
  `infokey_answers_fixed_keys`, `min_max_and_bound`, `clienttype_of_slots`,
  `strlen_strcat_and_substring` and `string_comparisons` (`crates/qc-builtins/src/printf.rs`,
  `crates/qc-builtins/src/tests.rs`)
- **Defects found:** none in the builtins; one in the process: a change to a reviewed crate's
  ratchet table merged under its old key (#654), which `cargo xtask ratchet` now settles once a
  later version names the re-review key (`xtask/src/ratchet.rs`, `history_problems`)

## 6. Ratchets

The minimums stand; `ratchets.toml` names this review:

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
| The independent review of the tests (TEST-9) | `P2-R1B` | the measurement is a workflow run anyone can repeat; the review reads the tests and the equivalence reasons |
