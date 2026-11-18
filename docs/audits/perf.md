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

Turned on, `deny` in `[workspace.lints.clippy]` (`docs/conventions.md`, Performance), with
the workspace holding none: `assigning_clones`, `implicit_clone`, `inefficient_to_string`,
`iter_on_empty_collections`, `iter_on_single_items`, `iter_with_drain`,
`large_types_passed_by_value`, `manual_string_new`, `needless_for_each`, `or_fun_call`,
`redundant_clone`, `set_contains_or_insert`, `stable_sort_primitive`,
`unnecessary_box_returns` and `unnecessary_join`. The ones clippy
enables by default (`vec_box`, `box_collection`, `useless_vec`, `unnecessary_to_owned`,
`iter_overeager_cloned`, `redundant_allocation`, `large_enum_variant`, `ptr_arg`,
`manual_memcpy` and the rest of `clippy::perf`) were already held at zero by `-D warnings`.

The second pass (2026-11-17) turned on `or_fun_call` (78 sites), `redundant_clone` (73) and
`assigning_clones` (8), fixing every site. One `or_fun_call` site is not made lazy: the status
bar's `panel_buttons` draws the buttons it reports on, so it is called whatever was asked
already (the code review of the pass found it; no test did).

Left off, each with the reason. Counted on 2026-11-17 at `17a063a0`, every target and feature,
each site once; "shipped" leaves out tests, benchmarks, examples, `qs-differential` and `xtask`:

| Lint | Sites | Shipped | Why it stays off |
| --- | --- | --- | --- |
| `format_push_string` | 158 | 49 | Its fix, `write!` into the `String`, returns a `Result` each site must discard. The shipped sites build a report, a trace or a file once (the agent's context, `qc-trace`'s JSON, `progdefs.h`, the portal file, an export's manifest), none per frame or per statement. |
| `needless_pass_by_value` | 39 | 28 | Its fix changes signatures, 10 of them the LSP server's request handlers, which take each request's parameters by value, as they are deserialised. No site is in a profile. |
| `format_collect` | 25 | 2 | 23 sites are tests building their input. The two shipped write an id as hex, in `qs-checkpoint` and `qs-templates`. |
| `string_add` | 19 | 0 | Every site is a test building its input. |
| `naive_bytecount` | 10 | 6 | It asks for the `bytecount` crate, which this audit will not add for counting lines. |
| `trivially_copy_pass_by_ref` | 8 | 8 | The eight left are option structs borrowed by their public methods (`qs-map-bsp`'s `Compiler`, `qs-map-vis`'s `Vis`, `qc-progs`' `ReadOptions`), so that an option added later changes no caller. The pass took the ten others by value. |
| `needless_collect` | 7 | 2 | Four of the seven start threads and collect their handles before joining the first, which the lint cannot see is needed; one reverses an iterator that cannot be walked backwards; and one collects into a set to drop duplicates, which the lint's own fix removed until a test caught it. Held, it would be mostly `#[expect]`. |

[`perf-baseline.tsv`](perf-baseline.tsv) holds the count of each of these, and it only
shrinks: `cargo xtask perf-baseline` runs clippy with the seven as warnings, counts each site
once, and fails when a lint has more sites than the file says; with fewer it passes and names
`cargo xtask perf-baseline --write`, which lowers a line and never raises one (a platform
compiles less of the workspace than the Mac the file was counted on). `cargo xtask ci` runs it
after its own clippy step, whose build it reuses (12 s on the reference Mac), and on GitHub it
runs in the Clippy job. It is a command of its own and not a part of
`cargo xtask conventions`, which the plan named, because that check reads the sources alone
and this one needs clippy to have built the workspace.

## The crates

In the order of expected payoff. Each section lists what the checklist found, what changed with
its before and after numbers, and what was left with the reason.

1. The VM and the compiler: `qc-vm`, `qc-edict`, `qc-builtins`, `qc-codegen` and the front end.
2. The map tools: `qs-map-bsp`, `qs-map-vis`, `qs-map-light`.
3. The editor view and the renderer: `qs-editor-view`, `qs-render`, `apps/desktop`.
4. The simulation: `qs-sim`, `qs-client`.
5. The rest.

The first pass turned on twelve lints and fixed the few sites they found: two in shipped code
(`qs-map-bsp`'s T-junction fan consumes its triangles instead of draining them; the preview host
clones its progs without the slice copy) and the rest in tests and `xtask`. The second pass
turned on three more across every crate, 159 sites, most a default computed only when it is
needed (`unwrap_or_else`) or a clone of a value not used again. The workspace's tests pass,
and each rewrite to a lazy default was read for an argument with an effect of its own; the one
found is kept eager, above.

After the second pass every budget holds on the reference Mac (`cargo xtask bench`, 2026-11-17,
`570f726a`, all 19 met). Against the gate's run of 2026-11-16 (`docs/gates/perf-mvp.md`) none is
slower by more than a tenth except `textured_view_e1m1`, 12.02 ms against 10.79 ms (11% more, a
quarter of its 50 ms budget). `open_project_and_type` and `keystroke` are 8% and 9% slower. The
pass does not touch those paths beyond lazy defaults, and the two runs are a day and a machine's
other work apart, so the difference is not attributed; the next scheduled run on the CI runner
is the 10% rule's judge. `map_load_mg1` is half the gate's figure, which three direct runs of
the host confirm (below); the gate's run is the outlier there. The numbers:
`full_compile_e1m1` 6.167 s against 6.028 s, `id1_recompile` 51.43 ms against 50.87 ms,
`open_project_and_type` 282.38 ms against 260.46 ms, `keystroke` 0.140 ms against 0.128 ms,
`map_load_mg1` 338.60 ms against 715.74 ms.

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

### The map editor's view: `apps/desktop` (`quakec-studio`)

Measured in the app itself, release build with line tables, on `lq_e2m5.map` (11,539 brushes,
every LibreQuake texture), Textured, the window on the 120 Hz panel, the 3D camera turned by a
temporary hook for 6.5 s with no input, and each frame's UI time logged (`App::ui`, which builds
the frame's shapes; the GPU draws after it). Reference Mac, 2026-11-15, two runs each.

`sample` over the turn put most of the main thread in `MapEditor::pane`'s own code and
`Projection::line`: every frame tested each of the scene's brush edges against the set of
entities drawn as faces, only to skip nearly all of them. The lines of entities without faces
(point entities' boxes, an external map's instance) are now kept with the scene when it is built,
and the textured and lit views draw those alone.

| Change | Before | After |
| --- | --- | --- |
| The textured view's frame: only the lines of entities without faces are visited | 2.84 ms median, 3.06 ms p99 | 1.52 ms median, 2.24 ms p99 (-46%) |

The app's 227 tests pass, its snapshots among them, so the view draws what it drew.

Tried and not kept: working out the camera's axes and the tangent of its field of view once per
frame, not once per point projected (`map_panes.rs`, `Projection::view` and `screen`). Over
three pairs of runs the median frame fell from 1.53 ms to 1.37 ms, but the 95th percentile rose
from 2.1 ms to 2.4 ms and the 99th from 2.2 ms to 2.7 ms, so it is not a measured win.

Left, with the reason: what remains of the frame is egui's tessellation of the lines still drawn
(point entities' boxes, the outlined brushes, links), one shape each, and their projection. Drawing
them as one mesh would take most of it, at the cost of the view's own line drawing; at 1.5 ms of
an 8.33 ms frame it waits for a map where it shows.

### The map compilers: `qs-map-bsp`, `qs-map-vis`, `qs-map-light`

The P7-04 pass is this group's audit (Cow windings, pooled sets, a `thread_local` stack: lq-e1m1's
full compile from 287 s to 6.16 s). A `sample` of `full_compile_e1m1` over 60 s with line tables
(2026-11-15, 6.04 s median against the 8 s budget) finds the time where the algorithms put it:
light's `Tree::cast_ray` (21,209 samples of its own), vis's `Winding::clip` (12,479) and its
separating planes (12,129), and qbsp's tree growth across its threads. Allocation is about a
tenth of the samples, most of it in qbsp's `Builder::grow`, `Winding::clip` and
`Winding::clipped`, which already reserve their room and consume what they cut. Nothing was
changed: no single site is large enough to move the budget's median by a measurable step.

**The `Allocator` trait (Rust 1.100), evaluated 2026-11-17.** Tried: a per-thread arena for the
windings `qs-map-vis` cuts in a portal's flow, the allocation `Winding::clip` makes each time a
plane splits a winding and the largest single source of allocation in the compile after qbsp's
tree growth. The arena was one 4 MiB block per flow thread, handed out front to back and given
back as the flow's steps return, with the global allocator for what did not fit; each step's
windings became slices lent by a portal or an earlier step, or `Vec<_, &Arena>` cut in the
arena (`Vec::with_capacity_in`), in place of `Cow<Winding>`.

| `full_compile_e1m1`, criterion, 10 samples a run, reference Mac | Median | Least | Most |
| --- | --- | --- | --- |
| Without the arena, first run | 6.039 s | 5.989 s | 6.098 s |
| With the arena, first run | 5.942 s | 5.909 s | 6.026 s |
| Without the arena, second run | 6.011 s | 5.970 s | 6.120 s |
| With the arena, second run | 5.994 s | 5.858 s | 6.098 s |

Not kept. The two pairs of runs differ by 1.6% and 0.3%, and the runs' own spread is 2%, so it
is not a measured win. Adopting it would also cost more than it gives: an arena is an
`unsafe impl Allocator`, and the workspace denies `unsafe`; the one released arena crate in the
lockfile, `bumpalo` (3.20.3, and 3.20.4 of 2026-11-13), offers the trait only behind
`#![feature(allocator_api)]`, which does not build on stable 1.100 (`E0554`). The map compilers
(`qs-map-bsp`, `qs-map-light`) were not tried: their allocation is a smaller share of their time
than vis's, and the same two costs apply. It is worth trying again when a released arena
implements the stable trait and a profile shows allocation as more than a tenth of a compile.

### The benchmarks themselves

`check_after_typing` leaves its check thread working after it returns: its 220 pauses, made
instantly with a set clock, are checked in a few passes of the 50,000-line project, about 17 s
of one thread in the run sampled above, beside the benchmarks after it. Waiting for that thread
before returning changed none of them on the reference Mac (`id1_recompile` 50.66 ms against
50.45 ms, `full_compile_e1m1` 6.041 s against 6.045 s), which has the cores to spare, so the
benchmark is left as it is; a runner with fewer cores is where it would show first.

### The renderer: `qs-render`

Measured in the release preview host with line tables, unstripped, in `--benchmark` mode on
id1's e1m1: no vsync, the free camera turning, 14 s, its main thread sampled for 10 s by
`sample`. Reference Mac, 2026-11-16, three runs each.

Most of the host's own work in a frame was `Play::frame`, and most of that was its first line,
`MapMesh::relight_with`. Every frame, that line allocated a mark for each face and asked each
face whether a style it reads had changed. In e1m1 the styles change ten times a second, while
the host draws thousands of frames. `relight_with` now returns at once when the styles are the
ones applied and no light is lit now or was last frame, the case in which its loop skipped every
face. `Lightmaps::mark_lights` gives no marks, and allocates nothing, when no light is lit. The
light is drawn as before: the face loop runs whenever a style changes or a light is lit.

| Change | Before | After |
| --- | --- | --- |
| `relight_with` skips the frame when nothing it reads changed | 908, 900 samples of 5,732 and 5,731 (16%), 11,741 to 12,114 fps | 0 samples of 5,698 and 5,713, 14,289 to 14,603 fps (+20%) |

`frame_e1m1`, the budget's `timerefresh` pass (128 frames offscreen at one time, 383 us at
the 99th percentile), is the GPU's: five passes each took a median of 0.0203 s (159 us a frame)
before and after, so the budget is unchanged. `qs-render`'s 166 tests pass unchanged.

Left, with the reason: what remains of the frame is the view model's vertices, built on the CPU
each frame (`play.rs:615`, `Model::draw_alias`, 128 samples), and the preparation of the frame's
buffers for the GPU. At a few hundredths of a millisecond a frame on e1m1, no single site is
large enough to move a budget; a scene with many alias models is where they would show first.

### The simulation and the preview host: `qs-sim`, `qs-client`, `qs-game`, `quakec-engine`

The engine routes hold their budgets (`server_frame_e1m1` 477.83 us p99 against 1 ms, QuakeC
90.67 us of it). A `sample` of the release host in `--benchmark` mode on e1m1 for 10 s, with line
tables (2026-11-16), finds the simulation small beside drawing: `World::frame` 29 samples of the
main thread's 5,379, `Play::frame` and the map's GPU preparation most of the rest.

| Change | Before | After |
| --- | --- | --- |
| `Game::host_action` takes the progs' host command first and copies the host state (every setting, the command buffer and the aliases) only when there is one, not every frame | 109 samples, about 0.9 us a frame | 1 sample |

The host's 74 tests pass.

`--benchmark` draws about 12,000 frames a second with no vsync, so a cost paid once a frame is
counted far more often than in play at 72 Hz: the change is a small, certain saving, not a
visible one.

Left, with the reason: `Play::frame`'s largest part in that run is `MapMesh::relight_with`,
which already makes again only the faces whose light changed, as `R_RenderDynamicLightmaps`
does, and pays a pass over every face's styles each frame; at the route's 159 us p99 frame
against its 383 us budget there is nothing to win that a person would see.

The host's code is `qs-game` since #1018; `apps/quakec-engine` is its command line. The second
pass's lints fixed five sites in `qs-game`, one in `qs-sim` and one in `qs-client`.

### The rest

Held to the lints above and to the budgets that run through them, measured on the reference Mac
(`docs/gates/perf-mvp.md`, 2026-11-16). None of these crates appears in a profile taken for the
sections above, so beyond the lint fixes nothing was changed: the rule is a change only with a
measurement that shows it. The checklist's items a lint does not cover (a collection grown
without its size, a lookup repeated in a loop, a quadratic scan, work done every frame) were
looked for where a budget or a profile pointed, which in these crates was nowhere.

**The formats (L0): `qc-diag`, `qc-lno`, `qc-types`, `qs-bsp`, `qs-mdl`, `qs-pak`, `qs-wad`,
`qs-wav`, `qs-map`, `qs-vfs`, `qs-paths`, `qs-install`, `qs-log`, `qs-git`, `qs-diff`,
`qs-checkpoint`.** A map's load reads the PAK, the BSP, its textures and its models:
`map_load_e1m1` 93.66 ms and `map_load_mg1` 715.74 ms against 1 s; three runs of the release
host on the rerelease's hub on 2026-11-17 loaded it in 332, 352 and 372 ms. The load was not
profiled: `sample` attaches after a third of a second has passed. Fixed: 11 `or_fun_call` in
`qs-bsp`, 5 in `qs-map` and its brush pruning's collected flags, `qs-pak`'s entry accessors by value, one
`clone_from` in `qs-diff`. Left: the lints off above (`qs-checkpoint` 3 sites, `qs-log` 1).

**The language's front end and services (L2, L3, `apps/lsp`, `apps/cli`): `qc-lexer`,
`qc-syntax`, `qc-project`, `qc-engines`, `qc-ide`, `quakec-lsp`, `quakec-ide`.** The compiler's
section covers `qc-preproc` and `qc-sema`. The budgets through these: `open_project_and_type`
260.46 ms against 1 s, `check_after_typing` 0.356 ms and `keystroke` 0.128 ms against 8.33 ms.
Fixed: one `or_fun_call` in `qc-syntax`. Left: `quakec-lsp`'s 10 handlers that take their
request by value (`needless_pass_by_value`), one each in `qc-ide` and `qc-project`, the
collect in `qc-syntax`'s tree walk, which reverses an iterator that runs one way, and one
line count in `qc-ide` (`naive_bytecount`).

**The bridge and the sound (L4): `qs-bridge`, `qs-engine`, `qs-sound`.** `save_to_game`, an edit
seen in the running game over the bridge, is 80.59 ms against 1 s. Fixed: `qs-sound`'s null output's frame count by value. Left: one `needless_pass_by_value` in `qs-engine`.

**The commands (L5): `qs-commands`, `qs-layout`, `qs-settings`, `qs-templates`, `qs-map-edit`,
`qs-workbench`.** Every command a key, the palette or the agent runs goes through these:
`entities_outliner` 0.138 ms against 8.33 ms, `region_compile_e1m1` 168.32 ms against 1.2 s,
`textured_view_e1m1` 10.79 ms against 50 ms. Fixed: 4 `or_fun_call` in `qs-map-edit`, one in
`qs-layout`, a clone in `qs-commands`' palette search, 3 clones in `qs-workbench`. Left: the
lints off above (`qs-workbench` 7 sites, `qs-commands` 2, `qs-settings` 2, `qs-templates` 1).

**The agent (L6): `qs-agent`, `qs-mcp`, `qs-models`.** No budget: a turn waits on the model, not
on these. Fixed: one `or_fun_call` in `qs-models`. Left: `qs-agent`'s 14
`format_push_string`, the context it builds once a turn.

**The editor view (L7): `qs-editor-view`.** `keystroke` 0.128 ms and
`keystroke_split_fold_wrap` 0.192 ms against 8.33 ms, a sixtieth of a frame. Fixed: one
`or_fun_call`.

**The desktop app beyond the map view: `quakec-studio`.** The same keystroke budgets are the
whole window's frame. Fixed: 9 `or_fun_call`, 4 clones, one `clone_from`. Left: 3
`needless_pass_by_value` and one line count.

**Not shipped: `qs-bench`, `qs-differential`, `qs-tests`, `qs-porting-example`, `xtask`.** Held
to the same lints, since they are one workspace; their time is the CI's, measured by its own
runs (`docs/ci.md`), not a budget.
