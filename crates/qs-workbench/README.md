# qs-workbench

The workbench (UI-2): the open project and the open file, what the Project, Symbols and
Problems panels list, the commands that open a project or a file, `project.open` and
`file.open`, and those that build and check the project, `project.build` and `project.check`.

- **Layer:** L5 commands. It depends on `qs-commands` (L5), `qc-codegen` (L4), `qc-engines`,
  `qc-ide` and `qc-sema` (L3), `qc-project` (L2), `qc-progs`, `qs-diff`, `qs-checkpoint`,
  `qs-pak`, `qs-vfs` and `qs-install` (L0), `schemars`, `serde` and `tracing`.
- **Decision:** plan requirement UI-2, P2-19 (#75); not engine behaviour.
- **Spec sections:** ENGT-3.12, the program a KEX rerelease target's build writes, which it
  refuses when KEX cannot load it; otherwise it implements a plan requirement, not engine
  behaviour. A diagnostic's line counts LF bytes only, as the lexer does (QC-1.2). QCT-1 to
  QCT-4 ([`qc-tests.md`](../../docs/spec/qc-tests.md)), a project's tests and `project.test`.
  PRJ-2.11, how a task runs. FIRE-4 and FIRE-5 ([`fire.md`](../../docs/spec/fire.md)),
  `map.fire` and `map.logic_issues`: an entity of the open map fired in `qs-sim` and the
  issues only running shows. BALANCE-2 to BALANCE-4 ([`balance.md`](../../docs/spec/balance.md)),
  `map.balance`: the open map's balance sheets on each skill, read from its spawn in `qs-sim`.
  BUDGET-1 to BUDGET-3 ([`budget.md`](../../docs/spec/budget.md)), `map.budget`: how much of the
  target engine's limits (ENGT-5) the open map uses, from the last compile and a spawn with room
  past id's limits.
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
- Staged edits (`staged`, P4-04, AGT-3): `stage` and `edit.stage_change` take a change to several
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
- Formatting and quick fixes (P4-34, UI-18): `file.format` formats a file's text, which the
  editor takes, unsaved; `code.actions` lists the quick fixes at a place and `code.fix` applies
  one; `file.inlay_hints` lists each argument's parameter name.
- Project search and replace (`search`, P4-33, UI-17): `project.search` takes matching case,
  whole words and regular expressions and answers each match's byte range; `file.replace_all`
  replaces every match in one file and saves it, one undoable step per file, so replacing across
  a project is the command once per file; `file.replace_range` changes one range of a file's
  text, unsaved, as an edit in a multibuffer excerpt does.
- The agent reference (`reference`, L-45, L-46, AGT-9): `project.reference` answers one topic
  of `docs/agent-reference/` (the language in short and in each dialect, the engine's entity
  model, monsters, weapons, items, triggers, models, maps, the workflow), so an agent that reaches the IDE over MCP reads what the built-in agent's prompt
  is built from.
- Entity definitions (`entity_classes`, P5-03, MAP-1): `project.entities` lists the entity
  classes the project's server QuakeC defines, read from its spawn functions by `qc-sema`, and
  with `fgd` writes them as an FGD file for other map editors
  (`quakec-ide run project.entities '{"fgd": "mod.fgd"}' --project <dir>`).
- Map diagnostics (`map_check`, P5-04, MAP-8): `map.check` gives a `.map`'s diagnostics, its
  format's (`qs-map`) and its entities' against the project's server QuakeC, each unknown
  classname or key with the name to write instead; `map.rename_key`, run once the person
  confirms the caution `symbol.rename` gives for a field, renames the key in every open map,
  unsaved and undoable.
- The open map is `qs-map-edit`'s (P6-40): the workbench holds its `MapStore` and answers the
  map commands' seam from the server QuakeC and the preview. `files.changed` takes the files
  another program wrote: the open map is read again, or with unsaved edits kept for
  `map.resolve_disk_change` and `map.compare_with_disk`, and changed QuakeC is read again (P5-17, MAP-32).
- Git (`git`, P4-35, UI-19): `git.status`, `git.diff`, `git.stage` and `git.unstage` (a file
  or one hunk), `git.commit`, `git.branches`, `git.switch`, `git.create_branch`, `git.log`,
  `git.blame`, `git.conflicts`, `git.resolve` and `git.head_text`, over `qs-git` in the open
  project's repository. A switch is refused while the editor has unsaved edits, and reads the
  open tabs and the project again, as a restored checkpoint does; a resolved conflict is
  written and saved.
- Terminal and tasks (`terminal`, P4-36, UI-20): `task.list` and `task.run` list and run the
  project's `[[task]]` tables (PRJ-2.10), each by the user's shell in the project directory
  (PRJ-2.11), and `terminal.new`, `terminal.send`, `terminal.poll` and `terminal.close` open a
  shell, write a line to it, take every tab's output and exit without waiting, and close a tab,
  ending its process. The tabs (`Terminals`) live in the workbench; each keeps its last MiB
  of output. The processes run on pipes, not a pseudo-terminal, as a first terminal.
- Export (`export`, P4-28, QA-15): `project.export` builds the project and writes a zip
  players install, `build/<mod>.zip` by default, holding the mod folder. That folder has the
  programs from `build/<mod>/`, the project's own assets under `ASSET_DIRS` (no `.map`
  sources), loose or packed into `pak0.pak`, a generated `README.txt` (the game the mod needs
  and how to install it), and the license and notice files at the project's top. It never
  follows a link, so the run roots under `build/run/` (ADR-0008) are never read. It refuses a
  file that is id's: one in `FS_KNOWN_FILES` (FS-6.2), or one whose bytes are a member of a
  PAK in the game folder's `id1` or `rerelease/id1`. It lists every asset the project's
  attribution file does not name. The same sources export to the same zip.
- The agent's tools (`tools` and `engine`, P4-03, AGT-2): `project.symbols`, `project.files`,
  `project.search` (every match of a text, unsaved edits included), `engine.builtins`
  and `project.compile`, the project built with whether it is clean and its counts; and over the
  running game, `engine.inspect` (edicts, one edict's fields, the globals or the stack),
  `engine.console` (a command and the lines it printed) and `engine.screenshot` (the frame
  as a PNG, in `build/screenshots` by default). The engine tools are functions over the
  bridge's client, which `cargo xtask bridge-client` runs against the headless host in CI.
- Checkpoints (`checkpoints`, P4-05, AGT-4): `checkpoint.create` records every file of the
  project as git objects on a private ref (`qs-checkpoint`), `checkpoint.list` lists them, and
  `checkpoint.restore` puts every file back, deletions included, dropping the editor's
  unsaved text and staged hunks so the IDE shows the checkpoint.
- `HasWorkbench` and `register`: the commands for any context with a `Workbench`. Such a
  context also has the open map (`qs_map_edit::HasMap`): its `MapStore` is the workbench's
  (`Workbench::maps_mut`). The workbench answers the map seam (`MapProject` from the server
  QuakeC, `LiveMap` from `Preview`), and `SendLive` sends a `LiveChange` over the bridge.
- `OPEN_PROJECT`, `OPEN_FILE` and their parameter types, so the app runs them without
  spelling their ids.
