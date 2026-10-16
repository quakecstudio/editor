# qs-bench

The performance-budget benchmarks: one criterion benchmark per budget in
[`docs/perf-budgets.md`](../../docs/perf-budgets.md) (PRIN-5, ENG-13, TEST-8).

- **Layer:** `tests` in `layers.toml` ([`docs/layers.md`](../../docs/layers.md)); it may depend on
  any crate it benchmarks, and nothing depends on it.
- **Spec sections:** none.
- **Shipped:** no.

The benchmarks are in `benches/budgets.rs`, in the criterion group `budgets`. A budget marked
`pending` has a placeholder; the issue named in its row replaces the placeholder with the real
measurement and marks the row `active` in the same PR.

Run them through xtask, which checks the budgets and records the JSON history:

```bash
cargo xtask bench            # or make bench
cargo xtask bench history
```
