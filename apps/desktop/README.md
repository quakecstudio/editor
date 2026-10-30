# quakec-studio

The desktop app (P2-17, ADR-0006): the window, its toolbar and its docked panels on egui
(eframe with the wgpu renderer, ADR-0003), themed from `docs/design/tokens.toml` with IBM Plex
Sans, JetBrains Mono and Lucide's icons (ADR-0010), and the macOS menu bar through `muda`.

- **Layer:** apps. It depends on `qs-layout`, `qs-workbench` and `qs-commands` (L5),
  `qs-editor-view` (L7), `qs-log` and `qs-paths` (L0), `eframe`, and on macOS `muda`.
- **Spec sections:** none; it implements plan requirements UI-2, UI-14, UI-15 and CONV-16.
- **Shipped:** yes.

## What it shows

The layout of the approved play-loop mockup (`docs/design/mockups/01-play-loop.png`): the
toolbar (Play, Pause, Step, Restart map, the preview host, command search and the panel
toggles), Project and Entities on the left, the editor and the game in the centre, Properties
and the agent on the right, Problems, Console and the debugger at the bottom, and the status
bar. The game, Entities, Properties, Console, the debugger and the agent are placeholders until
the preview host and the agent land; the toolbar's game controls are disabled until then.

Showing, hiding and resizing a panel runs a `qs-layout` command, from the toolbar, the macOS
View menu or a drag of the panel's edge, and the layout is written back to `layout.toml`.

## Project, Symbols and Problems

`cargo run -p quakec-studio -- <dir>` opens the project in `<dir>`, as the `project.open`
command does from the palette (P2-19, `qs-workbench`). The left panel's Project section lists
the files the project compiles as a tree, and its Symbols section the open file's symbols or,
under All, every file's. The Problems tab lists every diagnostic with its code, message and
`file:line:column`, and the status bar counts the errors and warnings. Choosing a file, a symbol
or a problem runs `file.open`, which shows the file in the editor (`qs-editor-view`) with the
caret at that place.

`fixtures/ember/` is a small project with two errors and a warning, which the tests in
`src/project/tests.rs` and the `project` snapshot open. `examples/problems_corpus.rs` is the
acceptance check on a real codebase: it opens the rerelease headless and clicks every problem's
row, and CI runs it in the Test Assets job:

```text
cargo run -p quakec-studio --example problems_corpus -- <rerelease codebase>
```

## Snapshots

`src/snapshots.rs` is the offscreen UI snapshot harness (P2-21, QA-3), on `egui_kittest`: each
script starts the app in a 1440 by 900 window, runs its commands and keys, renders the window
through wgpu with no window open, and compares it with its golden PNG in `snapshots/`. A
mismatch leaves `<name>.new.png` and `<name>.diff.png` beside the golden, which CI keeps as an
artifact. `make snapshots` writes the goldens again and `make screenshots` writes the same
images to `docs/screenshots/` for the docs. The goldens are rendered on macOS; Linux and Windows
may differ in a few pixels of anti-aliasing, within `max_failed_pixels`.

## The command palette

Cmd+Shift+P (Ctrl+Shift+P off macOS) or the toolbar's search opens the palette of the approved
mockup (`docs/design/mockups/04-command-palette.png`, UI-10): every command in the registry,
searched by summary and id with `qs_commands::palette::search`, each row its summary with the
matched characters in the accent, its id and its shortcut. Up and Down pick a row and Enter
runs it; a command with parameters first shows a form made from its schema
(`qs_commands::params`): a choice, a text, number or JSON field, or a checkbox per parameter.
Escape goes back from the form and closes the palette. A command's error stays on the palette.

`keymap.rs` binds shortcuts to commands that take no parameters: Cmd+Z runs `edit.undo` and
Shift+Cmd+Z `edit.redo`. The palette shows each command's shortcut from it.

## Strings

Every string the window shows is a `strings::Text` (CONV-16). The test
`the_ui_code_has_no_string_literals` fails on a string literal with words in it in the UI
modules, comments, attributes and log lines aside.

## Bundled fonts

`assets/fonts/`: IBM Plex Sans 1.1.0 and JetBrains Mono 2.304 (OFL-1.1) and Lucide 1.57.0's
icon font (ISC, with MIT for the icons it takes from Feather), each with its license file and a
row in `ATTRIBUTION.md`.

## Running it

```text
cargo run -p quakec-studio
```
