# Testing

Read before writing tests or building on a lower layer. The bar is
[`docs/testing.md`](../../docs/testing.md); the conventions are in
[`docs/conventions.md`](../../docs/conventions.md#tests-conv-11).

## Where tests go

- A test of one crate lives in that crate: inline `#[cfg(test)] mod tests` until about 500
  lines, then a sibling `foo/tests.rs`.
- A test that needs several crates goes in the single integration binary,
  `crates/qs-tests/tests/it/`, as a new module declared in `main.rs`. Never add a file
  directly under a `tests/` directory: each one links another binary.
- Test modules keep the workspace lints and opt out locally with
  `#![expect(clippy::unwrap_used, ..., reason = "test code: ...")]`.

## How tests are written

- **Cite the statement a test pins**: `// spec VM-12.3: <paraphrase>` on the line above the
  test. The spec-citation ratchet in `crates/qs-tests/tests/it/spec_coverage.rs` fails when a
  statement in `docs/spec/` has no citing test, and the baseline in
  `docs/spec/coverage-baseline.tsv` only shrinks.
- **Fixtures come from factories** taking a spec struct whose `Default` is the ordinary case;
  a test spells out only the axis it is about. A missing field is added to the spec struct,
  not worked around.
- **No wall-clock waits.** Code takes an injected clock; the preview host has a fixed-timestep
  headless mode. A `sleep` standing in for a clock is a flaky test.
- **Edges and errors, not just the happy path**: empty input, each limit at, below and above,
  malformed data, every error the code returns.
- Property tests (proptest) for every encoder and decoder and every stated invariant;
  differential tests wherever an oracle exists.
- **Break the code and watch the test fail** before trusting it; cargo-mutants does this
  systematically (`make test-mutants`).

## Readiness

No work may build on a layer until that layer's suite passes the bar in `docs/testing.md`
(coverage, mutation score, 100% spec citation, fuzzing, differential, no flakes, an
independent adversarial review). Readiness issues (`type:readiness`) are reviewed with
`/readiness-review` by an agent that did not write the code. Once passed, the numbers become
CI ratchets: never lower one to get green. A lower-layer defect found from above reopens
that layer's readiness.
