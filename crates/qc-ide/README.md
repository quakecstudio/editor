# qc-ide

The editor features of QuakeC Studio over the semantic model, with no protocol: go-to-definition,
find-references, rename, hover (with the engine-target database's entry for a builtin),
completion, semantic highlighting, parameter-name inlay hints and diagnostics (LANG-7). The
language server ([`apps/lsp`](../../apps/lsp/README.md)) translates these answers to the
Language Server Protocol, and the desktop editor will read them in process, so a modder gets the
same answers in VS Code, Zed or Neovim as in QuakeC Studio (LANG-9).

This follows rust-analyzer's split of its `ide` crate, which answers questions in its own terms
over the semantic database, from the `rust-analyzer` binary, which owns the protocol, the main
loop and the conversion of offsets to lines and columns. Here every answer is in byte offsets
and paths, and lines, columns and encodings are the front end's.

- **Layer:** L3 semantics. It depends on `qc-sema` and `qc-engines` (L3), `qc-syntax` and
  `qc-project` (L2) and `qc-diag` (L0).
- **Spec sections:** none of its own. It reads the resolution of
  [`language.md`](../../docs/spec/language.md) QC-11 and QC-12 (FTE-22 and FTEF-17 under FTEQCC)
  through `qc-sema`, and the engine tables of
  [`engine-targets.md`](../../docs/spec/engine-targets.md) through `qc-engines`; its tests cite
  the statements its choices rest on.
- **Shipped:** yes.

## How it works

An `AnalysisHost` opens a project directory with `qc_project::open_dir` and keeps one
incremental `qc_sema::Db` per module, since each module compiles its own files in its own order
(QC-23.3). A file several modules compile, such as a shared `defs.qc`, is in each of their
databases, and a definition in it is the same definition in each: its references and its rename
span every such module. The text of a file the editor has open (`set_overlay`) replaces its text
on disk in every module; editing a `quakec.toml` or a `.src` file opens the project again, and a
file no module compiles is analysed as a module of its own while it is open. A module's analysis,
its files' trees as written, its declared builtins and its `W0311` warnings are derived once per
edit.

| Feature | Method | What it reads |
| --- | --- | --- |
| Go-to-definition | `definition` | `ModuleIndex::definition_at`, a later declaration going to the first (QC-11.5) |
| Find-references | `references` | `ModuleIndex::references`, across every module that compiles the definition's file |
| Rename | `prepare_rename`, `rename` | Every declaration and use, a vector's components with it (QC-11.7); refused through a macro (QC-9.7), and unless the edited modules resolve to the same definitions, uses and diagnostics |
| Hover | `hover` | The declaration as written, its comment, and for a builtin the target engine's entry and every engine's (ENGT-3.9, ENGT-3.11) |
| Completion | `completions` | The globals, and the enclosing function's parameters and earlier locals (QC-12.1); only fields after `.` or `->` (QC-16.6) |
| Highlighting | `highlights` | Each resolved name by what it names, with declaration, constant (QC-11.6, vanilla) and builtin modifiers |
| Inlay hints | `parameter_hints` | The parameter name of each argument (QC-13.5), from the declaration that names them (QC-13.2) |
| Diagnostics | `diagnostics`, `all_diagnostics` | The project graph's, the preprocessor's and parser's, the semantic model's, and `W0311` for the target engine |

A position just after a name names it, as an editor's cursor at the end of a word does.

## What it is not, yet

- Completion after `.` offers fields only: an FTEQCC vector's `.x`, `.y` and `.z` (FTE-9.1) and a
  class's members need the type of what the access is made on, which the semantic model does not
  compute yet.
- A new `#include` line takes effect when the project is read again (`reload`, which the language
  server calls on save), not as it is typed.

## Tests

The tests in `src/tests/` build each project in memory through one factory (`ProjectSpec`) and
cite the statement each pins. The language server's tests drive every feature through the
protocol, and `cargo xtask lsp-corpus` drives it against the rerelease.
