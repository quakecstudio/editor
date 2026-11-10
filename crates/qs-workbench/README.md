# qs-workbench

The workbench (UI-2): the open project and the open file, what the Project, Symbols and
Problems panels list, the commands that open a project or a file, `project.open` and
`file.open`, and those that build and check the project, `project.build` and `project.check`.

- **Layer:** L5 commands. It depends on `qs-commands` (L5), `qc-codegen` (L4), `qc-engines`,
  `qc-ide` and `qc-sema` (L3), `qc-project` (L2), `qc-progs` and `qs-diff` (L0), `schemars`,
  `serde` and `tracing`.
- **Decision:** plan requirement UI-2, P2-19 (#75); not engine behaviour.
- **Spec sections:** ENGT-3.12, the program a KEX rerelease target's build writes, which it
  refuses when KEX cannot load it; otherwise it implements a plan requirement, not engine
  behaviour. A diagnostic's line counts LF bytes only, as the lexer does (QC-1.2). QCT-1 to
  QCT-4 ([`qc-tests.md`](../../docs/spec/qc-tests.md)), a project's tests and `project.test`.
- **Shipped:** yes.

## Prior art

The model is **VS Code's and Zed's** workbench: the Explorer's tree, the Outline and the
Problems list read the language server's answers, and choosing a row opens the file at that
place through the same command a keybinding or the palette runs. Opening is view state, not
an edit, so both commands run outside the edit history
(`qs_commands::Outcome::outside_history`), as `qs-layout`'s do.

## Public shape

- `Workbench`: opens a project (`open_project`) and a file (`open_file`), and lists the
  project's files as a tree (`tree`), its problems with their lines and columns (`problems`)
  and every symbol (`project_symbols`), worked out when the project opens so a frame only
  reads them; `OpenFile` holds the open file's text, caret and outline.
- `Location`, `Problem`, `TreeEntry` and `EntryKind`, `OpenFile` and `WorkbenchError`.
- `build` and `check`: each module of the open project compiled with `qc-codegen` from the
  editor's text of each open file, its progs.dat written to its output and its .lno beside it
  (PRJ-3.2, LNO-1.4), a module with errors writing nothing; and the project's problems, as
  `ModuleBuilt` and `Reported` values the commands return.
- Tabs, as Zed keeps them: each file opened gets one (`tabs`), the most recent first in
  `history`; `close_file` and `file.close` close one, returning to the tab open before it
  (Zed's `activate_on_close: "history"`), its unsaved text kept; `is_unsaved` marks a file
  edited since it was saved; `OpenFile::symbol_at` names the definition a caret is in, for the
  breadcrumb.
- `RecentProjects` and `recent_path`: the projects opened most recently, kept in the data
  directory, for the Project section to offer before a project is open.
- The play loop and the debugger (`preview`, `debug`, `entities`): the preview host started
  on the project's program, its entities, the debugger's breakpoints, stack and watches, and
  the `preview.*` commands.
- Staged edits (`staged`, P4-04, AGT-3): `stage` and `edit.apply` take a change to several
  files and stage it as hunks, the runs of changed lines of `qs-diff`'s Myers diff. While a
  hunk waits, the semantic model and a build read the file with it, and nothing is written
  to disk; `decide`, `edit.accept` and `edit.reject` settle hunks one by one, a rejected hunk
  putting back its lines byte for byte. Once a file's hunks are all decided, the accepted ones
  are the editor's unsaved text, which `file.save` writes; with none accepted the file is
  exactly as it was. `edit.staged` lists the hunks waiting. The model is Zed's agent panel
  and Cursor's review: hunks shown in the buffer, kept or rejected each, the buffer the
  language server reads all along.
- Tests (`testing`, P4-26, QCT): `run_tests` and `project.test` build the project once, call
  each `test_` function alone in the VM over the mock engine as `vm.run` does, a run error
  such as `error("...")` failing it, then play each `tests/*.scenario` in `qs-sim` over the
  game folder given or the first install found, skipping them with no game; the result lists
  each test's kind, verdict and message, and the counts. The CLI's `quakec-ide test`, the
  palette and the agent run the same command.
- The agent's tools (`tools` and `engine`, P4-03, AGT-2): `project.symbols`, `engine.builtins`
  and `compile`, the project built with whether it is clean and its counts; and over the
  running game, `engine.inspect` (edicts, one edict's fields, the globals or the stack),
  `engine.console` (a command and the lines it printed) and `engine.screenshot` (the frame
  as a PNG, in `build/screenshots` by default). The engine tools are functions over the
  bridge's client, which `cargo xtask bridge-client` runs against the headless host in CI.
- `HasWorkbench` and `register`: the commands for any context with a `Workbench`.
- `OPEN_PROJECT`, `OPEN_FILE` and their parameter types, so the app runs them without
  spelling their ids.
