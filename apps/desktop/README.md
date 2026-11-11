# quakec-studio

The desktop app (P2-17, ADR-0006): the window, its toolbar and its docked panels on egui
(eframe with the wgpu renderer, ADR-0003), themed from `docs/design/tokens.toml` with IBM Plex
Sans, JetBrains Mono and Lucide's icons (ADR-0010), and the macOS menu bar through `muda`.

- **Layer:** apps. It depends on `qs-layout`, `qs-workbench` and `qs-commands` (L5),
  `qs-editor-view` and `qs-render` (L7), `qs-log` and `qs-paths` (L0), `eframe`, `skrifa` for the system font
  fallback, and on macOS `muda`.
- **Spec sections:** none; it implements plan requirements, not engine behaviour.
- **Requirements:** UI-2, UI-14, UI-15 and CONV-16.
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

## The code editor

The editor (P2-18, UI-4) is `src/editor.rs` over `qs-editor-view`: the open file coloured by
its keywords, literals and comments and by the semantic model's names (`src/code_style.rs`,
the `[syntax]` colours of `docs/design/tokens.toml`), multiple carets, undo that groups typing,
and an inline diff from a base text. Its keys are VS Code's:

| Key | What it does | Command |
| --- | --- | --- |
| Cmd+S | Saves the file | `file.save` |
| Cmd+F | Opens the find and replace bar | (the view's own) |
| F12 | Goes to the definition | `symbol.definition`, `file.open` |
| Shift+F12 | Lists the references; a click opens one | `symbol.references` |
| F2 | Renames everywhere, unsaved | `symbol.rename` |
| Ctrl+Space | Completes the name being typed | `symbol.complete` |
| Cmd+Z, Shift+Cmd+Z | Undoes and redoes the editor's edits | (the view's own) |

Resting the pointer on a name shows its declaration (`symbol.hover`). An edit reaches the
project as `file.update` once typing pauses for 0.3 s, and at once before a save or a question,
so the Problems list and every answer follow the text on screen.

## The map view

The Map tab draws a BSP with `qs-render` inside the window, through an `egui_wgpu` paint
callback on the window's wgpu device (P5-05, ADR-0003): `Shell::show_map(mesh, camera)` puts a
renderer of the mesh in egui-wgpu's callback resources and shows the tab. The window has a
32-bit depth buffer (`depth_buffer: 32`) for the renderer's depth test. Dragging looks around
and the wheel moves forward and back. Opening a map from the UI comes with the map editor;
`src/map_view/tests.rs` draws a test floor and compares it with `snapshots/map-view.png`,
through a test renderer that gives egui's render pass the same depth buffer.

## Inline edit

Cmd/Ctrl+K on a selection in the code editor (P4-10, UI-6), as Zed's inline assistant:

1. A box asks what to change.
2. The agent panel's model rewrites the selection off the frame, from
   `qs_agent::INLINE_SYSTEM` and `inline_prompt`.
3. The answer replaces the selection as **one** undo step and is drawn as an inline diff from
   the text before.
4. Accept keeps it, and one undo still takes it back. Reject undoes it.

With no provider set up, the box says so. `src/editor/tests.rs` drives it with a fake model.

## The agent panel

The right panel's Agent section (P4-09, UI-5, PRIN-4), modelled on Zed's agent panel, is
`src/agent.rs`:

- **Provider:** until one is set up, a form for Anthropic, an OpenAI-compatible endpoint or a
  local Ollama, with the model, an optional address and the API key. The key goes to the
  system keychain (`qs_models::Keychain`); the rest goes to `agent.json` in the app's data
  directory, which never holds a key.
- **Threads:** each task starts a thread, and any number run at once. A thread is a
  `qs_agent::Run`, driven a reply at a time. Its model waits on a thread of its own, streaming
  the reply's text, and the window takes each reply between frames
  (`AgentPanel::pump`). The run's tool calls go through the window's own session, so the
  window shows what the agent does.
- **What it shows:** the model's text as it is written, each call with whether it ran, and how
  the run ended (done, or the reasons it is not). There is also a button that restores the
  checkpoint the run recorded first.
- **Permissions (P4-30):** a picker beside Start chooses the profile: Ask, Write or Full. It is
  kept in the project's settings, which also hold the tool rules (`docs/settings.md`). A call
  that needs the modder's permission waits in its thread, showing the tool, the paths it names
  and why it asks, with Allow once, Always allow and Deny. Frames go on meanwhile. Always allow
  keeps its rule in the project's settings.
- **Proposed changes:** every staged hunk, with Accept and Reject for each and for all of
  them (`edit.accept`, `edit.reject`).

`src/agent/tests.rs` covers it. Two threads whose models are held wait while frames go on,
and each ends when its model answers. A thread's hunk is rejected from the panel and its
checkpoint restores the project. The provider form keeps the key out of the file. A call
that asks waits in its thread for Allow once, Always allow or Deny, and the profile is kept in
the project's settings.

## Fonts

`assets/fonts/`: IBM Plex Sans 1.1.0 and JetBrains Mono 2.304 (OFL-1.1), Lucide 1.57.0's icon
font (ISC, with MIT for the icons it takes from Feather), and Noto Sans SC 2.004 (OFL-1.1),
the fallback for Chinese and Japanese text (ADR-0003). Each has its license file and a row in
`ATTRIBUTION.md`. A character none of them has is drawn in a system font that has it
(`src/fallback.rs`): the first time one appears in the editor, the system's fonts are listed
in the folders `qs_paths::current_font_dirs` names, and the first face whose character map
has it (read with `skrifa`, as egui reads fonts) is given to egui.

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
Shift+Cmd+Z `edit.redo`, which the editor takes itself while it has the keyboard; Cmd+1,
Cmd+2, Cmd+3 and Cmd+4 run `view.focus_editor`, `view.focus_left_panel`,
`view.focus_right_panel` and `view.focus_bottom_panel` from anywhere (Cmd+0 stays egui's zoom
reset). The palette shows each command's shortcut from it.

## Accessibility

Every area and control has an AccessKit name and role (QA-9, P2-23): the toolbar, the status
bar, each panel and the centre are named panes, section headings are headings, the icon-only
panel toggles are named for the panel they show and say whether it is shown, the palette is a
dialog whose rows are buttons named by their summaries, and the editor's text is a multiline
text input with its selection. A focus command gives a panel's first heading, or the bottom
panel's shown tab, the keyboard, showing the panel first, so Tab then walks the panel; the
editor's gives the editor the keyboard, or the shown centre tab when no file is open or
another tab is shown. The tests in
`tests.rs` query the tree with `egui_kittest`; `cargo xtask contrast` holds both themes to
WCAG AA. The recorded VoiceOver, NVDA and Orca passes are P4-24's.

## Strings

Every string the window shows is a `strings::Text` (CONV-16). The test
`the_ui_code_has_no_string_literals` fails on a string literal with words in it in the UI
modules, comments, attributes and log lines aside.

## Running it

```text
cargo run -p quakec-studio
```
