# Spike: gpui vs egui against the editor acceptance test

Issue: [#10](https://github.com/quakecstudio/editor/issues/10) (P0-10). Requirements: UI-1,
OPEN-3, PRIN-5. Prototypes: [`spikes/`](../../spikes/README.md). Raw data:
[`data/`](data/README.md). Decision: egui, in
[ADR-0003](../adr/0003-ui-framework.md) (accepted, merged in
[#250](https://github.com/quakecstudio/editor/pull/250)).

## Result

On the reference Mac (Apple M5) **both prototypes pass**: p99 frame CPU time over every measured
phase is 2.1 ms for egui and 6.2 ms for gpui with the 3D viewport on, 2.1 ms and 4.2 ms with it
off, against a threshold of 8.33 ms (120 Hz). **The mid-range Windows laptop and the Linux
Wayland machine have not been measured**: this spike had only the Mac. The acceptance test
needs all three, so the verdict is incomplete until someone runs
[`spikes/bench.sh`](../../spikes/bench.sh) on `ref-windows` and `ref-linux` (the steps are in
[What is left](#what-is-left)).

Beyond frame times, the spike found that neither framework provides a code editor, that egui
hosts a wgpu viewport natively while gpui needs a per-frame copy or `unsafe` interop, and that
gpui is currently distributed only through third-party snapshots. ADR-0003 weighs these and
recommends egui, subject to the Windows and Linux runs.

## What was built

Two prototypes over one framework-free crate, so they differ only in the framework:

| | Shared (`ui-common`, 1,522 lines) | gpui (`ui-gpui`, 1,322 lines) | egui (`ui-egui`, 732 lines) |
| --- | --- | --- | --- |
| Project | All `.qc` files under a directory, path order, first 50,000 lines as one buffer | Project panel: `uniform_list` | Project panel: `ScrollArea::show_rows` |
| Editor | Lines, multiple cursors, IME composition text, inline diff hunk, display rows | Custom `Element`: shapes visible rows with gpui's text system, paints carets; `EntityInputHandler` for the platform IME | Custom widget: lays out visible rows as `LayoutJob`s, paints carets; IME preedit and commit events |
| Highlighting | Line lexer with block-comment state; semantic classes (function, builtin, field, constant) from a declaration scan standing in for the semantic model; mockup colours | Text runs per token | Text sections per token |
| 3D viewport | A spinning box, one WGSL shader, the same mesh and matrices | Separate wgpu 29 device renders offscreen; pixels read back (pipelined over two buffers) into a gpui `RenderImage` | `egui_wgpu` paint callback inside egui's render pass, same device |
| Accessibility | One AccessKit text run per visible row, caret position | Element role `MultilineTextInput`, synthetic text-run children | `MultilineTextInput` node, text runs as child `Ui` nodes |

Both use the dark theme tokens and code colours from the approved mockups
([`06-theme-and-chrome.png`](../design/mockups/06-theme-and-chrome.png)) and Menlo for code
(the mockups' JetBrains Mono would be a bundled asset; the spike avoids bundling fonts). Neither
prototype implements selection, undo or soft wrap; they were not part of the test.

Versions: gpui `gpui-pre` 0.3.7 (a snapshot of Zed at `1a28cff`), egui and eframe 0.36.2,
wgpu 29.0.4 (gpui side) and 30.0.1 (egui side), Rust 1.99.0.

## Method

- **Project.** id's rerelease QuakeC (all six codebases) at the commit pinned in
  `corpus/manifest.toml`, fetched by [`spikes/fetch-corpus.sh`](../../spikes/fetch-corpus.sh)
  and checked by SHA-256; the first 50,000 lines in path order. Nothing is committed.
- **Script.** 2,845 frames, one action per frame, the same in both
  ([`script.rs`](../../spikes/ui-common/src/script.rs)): 120 idle warm-up frames (not measured);
  **type**: jump to line 25,000, type a 49-character statement and Enter 12 times, then 40
  backspaces; **scroll**: 600 frames down and 600 up at 3 rows per frame (180 rows a second at
  60 Hz); **multicursor**: 5 cursors typing 5 statements; **ime**: 20 compositions of にほん
  committed as 日本; **diff**: open an 8-line inline diff 4 times and scroll through it.
- **Frame time.** CPU time from applying the frame's action to handing the frame to the GPU,
  without any wait for vsync. egui: eframe's own `cpu_usage` for the frame, which covers UI,
  tessellation and wgpu submission. gpui: the action plus gpui's profiler draw and present
  durations. Percentiles are nearest rank, as in [`perf-budgets.md`](../perf-budgets.md).
- **Runs.** Each prototype with and without the viewport, three runs each, interleaved, on one
  machine with nothing else being measured. The window was kept above other windows, because
  gpui stops drawing an occluded window.
- **Machine.** `ref-mac`: Mac17,2, Apple M5 (10 CPU cores, 10-core GPU, 24 GB), macOS 27.0.1,
  on an external 60 Hz display, so frame intervals are bounded below by 16.7 ms; frame CPU time
  is what the 120 Hz threshold is checked against.

## Results on `ref-mac`

p99 frame CPU time per phase, three runs pooled (the full table, with p50, p95 and maximum, is
[`data/ref-mac/summary.md`](data/ref-mac/summary.md)):

| Prototype | type | scroll | multicursor | ime | diff | **all** | Frames over 8.33 ms |
| --- | --- | --- | --- | --- | --- | --- | --- |
| egui | 2.06 | 2.07 | 2.22 | 1.88 | 1.95 | **2.12** | 0 of 8,175 |
| egui, no viewport | 2.12 | 1.97 | 2.16 | 1.89 | 1.89 | **2.05** | 0 of 8,175 |
| gpui | 5.93 | 6.03 | 7.25 | 8.19 | 5.52 | **6.15** | 6 of 8,175 |
| gpui, no viewport | 3.55 | 3.92 | 4.59 | 6.70 | 3.71 | **4.18** | 3 of 8,175 |

Medians over all measured phases: egui 1.39 ms (1.31 without the viewport), gpui 4.09 ms (2.68
without). Of gpui's frame time, draw (layout and paint) is the median 3.9 ms with the viewport and
2.5 ms without; present (Metal submission) is 0.15 to 0.19 ms.

What the numbers say:

- **Both pass on this machine, egui with more margin.** egui's p99 is a quarter of the budget,
  gpui's three quarters with the viewport. A mid-range Windows laptop can be several times
  slower per core than an M5, so the margin matters, and only the Windows and Linux runs can
  settle it.
- **The viewport costs gpui about 1.4 ms per frame and egui almost nothing.** egui draws the
  scene inside its own pass; gpui copies 1.7 MB of pixels from a second device and uploads
  them to its atlas every frame.
- **gpui's frames over budget are first-time costs.** All 9 are one of two frames: the first
  frame of the IME phase in every gpui run (8.4 to 11.9 ms), where the first CJK character
  needs a fallback font, and the jump to a new region at the start of the multi-cursor phase
  in two viewport runs (8.4 and 8.6 ms), where Core Text shapes a whole screen of lines it has
  not cached. Once shaped, lines are cached frame to frame. egui has no font fallback: the
  prototype loads its CJK font at startup (see [Text and fonts](#text-and-fonts)), and no egui
  frame went over budget.
- **Scrolling and typing are cheap in both** because both editors lay out only visible rows.
  An early gpui version that rendered all panels in one view and laid out 200 project rows as
  plain `div`s spent most of a 4 to 5 ms frame in layout (taffy and text wrapping, from a
  profile); gpui needs views split so that unchanged ones are cached, and long lists
  virtualised, as Zed does.
- **Frame pacing.** egui presented every frame at the display's 16.7 ms, with one interval over
  25 ms in 16,350. gpui did the same except in one viewport run where its window was inactive and
  gpui capped it at 20 to 30 frames a second (by design, to save power); per-frame CPU time in
  that run was in line with the others.

Startup (`open_project_and_type`, budget 1 s): reading and lexing the 50,000 lines took 16 to 40
ms, and the first frame was built 95 to 146 ms after launch in both.

## Findings by topic

### The editor

Neither framework ships a code editor. The design document credits gpui with "fast shaping,
multi-cursor, large buffers"; those are features of Zed's `editor` crate, which is
GPL-3.0-or-later and so license-compatible, but it is not a library: it lives in Zed's
repository and depends on Zed's project, language, settings and workspace crates. Both
prototypes therefore use a custom widget over our own buffer. gpui's element API made the input
handler and text runs natural; egui's immediate mode made the widget shorter.

One ready-made editor exists for gpui and was **not measured**: GPUI Kit's `Editor`
(`gpui-component`, Apache-2.0, by the same maintainer as the `gpui-pre` snapshots), a rope and
tree-sitter based code editor that added multi-cursor editing in 0.6.1 and lists highlighting,
folding, diagnostics, search and LSP hooks. Its 0.7.0 release builds on `gpui-pre` 0.3.7, the
snapshot used here, so it could be put under the same harness. egui has no comparable
component. If gpui is preferred, measuring that editor against the script is the most useful
next step, because it is the one thing that could save the editor-widget work on either side.

### IME

- **gpui** implements the platform's text input client: on macOS `NSTextInputClient`, with
  marked text ranges, replacement ranges and `bounds_for_range` for placing the candidate
  window. The prototype implements `EntityInputHandler` over the cursor's line.
- **egui** receives `Ime::Preedit` and `Ime::Commit` events from winit and reports the caret
  rectangle through `IMEOutput`; there are no replacement ranges or reconversion.
- **Tested:** the scripted compositions exercise each prototype's composition rendering
  (underlined marked text at the caret) in the measured runs. **Not tested:** typing through a
  real OS input method, which needs a person at the machine.

### Multi-cursor and inline diff

Both are model features in `ui-common` and render the same way in both prototypes: carets per
cursor, and removed rows (red) above added rows (green) with `-` and `+` in the gutter, as in
[`03-inline-edit.png`](../design/mockups/03-inline-edit.png). The scripted phases cover them.
Keyboard bindings (Cmd+Alt+Down, Alt+click, Escape) are wired in both but were not exercised
by hand.

### Accessibility

Both frameworks integrate AccessKit on macOS, Windows and Linux; gpui since 2026 (its crates
depend on `accesskit_macos`, `accesskit_windows` and `accesskit_unix`), egui since 2022. In
both, a custom editor must publish its own text runs and caret, which the prototypes do.
Checked through the macOS accessibility API from a script (no screen reader):

- **egui:** the window's tree contained the editor as an `AXTextArea` labelled "QuakeC
  source" with its caret reported (`AXSelectedTextRange`), plus the panel labels and status
  text. Its `AXValue` was empty, so the text runs are not yet reaching the adapter as the
  area's value; that needs work in the widget.
- **gpui:** the tree stayed empty: gpui builds it only once it decides assistive technology is
  connected (`Window::is_a11y_active`), and querying from a script did not trigger that.

Neither was tried with VoiceOver, Narrator or Orca.

### The 3D viewport

- **egui:** `egui_wgpu::Callback` runs our pipeline inside egui's render pass on egui's device
  and queue. No copies, no `unsafe`, and the same path works on the web.
- **gpui:** gpui renders with Metal on macOS, DirectX 11 on Windows and wgpu on Linux and the
  web, and exposes none of those devices. The prototype renders on its own wgpu device and
  reads the pixels back into a `RenderImage`, two buffers deep so the UI thread does not wait
  for the GPU, at the cost of a frame of latency and about 1.4 ms per frame at 720 x 600. A
  zero-copy path needs per-OS `unsafe` interop (an IOSurface through gpui's macOS-only
  `surface()` element, a shared DirectX texture on Windows), which `unsafe_code = "deny"`
  (CONV-3) does not allow today. This matters for the map editor (P5-05 onward), model
  previews and the post-MVP embedded game view.

### Text and fonts

gpui uses the platform text stack (Core Text on macOS, DirectWrite on Windows, cosmic-text on
Linux and the web) with system font lookup and fallback; the fonts ADR-0010 bundles (IBM Plex
Sans, JetBrains Mono) are added to it. egui shapes and rasterises its own
glyphs from fonts it is given: it has no system font lookup or fallback, so the prototype names
font files (Menlo, SF, Hiragino on macOS; DejaVu and Noto on Linux; Consolas, Segoe UI and MS
Gothic on Windows), and a shipped app would bundle its fonts, including a CJK fallback. egui's
own bundled fonts are `OFL-1.1 AND Ubuntu-font-1.0`, which `deny.toml` does not allow, so the
prototype turns `default_fonts` off.

### Window chrome

[ADR-0010](../adr/0010-visual-design.md), accepted while the spike ran, asks for native chrome:
macOS traffic lights and the system menu bar, and a client-side title bar on Windows and Linux.
gpui sets the macOS menu bar (`App::set_menus`) and places the traffic lights in a transparent
title bar (`TitlebarOptions`), as Zed does. egui and winit have no native menus; the menu bar
would need a separate crate such as `muda`. The Windows and Linux title bars are drawn by the
app in either. Neither prototype built the chrome.

### Web build (OPEN-3)

egui's web target is mature: eframe runs in the browser on WebGL2 or WebGPU, and the paint
callback path works there. gpui gained a basic web platform in February 2026 (Zed PR #50228):
one canvas-backed window, WebGPU with a WebGL2 fallback, bundled fonts. The MVP excludes a web
build (L-07 is in LATER), so either framework keeps it possible; it does not have to decide the
choice. Neither prototype was built for the web.

### Distribution and versions

- **egui** is released on crates.io by its maintainers: 0.36.0 on 2026-08-05, 0.36.2 on
  2026-09-08, 0.37.0 on 2026-10-15. MIT OR Apache-2.0.
- **gpui** on crates.io is 0.2.2, from 2025-10-22, before Zed split gpui into platform crates.
  A Zed discussion (April 2026) reports that the current gpui cannot be published without
  changes to Zed's manifests. Since 2026-09-03 the gpui-component maintainer has published
  weekly `gpui-pre-*` snapshots of Zed's crates, each naming its source commit (0.3.0 to
  0.3.9); a third-party news page reports that Zed said it would automate releases (not
  verified here). Using gpui today means a third-party snapshot or a git dependency on Zed
  pinned to a commit (an `allow-git` entry in `deny.toml`). Apache-2.0.

### Licenses, advisories and dependencies

`cargo deny check` with the repository's `deny.toml`, run against the spike workspace:

| | egui (`default_fonts` off) | gpui |
| --- | --- | --- |
| Licenses | pass | fails on `libbz2-rs-sys` (`bzip2-1.0.6`), through `gpui-pre-http-client` and `async-compression`; a permissive license that is not in the allow list |
| Advisories | `paste` unmaintained (RUSTSEC-2024-0436), through wgpu's Metal backend and `image` | the same, plus `rustybuzz` (RUSTSEC-2026-0206) and `ttf-parser` (RUSTSEC-2026-0192) unmaintained, through `usvg` and `resvg` |
| Sources | crates.io only | crates.io only (`gpui-pre`); a Zed git pin would need `allow-git` |
| Crates on macOS | 181 | 451 |

All 796 crates in the spike's `Cargo.lock` (all targets) are otherwise under licenses the
allow list accepts.

### Build requirements

- **gpui:** on macOS, Xcode's separately downloaded Metal toolchain to compile its shaders, or
  the `runtime_shaders` feature the prototype uses; on Windows, the Windows SDK's shader
  compiler and a resource compiler at build time (neither could be cross-checked from the
  Mac); on Linux, a C compiler and the Wayland, X11, xkbcommon and fontconfig development
  packages. It also pulls `block` 0.1.6, which rustc warns will be rejected by a future
  version.
- **egui:** pure Rust; type-checks for `x86_64-pc-windows-msvc` and
  `aarch64-unknown-linux-gnu` from the Mac without extra tools.

Neither prototype is built in CI: `spikes/` is its own Cargo workspace, outside the root
workspace and every CI job.

## Acceptance criteria

| Criterion | Status |
| --- | --- |
| Both prototypes measured with the same scripted input and harness | Met: one script, one driver and one statistics module in `ui-common`; raw data in [`data/ref-mac/`](data/ref-mac/) |
| p99 at most 8.3 ms while typing and scrolling, on all three machines | Met on `ref-mac` by both. **Not measured** on `ref-windows` and `ref-linux` |
| IME works | Composition rendering met in both (scripted). Real OS input method **not tested** |
| Multi-cursor works | Met in both (scripted) |
| A wgpu viewport renders inside the window | Met in both: egui natively, gpui through a read-back image |
| If neither passes, the ADR names the fallback and its cost | Both pass on the Mac; ADR-0003 names the fallback in case either fails on Windows or Linux |

## What is left

Things only a person with the hardware can do, before ADR-0003 is accepted:

1. **Windows.** On the mid-range Windows 11 laptop: install the Windows SDK, then
   `bash spikes/fetch-corpus.sh C:/qs-rerelease` and
   `QS_BENCH_HOST=ref-windows bash spikes/bench.sh C:/qs-rerelease docs/spikes/data/ref-windows 3`
   (Git Bash), keeping the window visible. Commit the directory it writes.
2. **Linux under Wayland.** Install the development packages listed above, then the same with
   `QS_BENCH_HOST=ref-linux` into `docs/spikes/data/ref-linux`.
3. **IME by hand**, on each OS: type Japanese (and ideally Chinese pinyin) with the system input
   method in both prototypes; check that the composition shows at the caret, the candidate
   window sits next to it, and committing inserts the text.
4. **Screen reader by hand:** VoiceOver, Narrator and Orca on the editor: does it read the
   line under the caret, and follow it as it moves?
5. **Multi-cursor by hand:** Cmd+Alt+Down (Ctrl+Alt+Down elsewhere), Alt+click, type, Escape.

## Sources

- gpui: [crates.io `gpui`](https://crates.io/crates/gpui) (0.2.2, 2025-10-22);
  [crates.io `gpui-pre`](https://crates.io/crates/gpui-pre) and
  [`gpui-pre-platform`](https://crates.io/crates/gpui-pre-platform) (snapshots from 2026-09-03);
  [Zed discussion #55271](https://github.com/zed-industries/zed/discussions/55271), "Is it time
  to publish a new version of GPUI on crates.io?" (2026-04-30);
  ["The 315-Day Vacuum"](https://gpui-archipelago.github.io/news/315-day-vacuum/) (third-party
  account of the snapshots and Zed's reply, not verified);
  [Zed PR #50228, "GPUI on the web"](https://github.com/zed-industries/zed/pull/50228)
  (2026-02-26); gpui's accessibility guide, `src/_accessibility.rs` in `gpui-pre` 0.3.7; the
  macOS frame source and occlusion handling, `src/window.rs` in `gpui-pre-macos` 0.3.7.
- gpui-component: [crates.io](https://crates.io/crates/gpui-component),
  [editor documentation](https://longbridge.github.io/gpui-component/docs/components/editor),
  [GPUI Kit releases](https://github.com/longbridge/gpui-kit/releases) (multi-cursor in 0.6.1);
  `gpui-component` 0.7.0's manifest (depends on `gpui-pre` =0.3.7).
- egui: [crates.io `egui`](https://crates.io/crates/egui) and
  [`eframe`](https://crates.io/crates/eframe) (release dates above); `CallbackTrait` in
  `egui-wgpu` 0.36.2; `IMEOutput` and `accesskit_node_builder` in `egui` 0.36.2.
- Advisories: [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436),
  [RUSTSEC-2026-0206](https://rustsec.org/advisories/RUSTSEC-2026-0206),
  [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192).
