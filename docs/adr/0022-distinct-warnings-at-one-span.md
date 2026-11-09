# ADR-0022: Give a directive's two different warnings at one place both

- **Status:** Accepted
- **Date:** 2026-11-09
- **Decided by:** Justin Plock
- **Issue:** [#763](https://github.com/quakecstudio/editor/issues/763)
- **Requirements:** TEST-5

## Question

When one directive gives two warnings of one code with different texts at the same span, are both reported, against QCD-1.5's rule of one diagnostic per code and span?

## Context

QCD-1.5 says the lexer, the preprocessor and the parser "report a diagnostic once: no two of the diagnostics they give for one input have the same code and the same span", with one exception, FTEQCC's `W0227` (FTEF-21.6). The qc-preproc fuzz target asserts exactly that. P3-39 (#762, PR #764) made `Run::report` drop a second diagnostic with one code and one span, and added `Run::report_also` to keep, pending this decision, two cases where FTEQCC gives two different warnings at one place:

- An `#append` whose parameters differ from the macro's: FTEF-14.58 gives "different number of macro arguments in macro append" or "arguments differ in macro append", then "Alternate precompiler definition of NAME (OLD -> NEW)", both `W0115` at the macro's name.
- A `#pragma flag` (and `keyword`, `optimise`, `opcode`) naming several flags: FTEF-12.11 gives a `W0113` for each NAME, all at the pragma's span, so two names with different problems are two texts at one place.

The fuzz target had not found either pair; nothing stopped it.

## Options

| Option | Consequences |
| --- | --- |
| A. Both are given, as named exceptions to the one-report-per-place rule, as `W0227` is | FTEQCC's output kept; QCD-1.5 is withdrawn for a statement naming the exceptions; the fuzz target allows exactly these; small change |
| B. Each warning at its own span: the parameter list or the `#append` word for the first `W0115`, each NAME's word for `W0113` | The most precise; FTEF-14.58 and FTEF-12.11 withdrawn and replaced; the pragma's words would need their source offsets kept, which the preprocessor's pragma text does not hold today |
| C. Only the first of each pair is given | The simplest; hides a warning FTEQCC prints, and FTEF-14.58 and FTEF-12.11 change |

## Decision

Option A, decided by Justin Plock on 2026-11-09 in the P3 session. When a directive gives a further warning of the same code at a span it has already warned at in the same run, with a different text, both are given: the second `W0115` of an `#append` that defines the macro anew (FTEF-14.58) and a pragma's `W0113` for each further NAME (FTEF-12.11). A repeat with the same text is still one report. Options B and C were not chosen: B costs the pragma offsets for no gain in what a modder sees, and C drops a warning FTEQCC gives.

## Consequences

- `docs/spec/language-diagnostics.md`: QCD-1.5 is withdrawn for QCD-1.6, which names `W0227`, these `W0115` and `W0113` cases as its exceptions; every citation of QCD-1.5 moves to QCD-1.6. FTEF-14.79's pointer to this decision is reworded to name QCD-1.6.
- `fuzz/fuzz_targets/qc_preproc.rs`: two diagnostics of one code and one span are allowed only for `W0113` and `W0115` with different texts.
- `crates/qc-preproc`: `Run::report_also` already gives these pairs (P3-39); its rustdoc cites QCD-1.6.
- Nothing in `plan/` beyond closing P3-40 (#763).

## Links

- Decision issue: [#763](https://github.com/quakecstudio/editor/issues/763) (P3-40), found in [#764](https://github.com/quakecstudio/editor/pull/764) for P3-39 ([#762](https://github.com/quakecstudio/editor/issues/762)).
- Related: ADR-0017 (the clean-port policy for FTEQCC's behaviour).
- Sources: `docs/spec/language-diagnostics.md` QCD-1.5; `docs/spec/dialect-fteqcc.md` FTEF-12.11, FTEF-14.58, FTEF-14.79, FTEF-21.6; `fuzz/fuzz_targets/qc_preproc.rs`; `crates/qc-preproc/src/lib.rs` `Run::report_also`.
