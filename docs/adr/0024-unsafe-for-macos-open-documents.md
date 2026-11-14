# ADR-0024: Allow one unsafe module to receive the files macOS opens with the app

- **Status:** Accepted
- **Date:** 2026-11-14
- **Decided by:** Justin Plock
- **Issue:** [#850](https://github.com/quakecstudio/editor/issues/850), whose installer package registers the file types; the work is PR #945
- **Requirements:** QA-6, ARCH-3

## Question

May the desktop app use `unsafe` code, which every crate denies, to receive the files macOS
asks it to open from Finder, the Dock or Open With?

## Context

macOS does not put a file opened with an app on its command line. It sends the request to the
app delegate's `application:openURLs:`, and an app whose delegate lacks that method opens with
nothing and Finder says it cannot open the file. The app's delegate is winit's: eframe 0.36.2,
the UI framework (ADR-0003), runs on winit 0.30.13, whose `WinitApplicationDelegate` has no
such method and offers no other way to receive the files. winit 0.31 lets an app set its own
delegate, but eframe does not use it yet, and that delegate would still be written with
`unsafe` objc2 code.

`docs/conventions.md` (Lints) settled in P0-01 that no crate needs `unsafe` and keeps
`unsafe_code = "deny"` everywhere, saying a proven need goes in one platform crate with a
`SAFETY:` comment on every block, the section changing in the same PR.

Prior art: Zed's gpui registers its own delegate class with `application:openURLs:` in
`crates/gpui_macos/src/platform.rs` (zed-industries/zed@4a6121bb), in `unsafe` code; the
`bevy_macos_open_files` crate adds the method to winit 0.30's delegate class for Bevy apps, and
needs Bevy. No crate does it for an eframe app (crates.io searched 2026-11-14).

## Options

| Option | Consequences |
| --- | --- |
| A. One small `unsafe` module in the desktop app | `apps/desktop/src/open_documents.rs`, macOS only, adds the method to winit's class with objc2 0.6 (already in the tree), behind two `#[expect(unsafe_code, reason)]` attributes at function scope. Finder, the Dock and Open With work now. The module names winit's private class, so a winit upgrade has to keep it or replace the module. |
| B. Ship without opening files from Finder | No `unsafe`. The file types are registered for their names only, and the app is not offered under Open With on macOS until eframe moves to winit 0.31, which still needs `unsafe` for a delegate. |
| C. A separate crate of our own, published outside the workspace | The workspace stays free of `unsafe`, at the cost of a repository, releases and upkeep for about 80 lines. |

## Decision

Option A. The desktop app may hold the one `unsafe` module `apps/desktop/src/open_documents.rs`,
compiled on macOS only, which adds `application:openURLs:` to winit's delegate class; each
`unsafe` block has a `SAFETY:` comment and each function its own `#[expect(unsafe_code,
reason)]`. It is the only `unsafe` in shipped code. Justin chose it on 2026-11-14 over B and C,
after asking for a dependency first and learning there is none for an eframe app.

## Consequences

- `docs/conventions.md` (Lints) names this module as the one exception.
- winit is pinned exactly (`=0.30.13`), so an upgrade is its own change: it has to keep
  `WinitApplicationDelegate` or replace the module, with winit 0.31's own delegate when eframe
  uses it. If the class is not found the app logs a warning and runs without the method.
- Any other `unsafe` needs its own ADR.

## Links

- PR #945, which adds the module and this ADR; issue #850.
- ADR-0003, the UI framework.
- `docs/conventions.md`, Lints.
- zed-industries/zed@4a6121bb0f10c6e8037b741b7dc0a4cd639ae289, `crates/gpui_macos/src/platform.rs`.
- bevy_macos_open_files 0.20.1 on crates.io, `src/macos.rs`.
- winit 0.30.13, `src/platform_impl/macos/app_state.rs`.
