# qs-editor-view

The code editor widget (UI-4, P2-27): the UI spike's egui editor
([`spikes/ui-egui`](../../spikes/ui-egui/src/main.rs), on the
[`spikes/ui-common`](../../spikes/ui-common/src/document.rs) document) grown into a shipped
crate. P2-18 builds the code editor on it: the rope buffer, semantic highlighting through the
`Styler` hook, find and replace, the inline diff and the language features.

- **Layer:** L7 UI. It depends on `egui` alone; `eframe` is a dev-dependency of the benchmark.
- **Decision:** ADR-0003 (egui with a custom editor widget); not engine behaviour.
- **Spec sections:** none; it implements plan requirements UI-4, PRIN-5 and QA-9, not engine
  behaviour.
- **Shipped:** yes.

## Prior art

The model is **Zed's and VS Code's** editor: an editor is its own widget, not a text box. Only
the visible lines are laid out and drawn, each line's layout is cached and relaid out only when
the line changes, and every caret is an equal selection with an anchor and a head. The keys
follow VS Code's: Cmd or Ctrl with Alt and an arrow adds a caret above or below, Escape keeps
the primary one, Alt-click adds one, and Shift extends a selection.

## What it does

- **`Buffer`**: the text as lines, each with a `LineId`. An edit gives the lines it changes new
  ids and leaves every other line's alone, even when it moves.
- **`EditState`**: the buffer and its selections, sorted and merged, one of them the primary.
  Movements by character, line, page and to the text's ends keep the column a run of vertical
  moves started at. Typing, pasting, a new line (keeping the indent), Backspace and Delete edit
  at every selection, from the last up, moving the carets below for what each edit added.
- **`LayoutCache` and `Styler`**: each visible line is laid out once, keyed by its `LineId`,
  until the styler's revision, the font or the scale changes; lines not drawn in a frame are
  dropped.
- **`EditorView`**: draws the visible lines, the current line, the selections, the carets and
  the line numbers in the theme's colours (`ViewStyle`), and turns egui's keyboard, pointer,
  wheel, clipboard and IME events into edits.
  - IME composition is drawn underlined at the primary caret, outside the cache, and the
    candidate window is placed at the caret after it (`PlatformOutput::ime`).
  - The AccessKit tree has the editor as a multi-line text input whose children are one text
    run per visible line, with each character's length, and the primary selection between the
    runs it lies on. eframe's `accesskit` feature, which delivers the tree to a screen reader,
    is P2-23 (#79).

## Measurements

`cargo run --release -p qs-editor-view --example bench -- <out-dir> [host]` replays the
spike's script as egui events into a 50,000-line file and writes the frame times. On the
reference Mac the p99 is 2.24 ms or less in every phase against the 8.33 ms bar
([`docs/spikes/data/ref-mac/ref-mac-editor-view.md`](../../docs/spikes/data/ref-mac/ref-mac-editor-view.md)).

## Tests

`src/tests.rs`: the buffer and the selections directly, and the view in a headless egui context
with the bundled code font: keys, clicks, the wheel, the clipboard, IME preedit, commit and
surrounding deletes, what the layout cache relays out after each kind of edit, and the AccessKit
nodes.
