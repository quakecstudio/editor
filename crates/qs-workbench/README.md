# qs-workbench

The workbench (UI-2): the open project and the open file, what the Project, Symbols and
Problems panels list, and the commands that open a project or a file, `project.open` and
`file.open`.

- **Layer:** L5 commands. It depends on `qs-commands` (L5), `qc-ide` (L3), `qc-project` (L2),
  `schemars`, `serde` and `tracing`.
- **Decision:** plan requirement UI-2, P2-19 (#75); not engine behaviour.
- **Spec sections:** none; it implements a plan requirement, not engine behaviour. A
  diagnostic's line counts LF bytes only, as the lexer does (QC-1.2).
- **Shipped:** yes.

## Prior art

The model is **VS Code's and Zed's** workbench: the Explorer's tree, the Outline and the
Problems list read the language server's answers, and choosing a row opens the file at that
place through the same command a keybinding or the palette runs. Opening is view state, not
an edit, so both commands run outside the edit history
(`qs_commands::Outcome::outside_history`), as `qs-layout`'s do.

## Public shape

- `Workbench`: opens a project (`open_project`) and a file (`open_file`), and lists the
  project's files as a tree (`tree`), its problems with their lines and columns (`problems`),
  a file's outline (`outline`) and every symbol (`project_symbols`).
- `Location`, `Problem`, `TreeEntry` and `EntryKind`, `OpenFile` and `WorkbenchError`.
- `HasWorkbench` and `register`: the commands for any context with a `Workbench`.
- `OPEN_PROJECT`, `OPEN_FILE` and their parameter types, so the app runs them without
  spelling their ids.
