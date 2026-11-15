# Performance audit

Each crate held to one checklist, one crate or small group at a time (#966, PRIN-5, TEST-8), as
the CONV-17 audit held them to the type-design rules. A change is kept only with a measurement
that shows it: a `qs-bench` budget, a criterion benchmark or a dhat allocation count, before and
after. No change alters output: each crate's tests, differentials and snapshots pass unchanged.
The P7-04 pass on `qs-map-vis` and `qs-map-light` is the worked example: Cow windings, pooled
sets and a `thread_local` stack took `lq-e1m1`'s full compile from 287 s to 6.16 s.

## The checklist

- Needless `clone` and `to_owned`, and an owned value taken where a borrow would do.
- Collections grown without `with_capacity` where the size is known.
- `collect` into a collection only to iterate it again.
- `format!` and `String` building on per-frame or per-statement paths.
- `Box` and `Arc` churn in loops.
- The same hashing or lookup repeated in a loop.
- Quadratic scans where an index or a sort would do.
- Work done every egui frame that could be cached until its input changes.
- The clippy lints below.

## The clippy lints

Turned on, `deny` in `[workspace.lints.clippy]` (`docs/conventions.md`, Performance), because
the workspace holds none: `implicit_clone`, `inefficient_to_string`,
`iter_on_empty_collections`, `iter_on_single_items`, `iter_with_drain`,
`large_types_passed_by_value`, `manual_string_new`, `needless_for_each`,
`set_contains_or_insert`, `stable_sort_primitive`, `unnecessary_box_returns` and
`unnecessary_join`. The ones clippy enables by default (`vec_box`, `box_collection`,
`useless_vec`, `unnecessary_to_owned`, `iter_overeager_cloned`, `redundant_allocation`,
`large_enum_variant`, `ptr_arg`, `manual_memcpy` and the rest of `clippy::perf`) were already
held at zero by `-D warnings`.

Left, each turned on once the crates it is found in are audited. Counted on 2026-11-15 at
`main` (`a4c3c23c`'s base), every target and feature, each site once:

| Lint | Sites | Group |
| --- | --- | --- |
| `format_push_string` | 156 | restriction |
| `or_fun_call` | 74 | nursery |
| `redundant_clone` | 62 | nursery |
| `needless_pass_by_value` | 37 | pedantic |
| `format_collect` | 25 | pedantic |
| `string_add` | 19 | restriction |
| `trivially_copy_pass_by_ref` | 18 | pedantic |
| `needless_collect` | 10 | nursery |
| `naive_bytecount` | 10 | pedantic |
| `assigning_clones` | 8 | pedantic |

`naive_bytecount` asks for the `bytecount` crate, which this audit will not add for counting
lines; its sites are kept unless a measurement says otherwise. `large_include_file` is left off:
its one site is the CJK font the app embeds (`apps/desktop/src/theme.rs`).

The count is taken with:

```bash
cargo clippy --locked --workspace --all-targets --all-features --message-format=json -- \
  -W clippy::format_push_string -W clippy::or_fun_call -W clippy::redundant_clone ...
```

## The crates

In the order of expected payoff. Each section lists what the checklist found, what changed with
its before and after numbers, and what was left with the reason.

1. The VM and the compiler: `qc-vm`, `qc-edict`, `qc-builtins`, `qc-codegen` and the front end.
2. The map tools: `qs-map-bsp`, `qs-map-vis`, `qs-map-light`.
3. The editor view and the renderer: `qs-editor-view`, `qs-render`, `apps/desktop`.
4. The simulation: `qs-sim`, `qs-client`.
5. The rest.

No crate has been audited yet; the first pass turned on the lints above and fixed the few sites
they found: two in shipped code (`qs-map-bsp`'s T-junction fan consumes its triangles instead
of draining them; the preview host clones its progs without the slice copy) and the rest in
tests and `xtask`.
