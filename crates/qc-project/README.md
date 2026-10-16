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
errors in a directive's form, is the preprocessor's work (`qc-preproc`).

Written from the spec, not ported. Output paths follow
[ADR-0008](../../docs/adr/0008-game-directories.md): `build/<mod>/progs.dat` and so on, in the
project.

- **Layer:** L2 syntax. It depends on `qc-lexer` (diagnostic codes) and `qc-preproc` (the
  file loader).
- **Spec sections:** PRJ-1 to PRJ-6 in [`project-file.md`](../../docs/spec/project-file.md);
  QC-23 in [`language.md`](../../docs/spec/language.md); QCD-5.1, QCD-5.2, QCD-5.6 to QCD-5.8
  and QCD-8.4 in [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md).
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
spelt `Animations.h`.
