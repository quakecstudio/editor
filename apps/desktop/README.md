# quakec-studio

The desktop app (P2-17, ADR-0006): the window, its toolbar and its docked panels on egui
(eframe with the wgpu renderer, ADR-0003), themed from `docs/design/tokens.toml` with IBM Plex
Sans, JetBrains Mono and Lucide's icons (ADR-0010), and the macOS menu bar through `muda`.

- **Layer:** apps. It depends on `qs-layout`, `qs-workbench` and `qs-commands` (L5),
  `qs-editor-view` and `qs-render` (L7), `qs-log`, `qs-paths`, and for the map editor
  `qs-map`, `qs-bsp`, `qs-mdl` and `qs-vfs` (L0), `eframe`, `skrifa` for the system font
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

With no project open the Editor tab shows the Welcome page (`src/welcome.rs`, L-27): three
steps from a first launch to a running mod. The first says what was found of Quake's files and
opens the guide to them, the second opens the New Project dialog, a folder or a recent
project and says why one did not open, and the third says what Play does and its key; below
them are links to the command palette and the user guide. The game is looked for when the app
starts with nothing to open, and again when the folder chosen for Play changes.

New Project, in the File menu, the Project panel and the Welcome page, opens one dialog
(`src/new_project.rs`, L-28) that asks which game the mod is for, Original Quake or the 2021
rerelease, and then for the folder. The rerelease is chosen to begin with when it is the only
game found, the original otherwise.

The window is given the app's icon (`assets/icon.png`), which eframe would otherwise replace
with egui's; inside the macOS bundle the bundle's own icon is left in place.

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

## The map editor

A `.map`, or a shipped `.bsp`'s entities, opened from the Project tree or with `map.open`,
shows in the Map tab (`src/map_editor.rs`, `src/map_scene.rs`, P5-06): its brushes or faces as
lines, each entity as its class's box with its model's wireframe, picked with the mouse and
edited with TrenchBroom's keys, the selected entity's keys in the Properties panel. Each edit
runs a `map.*` command, so it undoes. [`docs/map-editor.md`](../../docs/map-editor.md) maps
TrenchBroom's bindings; `snapshots/map-editor.png` is its golden, and
`the_map_view_takes_trenchbrooms_keys` drives the keys end to end.

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

**External agents (P4-14, UI-11).** A thread can run on an external agent over the Agent Client
Protocol (`qs-acp`, `src/agent/external.rs`) instead of the built-in one: the agent is a
subprocess with its own model and account, so no provider is set up for it.

- **Setting one up:** with no agent set up the panel offers Use Claude Code, which adds Claude
  Code's ACP adapter, run by `npx` at a pinned version, to `agent.external` in the user's
  settings (it needs Node 22 or later and the `claude` command, signed in). Other agents are
  added there by hand; with more than one agent to run, a list beside the threads chooses.
- **What a thread shows:** what the agent says, drawn as Markdown (`src/markdown.rs`, L-43),
  and reasons as it streams, each tool call with its state, the agent's own permission options
  as buttons, Stop while it runs, and how the turn ended. Its map edits, made through the
  app's tools, apply at once with no review; Restore checkpoint puts the project's files and
  the open map back.
- **The IDE's tools** reach the agent as an MCP server: `quakec-ide mcp --connect`, beside the
  app.
- **Edits:** the IDE offers ACP's file methods. An agent that uses them reads the editor's text,
  unsaved changes included, and its writes are staged as hunks to accept or reject. Claude
  Code's adapter (0.109.0) does not use them: it reads and writes the disk itself. So every
  external thread records a checkpoint before the agent starts, and Restore checkpoint puts the
  project back.
- **What Does This Code Do?** in the editor's context menu, over a selection, starts a thread
  that asks the agent to explain the selected lines in plain language, and shows the panel.

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
- **What it shows:** the model's text as it is written, drawn as Markdown, each call with
  whether it ran, and how the run ended (done, or the reasons it is not). There is also a button that restores the
  checkpoint the run recorded first.
- **Permissions (P4-30):** a picker beside Start chooses the profile: Ask, Write or Full. It is
  kept in the project's settings, which also hold the tool rules (`docs/settings.md`). A call
  that needs the modder's permission waits in its thread, showing the tool, the paths it names
  and why it asks, with Allow once, Always allow and Deny. Frames go on meanwhile. Always allow
  keeps its rule in the project's settings.
- **Threads, as Zed's agent panel keeps them (P4-31):**
  - **Follow the agent:** while on, the editor opens the last file a reply's calls name.
  - **History:** a thread that ends is kept in `.quakec-studio/threads/` (`qs_agent::history`).
    History lists the kept threads, newest first, with a search; reopening one shows its
    messages, its checkpoint with Restore, and the hunks it staged that still wait.
  - **Edit and run again:** an ended thread's messages each have Edit; Run again from here
    goes on from the edited message, from a new checkpoint (`Run::resume`).
  - **Queue:** while a thread runs, a message typed in its box is read on its next turn.
  - **Usage and compaction:** each thread shows its estimated tokens. Near the model's context
    limit the run compacts the transcript into a summary the model writes, and the summaries
    are shown under their heading.
  - **Screenshots:** Attach screenshot gives the running game's frame (`engine.screenshot`)
    as an image, to the running thread or the next one started. Pasting an image is not
    supported: egui's paste event carries text only.
  - **Notification:** a thread that finishes asks for the modder's attention
    (`RequestUserAttention`).
- **Proposed changes:** every staged hunk, with Accept and Reject for each and for all of
  them (`edit.accept`, `edit.reject`).
- **@ mentions (P4-12, UI-8):** a task can name `@file:<path>`, `@symbol:<name>`,
  `@builtin:<name>`, `@entity` (the edict selected in the outliner or picked in the game) and
  `@error` (the console's last error). Each one is read when the thread starts and attached
  after the task (`qs_agent::expand_mentions`).
- **Ask about this (P4-12, UI-9):** right-clicking an outliner row, or the button in the
  Properties panel, selects the edict and starts a thread that explains it. The thread holds
  its classname, its current think function and every field (`qs_agent::EntityState::ask_about`).

`src/agent/tests.rs` covers it. Two threads whose models are held wait while frames go on,
and each ends when its model answers. A thread's hunk is rejected from the panel and its
checkpoint restores the project. The provider form keeps the key out of the file. A thread
started from an entity holds its state, and a task's mentions attach what they name. A call
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

`keymap.rs` binds shortcuts to commands that take no parameters, from `qs-layout`'s default
keys (`docs/keymap.md` lists them all): Cmd+Z runs `edit.undo` and Shift+Cmd+Z `edit.redo`,
which the editor takes itself while it has the keyboard; Cmd+1, Cmd+2, Cmd+3 and Cmd+4 run
`view.focus_editor`, `view.focus_left_panel`, `view.focus_right_panel` and
`view.focus_bottom_panel` from anywhere (Cmd+0 stays egui's zoom reset). The palette shows
each command's shortcut from it. The keys the map view takes itself (`qs_layout::VIEW_KEYS`:
the selection's, the grid's, the tools', the fly keys) are read from the same crate, and
while a pane of the view has the keyboard they come before a key taken anywhere.

**The command line (UI-23).** `command_line.rs` draws `qs_commands::line::CommandLine` in a
strip above the status bar, as AutoCAD's command window: Cmd+; brings the keyboard to it, a
command is typed by its id, its name or a start of either, after a `/` or not. As it is typed
the commands that match are listed above the line with what each does (L-44), the one Enter
would run marked: Down and Up move the mark, Tab completes to the marked command, and Enter
or a click runs it or asks for its parameters one at a time. Up from the top of the list
brings back earlier lines and Escape gives up. The status bar's error and warning counts are
a button that shows the Problems panel. While it asks for an entity, a brush, a face or a point (`pick.rs` reads which from the
parameter's schema), a click in the map view answers it.

While the game in the Game tab has the keyboard its keys are its own. On macOS the window
still takes the focus keys (Cmd+1 to Cmd+4 and Cmd+;), so the keyboard can leave the game;
where the platform's modifier is Ctrl those would be Quake's fire with a weapon's number, so
the window takes none and a click outside the game takes the keyboard.

**Custom keybindings (P4-37, UI-21).** As Zed's `keymap.json` does, `keymap.toml` in the app's
config directory rebinds any command that takes no parameters, in Zed's key spelling:
`secondary` is Cmd on macOS and Ctrl elsewhere, then `cmd`, `ctrl`, `alt` and `shift`, then one
key.

```toml
[[binding]]
keys = "secondary-shift-b"
command = "project.build"
```

A binding takes its keys from the default that had them, and a command's own shortcut, as the
palette shows it, is the user's. The file is read again the frame after it changes, with no
restart. Keys bound twice, an unknown command or one that needs parameters, and keys the
keymap does not know are each logged as a problem, and the rest of the file still applies
(`Keymap::problems`). Key sequences, such as `ctrl-k ctrl-c`, are not taken yet.

**Terminal and tasks (P4-36, UI-20).** The bottom panel's Terminal tab (`terminal.rs`) shows
the workbench's terminal tabs (`qs_workbench::terminal`): each tab's title and status, the
output of the one shown, and for a shell an input box whose line goes to `terminal.send`. New
terminal runs `terminal.new`. The palette lists the project's `[[task]]` tables (PRJ-2.10)
first, as Run task rows, and choosing one runs `task.run`. A tab opened by any caller is shown
at once, the bottom panel opened for it, and its status reads Running, Exited with its code, or
Ended by a signal. The processes run on pipes, not a pseudo-terminal. `fixtures/tasks/` is a
project with two tasks, one that fails, which the `terminal` snapshot runs.

**Quick fixes, inline diagnostics, inlay hints and formatting (P4-34, UI-18).** The editor
shows each problem of its file at its line's end (`line_notes`), a lightbulb before one a
quick fix may fix; a click on it, or Cmd+., lists the fixes (`code.actions`) in a popup and
choosing one applies it (`code.fix`). The parameter names before a call's arguments come from
`file.inlay_hints` when the file's analysis changes. Cmd+Shift+I runs `file.format`.

**Git (P4-35, UI-19).** The bottom panel's Git tab (`git_panel.rs`) shows the branch, to switch
or create one; each changed file with Stage and Unstage and its hunks, each staged or unstaged
on its own; a conflicted file's conflicts with Keep ours, Keep theirs and Keep both; the commit
message and Commit; and the open file's history and blame. It is read through the `git.*`
commands (`read_git`) when shown and after each action. The editor marks its file's lines
changed since `HEAD` in the gutter (`EditorView::set_gutter_base`, from `git.head_text`).

**Editor layout and navigation (P4-32, UI-16).** The editor's folds come from the syntax tree
(`qc_ide::AnalysisHost::folding_ranges`, through `OpenFile::folds`). The layout's commands
(`qs-layout`, `editor.rs`) are carried out here: soft wrap on the editor; the outline panel at
the editor's right, the open file's symbols with the caret's marked; a split's second pane
(`Shell::second_pane`), made from the open file, its text kept as the first pane's while both
show one file, and a click in it making it the pane being edited (`swap_panes`: its file is
opened in the workbench after the first pane's edits are sent); go to line, its prompt the
palette's when no line is given; and the go to symbol picker and the tab switcher
(`picker.rs`), drawn as the file finder is. Their keys are Zed's where Zed has one chord
(`keymap.rs`). `keystroke_split_fold_wrap` (`docs/perf-budgets.md`) holds the keystroke
budget with all three in use.

**Project search and the multibuffer (P4-33, UI-17).** The centre's Search tab
(`multibuffer.rs`) shows project search results, a name's references (Shift+F12) or the Problems
list (its Open as excerpts button) as excerpts of their files, each its place's whole lines with
a line of context, as Zed's Multibuffers page describes. `view.project_search` (Cmd+Shift+F)
opens it with the search box focused: match case, whole word and regular expression, and
Replace all, which runs `file.replace_all` once per file. An edit in an excerpt runs
`file.replace_range` on its file; Open goes to the file with the caret where it was in the
excerpt. An excerpt of a file with bytes that are not UTF-8 is shown read-only.

**Vim mode (P4-37).** `editor.key_mode = "vim"` in the user's settings
([`docs/settings.md`](../../docs/settings.md)) puts the code editor in `KeyMode::Vim`
([`qs-editor-view`](../../crates/qs-editor-view/README.md), `vim`), read when the window
starts.

**The update notice (P4-19, QA-11).** `update.rs` shows, in the status bar, "Update available:
<version>" once a check found a later release, and its window says how to get it with what
installed this copy (`qs_update::Install::channel`): the package manager's command with Copy
Command, or Download, which opens the release's page in the browser, as Release Notes does.
Skip This Version keeps the version as `updates.skip_version`. The app downloads and installs
nothing. A check is `qs-update`'s one request, made off the frame when Help, Check for Updates
runs (`help.check_for_updates`) and, only with `updates.check = "daily"`, as the app starts, at
most once a day ([`docs/privacy.md`](../../docs/privacy.md)); a failed background check is
logged and said nowhere else.

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

Every string the window shows is a `strings::Text` (CONV-16). The tes
