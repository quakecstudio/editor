# qc-project

QuakeC projects. A project is a set of modules (server, client and menu QuakeC), each a
separate program with its own `progs.src`, dialect, target engine and output path. It is
declared in `quakec.toml`, or opened bare from a directory or a `progs.src`. Each module's
file graph is the ordered list of files it compiles, with how each came in and which file
brought it in; the semantic model builds one symbol table per module on top of it.

The graph reads id's list form of `progs.src` and FTEQCC's source form (a `progs.src` that is
itself QuakeC with `#includelist`), and follows only the directives that decide which files
are read: `#include`, `#includelist`, the conditionals, `#define` and `#undef` for them, and
the pragmas `sourcefile`, `progs_dat`, `includedir` and `once`. Everything else, and the
errors in a directive's form, is the preprocessor's work (`qc-preproc`). Under the FTEQCC
dialect each QuakeC file is read with the preprocessor's directive reader
(`qc_preproc::FteFileReader`), so the graph meets the directives where FTEQCC meets them
(PRJ-5.13, PRJ-5.14), and under GMQCC with the same reader, as the preprocessor reads a GMQCC
file, with no `FTEQCC` defined (PRJ-5.15).

Files open as the host file system opens them, as FTEQCC and id's `qcc` do, and a name whose
case differs from the file it opened is warning W0402 on every OS, so a project that would
fail on a case-sensitive file system says so on macOS and Windows too (PRJ-7).

Written from the spec, not ported. Output paths follow
[ADR-0008](../../docs/adr/0008-game-directories.md): `build/<mod>/progs.dat` and so on, in the
project.

- **Layer:** L2 syntax. It depends on `qc-lexer` (diagnostic codes) and `qc-preproc` (the
  file loader and the FTEQCC directive reader).
- **Spec sections:** PRJ-1 to PRJ-7 in [`project-file.md`](../../docs/spec/project-file.md);
  the directive codes QCD-8 of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md). A readiness run
  attributes only these sections to the crate, and every active statement of each is cited by a
  test here.
- **Also cited here, in sections another crate completes:** the `progs.src` reading of QC-23.1
  to QC-23.3, QC-23.5 and QC-23.6 in [`language.md`](../../docs/spec/language.md) (QC-23.4, the
  output path relative to the working directory, is id's `qcc`; the graph resolves it against
  the `progs.src` directory, PRJ-4.3); the project error codes QCD-5.1, QCD-5.2 and QCD-5.6 to
  QCD-5.8; and the case warning QCD-7.16. The compiler-stage errors QCD-5.3 to QCD-5.5 and the
  resolution warnings of QCD-7 belong to the compiler and the semantic model, so QC-23, QCD-5
  and QCD-7 are not declared above.
- **Shipped:** yes.

## Invariants

Each has a test in `src/tests/`:

- Building a graph from any bytes ends, every entry's parent comes before it, and no graph
  holds more than `PRJ_MAX_FILES` entries (property tests in `properties.rs`, and the
  `qc-project` fuzz target).
- A list-form `progs.src` with no directives gives its files in the order listed.

`cargo xtask project-corpus` opens corpus codebases and checks every module's graph. In CI it
runs on id 1.01, where it also checks the graph against the order of the functions in the
`progs.dat` id's `qcc` compiles, on the rerelease, Copper and Arcane Dimensions, on Xonotic
through the `quakec.toml` in `corpus/projects/xonotic/`, and on macOS on FreeCS in its Nuclide
SDK tree (`cargo xtask corpus fetch freecs-tree`), whose `#include "animations.h"` names a file
spelt `Animations.h`: `--expect-warning W0402=2` checks that its client and server modules get
the warning.
