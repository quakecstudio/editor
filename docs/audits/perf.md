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

The first pass turned on the lints above and fixed the few sites they found: two in shipped
code (`qs-map-bsp`'s T-junction fan consumes its triangles instead of draining them; the preview
host clones its progs without the slice copy) and the rest in tests and `xtask`.

### The compiler: `qc-preproc`, `qc-sema`, `qc-codegen`

Measured with `id1_recompile`: one function of id 1.01 edited, then the module rebuilt through
the same database to progs bytes. Criterion, 10 samples, on the reference Mac (Apple M5),
2026-11-15. Profiled with `sample` over a build with line tables, keeping only the stacks under
the benchmark's own loop.

| Change | Before | After |
| --- | --- | --- |
| `qc-sema` `expand::render`: each token's database file is looked up once per source file, not by hashing its whole path once per token | 56.5 ms | 52.7 ms (-6.7%) |
| `qc-codegen` `Program::immediate`: an immediate's value is compared before its type, so the scan over every definition, id's `PR_ParseImmediate` order kept, rules most out with a word compare | 52.7 ms | 51.0 ms |
| Both | 56.5 ms | 51.0 ms (-9.8%), within the 200 ms budget |

No output changes: the `qc-preproc`, `qc-sema` and `qc-codegen` suites pass (1,468 tests), and
`cargo xtask compiler-diff --fast` reports 0 divergences on id 1.01.

Left, with the reason:

- **The module is resolved twice.** `compile_module` takes the analysis's diagnostics from
  `Db::analyze_module`, which resolves the module with its source map (FTEQCC's first pass, the
  autoproto plan, `extensions.qc`). `compile_lowered` then resolves it again with no source map,
  numbering files by their place in the module. Resolution is about a third of a recompile, but
  the two indexes differ on FTEQCC codebases, so sharing one is a change to the compiler's design,
  proved by the full compiler differential, not a tweak.
- **The scan for an equal immediate** is still linear in the definitions, as id's is (QC-20.2).
  An index would have to follow every definition whose initialised value or flags change after it
  is made, to keep returning id's first match. Before this change the scan was a fifth of a
  recompile, and the recompile is four times under its budget.
- **Each file's lowered declarations are cloned** for the code generator (`compile_module`,
  `decls.push(lowered.clone())`), about 3% of a recompile. Avoiding the clone means changing
  `qc_sema::resolve_module`'s public signature for that 3%.
- **The clippy lints still off** find 38 sites in the shipped code of `qc-preproc`, `qc-syntax`,
  `qc-sema` and `qc-codegen` (2026-11-15): 21 `or_fun_call` (most a span computed for a default,
  a few nanoseconds), 7 `needless_pass_by_value`, 5 `format_push_string` (`qc-codegen`'s
  `progdefs.h` writer and one in `qc-preproc`, once per compile), 3 `assigning_clones` and one
  each of `redundant_clone` and `needless_collect`. None is in the profile above; they are fixed
  when the lints are turned on for the whole workspace.

### The VM: `qc-progs`, `qc-vm`, `qc-edict`, `qc-builtins`, `qc-trace`

Measured with the engine routes, which play id 1.01's e1m1 in the release preview host
(`cargo xtask bench` with `QS_BENCH_GAME` naming a GOG install, reference Mac, 2026-11-15). Each
is far within its budget:

| Benchmark | Measured | Budget |
| --- | --- | --- |
| `quakec_e1m1`, the QuakeC of one server frame | 90.67 us p99 | 500 us |
| `server_frame_e1m1`, the whole server frame | 477.83 us p99 | 1 ms |
| `map_load_e1m1` | 17.01 ms median | 1 s |
| `save_to_game`, a QuakeC edit seen in the running game | 67.43 ms p95 | 1 s |

A `sample` of the release host playing e1m1 for 8 s finds it waiting on the display and the
event loop: the VM, the edicts and the builtins do not appear among its busy frames. With QuakeC
at a fifth of its budget and no hot spot to measure a change against, nothing was changed; the
P7-04 rule is a change only with a measurement that shows it.

Left, with the reason:

- **Each statement is decoded from the progs' bytes** as the run reaches it (`Progs::statements`,
  `Table::get` in `Machine::run_inner`), where id's VM reads a loaded array. It is the obvious
  candidate when a heavier route (the rerelease's mg1, `QS_BENCH_RERELEASE`) or a profile of a
  mod shows the VM; on e1m1 it does not register.
- **The clippy lints still off** find 18 sites in the shipped code of these crates (2026-11-15):
  12 `format_push_string`, 9 in `qc-trace`'s JSON writer (a trace, not a frame), 2 in `qc-edict`'s
  system definitions text (once per load) and 1 in `qc-builtins`' `printf`; 5
  `trivially_copy_pass_by_ref`, 4 in `printf`'s format spec and 1 in `qc-progs`' read options; and
  1 `needless_pass_by_value` in the mock engine. None is on a per-statement path; they are fixed
  when the lints are turned on for the whole workspace.
