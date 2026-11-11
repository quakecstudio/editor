# quakec-ide

QuakeC Studio's command line (GOAL-2, GOAL-4): it builds and checks a QuakeC project and runs
any command of the IDE by its id. The binary is `quakec-ide`
([ADR-0006](../../docs/adr/0006-product-name.md)).

- **Layer:** apps. It depends on `qs-commands` and `qs-workbench` (L5): every subcommand runs
  a command of the registry the window, the palette and the agent use (PRIN-3), so the
  command line does nothing the IDE does not. It reads its arguments with `clap`, which also
  prints `--help`, each subcommand's help and `--version`, and says what is wrong with a
  command line.
- **Spec sections:** none of `docs/spec/`; the build is `qs-workbench`'s `project.build`
  (PRJ-3.2, LNO-1.4).
- **Shipped:** yes.

## Usage

```text
quakec-ide build [<project>] [--dialect <vanilla|fteqcc>]
quakec-ide check [<project>] [--dialect <vanilla|fteqcc>]
quakec-ide test [<project>] [--dialect <vanilla|fteqcc>] [--filter <text>] [--game <folder>]
quakec-ide mcp [<project>] [--dialect <vanilla|fteqcc>] [--read <dir>]... [--write <dir>]...
               [--read-only] [--game <folder>]
quakec-ide run <command> [<json>] [--project <project> [--dialect <vanilla|fteqcc>]]
quakec-ide commands
quakec-ide doctor [--check <games|host|keychain>]... [--game <folder>] [--require-game]
```

| Subcommand | Commands it runs | Prints |
| --- | --- | --- |
| `build` | `project.open`, `project.build` | each error and warning as `<path>:<line>:<column>: <error\|warning> <code>: <message>`, then where each module was written |
| `check` | `project.open`, `project.check` | each problem the same way, then the counts |
| `test` | `project.open`, `project.test` (QCT-4.3) | each test as `passed`, `FAILED` or `skipped` and its name, the message of one that failed or was skipped indented below it, then the counts |
| `mcp` | `project.open`, then the agent's tools as MCP clients call them ([`docs/mcp.md`](../../docs/mcp.md)) | nothing but MCP messages, one per line, until stdin ends |
| `run` | `project.open` when `--project` is given, then `<command>` with `<json>`, `{}` when absent | what the command returned, as JSON |
| `commands` | `commands.list` | every command's id, summary and parameter schema, as JSON |
| `doctor` | none: it checks the install itself (P4-18) | a line per check, `ok` or `FAIL`: the games found (failing on none with `--require-game`), the preview host beside it started headless on `--game` or the first install and stopped over its bridge, and a keychain entry written, read back and deleted; exit 1 when a check fails. The release's install tests run it from each package ([`docs/releasing.md`](../../docs/releasing.md)) |

The project is the current directory when none is named, and a directory with no
`quakec.toml`, `progs.src` or `csprogs.src` is refused. `--dialect` reads a project with no
`quakec.toml` in that dialect (PRJ-2.8); it is vanilla otherwise, so the rerelease builds with
`quakec-ide build <rerelease> --dialect fteqcc`.

The exit status is 0 when the build wrote every module, the check found no error, no test
failed or the command ran; 1 when the build or the check found errors or a test failed; and 2
for a wrong command line, a directory with no project, a project that did not build for its
tests, or a command that failed.

## Tests

`src/tests.rs` reads every command line and refuses the wrong ones, builds and checks a small
project on disk (a clean one, one with an error printed at its line and column, one that needs
`--dialect fteqcc`), runs a project's tests and checks `run project.test` answers the same,
runs a command by id and lists the commands. The `Test Assets` CI job
builds the rerelease with it on Linux, macOS and Windows, with no other tool installed.
