# ADR-0003: Build the editor UI on egui

- **Status:** Proposed
- **Date:** 2026-10-15
- **Decided by:** pending
- **Issue:** [#10](https://github.com/quakecstudio/editor/issues/10)
- **Requirements:** UI-1, OPEN-3, PRIN-5

## Question

Should QuakeC Studio's UI layer (L7) be built on gpui or on egui, and does a web build matter
enough to influence the choice?

## Context

The design document recommends gpui, "settled with a two-week spike before committing", and
defines the acceptance test: open a 50,000-line QuakeC project, type with semantic highlighting,
use multi-cursor and IME, scroll at 120 Hz, render an inline diff, and measure frame times on an
M-series Mac, a mid-range Windows laptop and Linux under Wayland (UI-1, PRIN-5). The P0-10 issue
adds a wgpu 3D viewport inside the window, because the map editor (P5-05 onward) and model
previews share one wgpu renderer. OPEN-3 asks whether a web build matters enough to choose egui.
Waiting on the answer: the app shell (P2-17, [#73](https://github.com/quakecstudio/editor/issues/73)),
every L7 issue after it, the renderer (P5-05) and the web build (L-07). Two decisions accepted
while the spike ran bear on it: the preview host is a Rust engine on wgpu
([ADR-0004](0004-preview-host-base.md)), and the visual design bundles IBM Plex Sans and
JetBrains Mono and uses native chrome on each OS: macOS traffic lights and the system menu bar,
a client-side title bar on Windows and Linux ([ADR-0010](0010-visual-design.md)).

The spike built the same editor on both frameworks from shared, framework-free code
(`spikes/`): a virtualised custom editor with lexer-based semantic colours, multi-cursor, IME
composition, an inline diff, a virtualised project panel and a wgpu viewport, driven by the same
2,845-frame script. The full report, with sources, is
[`docs/spikes/ui-framework.md`](../spikes/ui-framework.md). What it found:

- **Performance on the reference Mac (M5).** Both pass with margin. p99 frame CPU time over
  every measured phase, three runs each: egui 2.1 ms with the viewport and without; gpui
  6.2 ms with the viewport and 4.2 ms without (threshold 8.33 ms). egui used between a third
  and a half of gpui's CPU per frame in these prototypes, and no egui frame went over budget;
  gpui's 9 over-budget frames (of 16,350) were first-time text shaping. **The Windows and
  Linux reference machines have not been measured**; the acceptance criterion needs all three.
- **Neither framework ships a code editor.** The design document credits gpui with "fast
  shaping, multi-cursor, large buffers", but those belong to Zed's `editor` crate, which is not
  a library (it lives in Zed's repository and depends on Zed's project, language and settings
  crates). Both prototypes needed the same custom editor widget; the gpui one is 1,322 lines
  and the egui one 732, over 1,522 shared lines. A third-party gpui editor component, GPUI
  Kit's `Editor` (Apache-2.0, multi-cursor since 0.6.1), exists and was not measured; egui has
  nothing comparable.
- **The 3D viewport.** egui draws wgpu inside its own render pass through a paint callback,
  on the same device, with no copy. gpui renders with Metal on macOS and DirectX 11 on Windows
  and exposes no device; the safe path is to render on a separate wgpu device and copy the
  pixels into a gpui image every frame, which cost about 1.4 ms per frame at 720 x 600 and adds
  a frame of latency. Sharing a texture needs `unsafe` platform interop per OS. The preview
  host is also wgpu (ADR-0004), so if the post-MVP embedded game view (ENG-10) renders in the
  IDE's process, egui hosts it the same way.
- **Window chrome (ADR-0010).** gpui sets the macOS menu bar (`App::set_menus`) and positions
  the traffic lights in a transparent title bar, as Zed does. egui has no native menus; the macOS
  menu bar would need a separate crate such as `muda`. On Windows and Linux the client-side title
  bar is drawn by the app in either framework.
- **Distribution.** egui is released on crates.io by its maintainers every few weeks (0.36.2
  used; 0.37.0 released 2026-10-15). The `gpui` crate on crates.io is 0.2.2 from 2025-10-22,
  and a Zed discussion from April 2026 reports that the current gpui cannot be published
  without changes to Zed's manifests. The spike used `gpui-pre`, a weekly snapshot of Zed's
  code published by a third party (the gpui-component maintainer). Zed has said, as reported by
  a third-party news page and not verified here, that it will automate GPUI releases. The
  alternative is a git dependency on Zed pinned to a commit.
- **Supply chain.** On macOS the gpui prototype resolves 451 crates and the egui one 181.
  Against `deny.toml` unchanged: gpui needs `bzip2-1.0.6` allowed (through its HTTP client)
  and brings two more unmaintained-crate advisories (`rustybuzz`, `ttf-parser`); egui's licenses
  pass once its bundled fonts (`OFL-1.1 AND Ubuntu-font-1.0`) are turned off. Both pull `paste`
  (unmaintained) through wgpu's Metal backend and `image`.
- **Text, IME and accessibility.** gpui uses each platform's text stack (Core Text on macOS)
  with font fallback, and its input handler is the platform's text input client, with marked
  ranges and candidate-window placement. egui shapes and rasterises its own glyphs, has no
  system font lookup or fallback (the prototype names font files, CJK included), and receives
  IME composition as preedit and commit events. Both expose AccessKit; a custom editor must
  publish its own text runs in either. On macOS the egui editor appeared to the accessibility
  API as a text area with its caret position; gpui's tree could not be activated from a
  script. Real IME input and VoiceOver were not tested on either (they need a person).
- **Web (OPEN-3).** egui's web target is mature (WebGL2 and WebGPU). gpui gained a basic web
  platform in February 2026 (one window, WebGPU with a WebGL2 fallback, bundled fonts). The
  MVP excludes a web build (L-07 is LATER), so either choice keeps it possible.
- **Behaviour seen while measuring.** gpui stops drawing a macOS window that is occluded (its
  display link stops) and caps an inactive window's frame rate, both to save power; egui kept
  drawing in every case, even with the display asleep. The benchmark therefore keeps its window
  above others. This is good behaviour for an editor, and a constraint for any test that
  drives gpui frames.

## Options

| Option | Consequences |
| --- | --- |
| **A. egui (recommended)** | P2-17 builds the shell on `eframe` with the wgpu renderer; the editor is the custom widget from the spike, grown into a crate in L7. P5-05's renderer draws inside the UI through `egui_wgpu` callbacks with no copy, and so could an in-process embedded game view after the MVP. The fonts ADR-0010 bundles are loaded into egui, plus fallback fonts (CJK and symbols) that gpui would take from the OS. The macOS menu bar needs a separate crate such as `muda`; the Windows and Linux title bars are drawn by the app. Web build (L-07) stays feasible with little extra work |
| B. gpui | P2-17 builds on gpui, pinned to a `gpui-pre` snapshot or a Zed commit (`allow-git` in `deny.toml`) until Zed publishes; each bump is manual work against a pre-1.0 API. The editor is the same custom element, or GPUI Kit's `Editor` if it passes the harness (it follows the same snapshots). P5-05 needs per-OS texture sharing with `unsafe` in one platform crate (an exception to CONV-3's `unsafe_code = "deny"`), or pays a per-frame readback. `deny.toml` gains `bzip2-1.0.6` and two advisory ignores. Native text with system font fallback, the platform IME client and the macOS menu bar come with the framework |
| C. Decide after the Windows and Linux runs | Nothing in P2 starts until a person runs `spikes/bench.sh` on `ref-windows` and `ref-linux`. If either framework fails there, that decides it; if both pass, the trade-offs above decide it |

If neither framework passes on all three machines, the fallback is the closer framework with
the editor widget's text path replaced by our own glyph cache and shaping (for example
`swash` and `parley` on egui, drawn as textured quads in one wgpu pass), which we estimate,
without having prototyped it, at three to four weeks, plus profiling on whichever OS failed.
On the reference Mac both prototypes spend most of their frame in text layout and painting,
so that is where the margin is.

## Decision

While Proposed: **egui, confirmed by measuring both prototypes on the Windows and Linux
reference machines before P2-17 starts (option A, with option C's measurement).** Both
frameworks meet the frame budget on the reference Mac, egui with three times the margin, and
neither framework gives us a code editor, so the editor widget is our work on either (GPUI
Kit's untested `Editor` is the one possible exception). What separates them is everything
around the editor: egui hosts the wgpu renderer that the map editor and previews depend on
without copies or `unsafe`, it is published and maintained on crates.io, it has fewer than half
the dependencies and needs no new license exceptions in `deny.toml`, and it already runs on the
web if L-07 is ever taken up. gpui's real advantages are native text rendering, the platform
IME client and the macOS menu bar that ADR-0010 asks for, which matter to how the editor feels;
on egui they are work we will have to schedule (font fallback, IME edge cases, menus), and they
are listed as consequences below. The answer to OPEN-3 is that a web build is not required for
v1 and does not decide the framework.

## Consequences

If accepted as recommended:

- **Plan.** P2-17 (app shell) is built on `eframe` 0.36 or later with the wgpu renderer. A new
  L7 issue turns the spike's editor widget into `crates/qs-editor-view` (text layout cache,
  selections, IME, AccessKit text runs) before P2's editor features. P5-05 uses `egui_wgpu`
  paint callbacks. L-07 (web build) stays in LATER with a note that egui supports it. The
  design document's "Editor and UI" section and its "gpui API churn" risk are rewritten for
  egui, and the "Does a web build matter enough to choose egui?" open question is closed.
- **Conventions and checks.** `eframe` and `egui` join `[workspace.dependencies]` with
  `default-features = false` (no `default_fonts`). `deny.toml` needs an advisory ignore for
  `paste`, which wgpu's Metal backend pulls in for either framework, with its reason.
- **Assets.** Besides the fonts ADR-0010 already bundles, a CJK fallback font (OFL-1.1, such
  as Noto Sans CJK) is bundled and listed in `ATTRIBUTION.md`.
- **Harder.** Native-looking text (hinting, subpixel rendering), system font fallback, full
  IME behaviour (reconversion, replacement ranges) and the macOS menu bar are not provided by
  egui and must be built, added (`muda` for menus) or accepted as limits. egui lays out the
  whole UI every frame it draws, so panels must stay virtualised as the spike's are.
- **Not yet known.** Windows and Linux frame times, real IME input and VoiceOver, Narrator and
  Orca behaviour. These are listed in the spike report's checklist and must be recorded before
  this ADR is accepted. If gpui is preferred instead, GPUI Kit's `Editor` should also be put
  under the spike's harness first.

## Links

- Issue: [#10](https://github.com/quakecstudio/editor/issues/10); spike report:
  [`docs/spikes/ui-framework.md`](../spikes/ui-framework.md); raw data:
  [`docs/spikes/data/`](../spikes/data/); prototypes: [`spikes/`](../../spikes/)
- Related: [ADR-0002](0002-license-model.md) (license model, GPL-3.0 compatibility of
  dependencies), [ADR-0004](0004-preview-host-base.md) (preview host on wgpu),
  [ADR-0010](0010-visual-design.md) (visual design, fonts and native chrome)
- Design document, "Editor and UI" and "Risks and open questions":
  [`docs/design/design-doc.md`](../design/design-doc.md)
- Sources are listed in the spike report's Sources section.
