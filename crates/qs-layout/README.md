# qs-layout

The workbench layout (UI-2): which of the left, right and bottom panels are shown and how big
each is, kept in `layout.toml` in the app's data directory (`qs_paths::AppDirs::data`), and the
commands that change it, `view.toggle_panel` and `view.resize_panel`.

- **Layer:** L5 commands. It depends on `qs-commands`, `schemars`, `serde` and `toml`.
- **Decision:** plan requirement UI-2, P2-17 (#73); not engine behaviour.
- **Spec sections:** none; it implements a plan requirement, not engine behaviour.
- **Shipped:** yes.

## Prior art

The model is **Zed's and VS Code's** workbench: a panel's visibility and size are view state,
saved between runs, and changed by commands a keybinding or the palette can run. Neither puts a
hidden panel in the edit history, so both commands here run outside it
(`qs_commands::Outcome::outside_history`): `edit.undo` passes them by.

## Public shape

- `Layout`, `PanelState` and `Panel`: the three panels' visibility and size; `Layout::read` and
  `Layout::write` for the file, the default when there is none.
- `HasLayout` and `register`: the commands for any context with a `Layout`.
- `TOGGLE_PANEL`, `RESIZE_PANEL` and their parameter types, so the app runs them without
  spelling their ids.
