# ADR-0021: Make Zed a porting upstream for the IDE

- **Status:** Accepted
- **Date:** 2026-11-06
- **Decided by:** Justin Plock
- **Issue:** None; decided in the session that built the IDE track (P3-12 to P3-16), carried out by this ADR's pull request
- **Requirements:** UI-14, LIC-2

## Question

May the IDE port code from Zed (`zed-industries/zed`), so that it gets a row in the upstream
table of `docs/porting.md`, rather than only being read for how it behaves?

## Context

UI-14 asks for "Zed's editor feel and Roblox Studio's play loop: calm, dense, keyboard-first,
native on each OS", and the development rules ask that a design lead with prior art, naming Zed
among the tools to follow. Until now Zed was read for behaviour only: the IDE track's tabs, file
finder and Open Folder (PR [#739](https://github.com/quakecstudio/editor/pull/739)) follow the
defaults in Zed's `assets/settings/default.json` and the ordering in
`crates/file_finder/src/file_finder.rs`, each cited in a comment. Rule H5 of
`cargo xtask licenses` refuses a `Ported from` line naming an upstream that is not in the table,
so code could not be ported from Zed, only behaviour matched.

What the repository says about its licence at `zed-industries/zed@4a6121bb0f10c6e8037b741b7dc0a4cd639ae289`
(the tip of `main` on 2026-11-06), read from a sparse checkout of that commit:

- `README.md`, "Licensing": "Zed source code is licensed primarily under GPL-3.0-or-later, with
  Apache-2.0 components where marked."
- The repository holds two licence files: `LICENSE-GPL` (SHA-256
  `0aa0322ea494da441088e4d9144c6720bf99d9e2e1eefcd29d1f58b19b5c4246`), the GPL version 3 text,
  and `LICENSE-APACHE` (SHA-256
  `752daf2fb234ca4a1fa372c073fe127f44b7b90fd2529ae44273a64f9d53da7a`), whose first line is
  `Copyright 2022 - 2025 Zed Industries, Inc.`
- Each crate states its licence in its `Cargo.toml`. Of the 254 crates under `crates/`, 207 say
  `license = "GPL-3.0-or-later"`, 37 say `license = "Apache-2.0"` (among them `gpui`, the UI
  framework), and 8 state none. The crates the IDE follows are GPL-3.0-or-later: `editor`,
  `workspace`, `file_finder`, `fuzzy`, `picker`, `project_panel`, `recent_projects`,
  `open_path_prompt`, `breadcrumbs`, `tab_switcher`, `debugger_ui` and `terminal_view`.
- The source files carry no licence or copyright header (`crates/file_finder/src/file_finder.rs`
  opens with `#[cfg(test)]`), so the only copyright line the repository gives is the one in
  `LICENSE-APACHE`.

GPL-3.0-or-later is this project's own licence ([ADR-0002](0002-license-model.md)), so a
GPL-3.0-or-later crate's code may be combined with ours as it is. Apache-2.0 code may be too,
but it brings Apache-2.0's notice and change-marking terms, and the IDE is built on egui, not on
`gpui`, so nothing yet needs it.

## Options

| Option | Consequences |
| --- | --- |
| A. A porting upstream for the IDE, its GPL-3.0-or-later crates only, under ADR-0017's clean-port policy (chosen) | Zed gets a row in `docs/porting.md`; the editor, workspace, finder, picker and debugger UI work in L5 to L7 and `apps/` may port Zed's logic and cite its functions. `cargo xtask licenses` accepts their `Ported from` headers |
| B. Keep Zed for behaviour only | The IDE keeps matching Zed's documented defaults and observed behaviour and writes everything fresh, as it has so far; subtle logic (fuzzy scoring, picker ordering, tab history) is reimplemented from descriptions |
| C. As A, and its Apache-2.0 crates too | Adds `gpui` and the other Apache-2.0 crates, with Apache-2.0's NOTICE and change-marking terms to carry in each ported file; nothing in the plan ports from them, since the IDE does not use `gpui` |

B was not chosen: Zed's crates are under this project's own licence, so reimplementing their
logic from descriptions adds work without a licence reason, and the behaviour UI-14 asks for is
best followed in the code that defines it. C was not chosen yet: no planned work needs an
Apache-2.0 crate, and adding one later is a change to this ADR's scope with its notice terms
worked out.

## Decision

Justin Plock decided on 2026-11-06, in the session building the IDE track: "we can include Zed
in docs porting if we want to take inspiration from more of their code."

So:

- `zed-industries/zed@4a6121bb0f10c6e8037b741b7dc0a4cd639ae289` is a porting upstream for the
  IDE, listed in the upstream table of `docs/porting.md`. Moving the pin is a PR that changes
  the row and checks the ported files' headers again.
- Only a crate whose `Cargo.toml` at the pinned commit says `license = "GPL-3.0-or-later"` may
  be ported. Its Apache-2.0 crates (`gpui` among them), crates that state no licence, and any
  third-party code vendored in the repository may not.
- A file ported from it keeps the line `Copyright 2022 - 2025 Zed Industries, Inc.` above ours,
  since Zed's source files carry no header of their own.
- The port follows ADR-0017's clean-port policy: port the behaviour, not the structure; the
  upstream's source is the authority, read and cited function by function; the Rust is written
  fresh inside our own design (egui, the L5 command registry, CONV-17), with no globals and no
  transliterated control flow. Zed's `gpui` entities, actions and settings do not come across;
  what a Zed setting does becomes our command or default, with the setting named in a comment.
- There is no oracle to differ against: a port from Zed is proved by its tests, which cite the
  Zed behaviour they pin (the setting, the function or Zed's own test).
- Zed is a source for the IDE layers only (L5 to L7 and `apps/`). Engine and format behaviour
  keeps id's code and the engines of ADR-0017 and ADR-0018 as its sources.

This is not legal advice; the licence compliance review before the first public release (P4-22,
[#133](https://github.com/quakecstudio/editor/issues/133)) confirms the details.

## Consequences

Carried by this ADR's pull request:

- `docs/porting.md` gains Zed's row, with its commit, its licence and the copyright line a port
  keeps, and says which crates may be ported. `.claude/rules/porting.md` summarises it.
- The test of `cargo xtask licenses` that lists the registered upstreams names Zed.

What it allows: the IDE work that follows Zed (tabs, the file finder, the picker, the project
panel, the debugger panel, preview tabs) may port Zed's functions and cite them in a
`Ported from zed-industries/zed@<commit>: <path>, <function>` line.

Harder or ruled out:

- Porting `gpui` or any other Apache-2.0 crate, or a crate with no stated licence, without
  changing this ADR.
- Porting Zed's structure: its entity model, actions and settings files stay Zed's.

## Links

- Pull request: this ADR's.
- Related ADRs: [ADR-0002](0002-license-model.md) (licence model, P4-22),
  [ADR-0003](0003-ui-framework.md) (egui, not gpui), [ADR-0017](0017-port-fteqcc.md) (the
  clean-port policy), [ADR-0018](0018-quakespasm-lineage-porting-upstreams.md) (the last
  upstreams added).
- Sources, read on 2026-11-06 at the pinned commit:
  - <https://github.com/zed-industries/zed/blob/4a6121bb0f10c6e8037b741b7dc0a4cd639ae289/README.md>
  - <https://github.com/zed-industries/zed/blob/4a6121bb0f10c6e8037b741b7dc0a4cd639ae289/LICENSE-GPL>
  - <https://github.com/zed-industries/zed/blob/4a6121bb0f10c6e8037b741b7dc0a4cd639ae289/LICENSE-APACHE>
  - <https://github.com/zed-industries/zed/blob/4a6121bb0f10c6e8037b741b7dc0a4cd639ae289/crates/file_finder/Cargo.toml>
  - <https://github.com/zed-industries/zed/blob/4a6121bb0f10c6e8037b741b7dc0a4cd639ae289/crates/gpui/Cargo.toml>
  - <https://github.com/zed-industries/zed/blob/4a6121bb0f10c6e8037b741b7dc0a4cd639ae289/assets/settings/default.json>
