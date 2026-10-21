# Projects and the project file

Prefix: `PRJ` (registered in [`README.md`](README.md)). IDs follow the rules there: never
renumbered, never reused, withdrawn rather than deleted.

Describes what a QuakeC Studio project is: the `quakec.toml` file that declares its modules
(server QuakeC, client QuakeC and menu QuakeC, LANG-3 and LANG-10), what is opened when there
is no `quakec.toml`, and how each module's ordered file graph is built from its `progs.src`.
Implemented by `qc-project`.

The project file is this project's own definition; no compiler or engine reads it. The
`progs.src` list form is id's (QC-23 in [`language.md`](language.md), from `qcc/qcc.c`, `main`
and `qcc/cmdlib.c`, `COM_Parse` in
[id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools) at commit
`c0d1b91c74eb654365ac7755bc837e497caaca73`). The source form, `#includelist`, the pragmas and
`#if` are FTEQCC's, read on 2026-10-16 from `engine/qclib/qccmain.c` (`QCC_main`,
`QCC_ContinueCompile`, `QCC_RegisterSourceFile`), `engine/qclib/qcc_pr_lex.c`
(`QCC_PR_Precompiler`, `ParsePrecompilerIf`, `QCC_PR_FalsePreProcessorIf`,
`QCC_FindBestInclude`, `QCC_JoinPaths`, `QCC_PR_SimpleGetToken`),
`engine/qclib/qcc_cmdlib.c` (`QCC_COM_Parse`) and `engine/qclib/qcctui.c` (`QCC_FileSize`,
`QCC_ReadFile`) of [fte-team/fteqw](https://github.com/fte-team/fteqw)
at commit `f937b9d88f71fc4429db5fe56c6a98d922711b2e`, the commit QC-9 cites. FTEQCC is not a
registered upstream ([`../porting.md`](../porting.md#upstreams)), so nothing here is ported
from it: these statements are this project's rules, written to agree with FTEQCC on every
`progs.src` in the corpus. Output paths follow
[ADR-0008](../adr/0008-game-directories.md).

Paths in this file are relative to the project directory unless a statement says otherwise.
"Resolved against a directory" means joined to it, with `.` components removed and each `..`
folded into the name before it, so one file has one path.

## PRJ-1 Opening a project

**Source:** this project. Prior art: Cargo opens the directory holding `Cargo.toml`; FTEQCC
compiles `progs.src` in its source directory when given no file (`QCC_main`).

### Behaviour

- **PRJ-1.1** Opening a directory that holds `quakec.toml` opens that file (PRJ-2), and the
  directory is the project directory. Opening a `quakec.toml` file directly does the same.
- **PRJ-1.2** Opening a directory without `quakec.toml` opens a bare project: its modules are
  the directory's `progs.src`, then its `csprogs.src` if there is one, then every source file
  either registers with `#pragma sourcefile` (PRJ-5.7), each read once in that order. A
  directory with neither `quakec.toml` nor `progs.src` is an error and opens nothing.
- **PRJ-1.3** Opening any other file opens a bare project of that file and the source files
  it registers, with the file's directory as the project directory.
- **PRJ-1.4** A bare project's name is the project directory's name (`project` when it has
  none), its mod name (PRJ-2.3) is that name when it is a valid mod name and `mod`
  otherwise, its dialect is `vanilla` and its target is `quake`. Each module's name is its
  source's path relative to the project directory, with `/` between components and without
  the extension when that is `.src`; its kind comes from its declared output (PRJ-3.3), and
  it has no defines and no include directories.
- **PRJ-1.5** In a bare project, a file in source form (PRJ-4.1) that compiles no file but
  itself, declares no output and registers at least one source file is a list of programs,
  as FreeCS's top-level `progs.src` is: it is not a module, and the files it registers are.

## PRJ-2 quakec.toml

**Source:** this project; the format is [TOML 1.0](https://toml.io/en/v1.0.0).

```toml
[project]
name = "Copper"            # optional; default: the project directory's name
mod = "copper"             # optional; default as for a bare project (PRJ-1.4)
dialect = "fteqcc"         # optional; vanilla (default), fteqcc or gmqcc
target = "quake"           # optional; quake (default), quakespasm, ironwail, vkquake,
                           # rerelease, fte or dp

[[module]]
kind = "ssqc"              # required; ssqc, csqc or menuqc
src = "progs.src"          # required; the module's progs.src, or a root source file
name = "server"            # optional; default: the kind
dialect = "fteqcc"         # optional; default: the project's
target = "fte"             # optional; default: the project's
output = "build/copper/progs.dat"  # optional; default: PRJ-3.2
defines = ["CSQC", "VERSION=2"]    # optional; macros defined before the first file
include_dirs = ["lib"]             # optional; searched after the including file's directory
```

### Behaviour

- **PRJ-2.1** `quakec.toml` holds an optional `[project]` table and one or more
  `[[module]]` tables with the keys above and no others. A key that is not listed, a value of
  the wrong type, a value outside its list, a file that is not valid TOML or a file with no
  `[[module]]` is an error that names the file, and the project does not open.
- **PRJ-2.2** **Withdrawn** (P2-29, #407): it allowed only four targets. Replaced by PRJ-2.8
  for `dialect` and PRJ-2.9 for `target`.
- **PRJ-2.3** The mod name is the single directory name ADR-0008 puts the build output in.
  It is valid when it is not empty, is not `.` or `..`, and holds no `/`, `\` or `:`. An
  invalid `mod` is an error.
- **PRJ-2.4** Module names are unique within a project; a repeated name is an error. A
  module without `name` is named after its kind.
- **PRJ-2.5** `src`, `output` and each `include_dirs` entry are relative paths resolved
  against the project directory. An absolute `src`, `output` or include directory (one with
  a root, such as `/x`, or on Windows a drive), or an `output` with a `..` component, is an error, so build
  output never leaves the project.
- **PRJ-2.6** Each `defines` entry is `NAME` or `NAME=VALUE`, where `NAME` is a QuakeC name
  (QC-3.1); `NAME` alone defines the macro with an empty body. Any other entry is an error.
- **PRJ-2.7** A module's `#pragma sourcefile` lines (PRJ-5.7) add no modules: the modules of
  a project with `quakec.toml` are exactly those it declares.
- **PRJ-2.8** `dialect` is one of `vanilla`, `fteqcc` and `gmqcc`. A module without its own
  `dialect` takes the project's.
- **PRJ-2.9** `target` is one of `quake` (id's engine, which the preview host follows),
  `quakespasm`, `ironwail`, `vkquake`, `rerelease` (the 2021 KEX engine, a compile target
  only, ENG-8), `fte` and `dp`; each selects an engine of the engine-target database
  (`engine-targets.md`, ENGT-3.5 and ENGT-3.6). A module without its own `target` takes the
  project's.

## PRJ-3 Modules and output paths

**Source:** this project and [ADR-0008](../adr/0008-game-directories.md), "Build output". The
output file names are the ones FTE loads for each module, from the corpus: `progs.dat`,
`csprogs.dat` and `menu.dat`.

### Behaviour

- **PRJ-3.1** A module is one program: a kind (`ssqc`, server QuakeC; `csqc`, client
  QuakeC; `menuqc`, menu QuakeC), a source (`src`), a dialect, a target, an output path and
  its file graph (PRJ-6). Modules are separate programs, each with its own symbol table
  (LANG-3): a macro, include directory or `#pragma once` of one module does not carry into
  another.
- **PRJ-3.2** A module's output path is its `output` when `quakec.toml` gives one, and
  otherwise `build/<mod>/<file>` with the file name of its kind: `PRJ_SSQC_OUTPUT`,
  `PRJ_CSQC_OUTPUT` or `PRJ_MENUQC_OUTPUT`. The output the `progs.src` itself declares
  (PRJ-4.3, PRJ-5.6) is kept as the module's declared output and does not change where the
  IDE writes (ADR-0008: nothing is written outside the project).
- **PRJ-3.3** In a bare project a module's kind is `csqc` when the file name of its declared
  output is `csprogs.dat`, `menuqc` when it is `menu.dat`, compared without regard to ASCII
  case, and `ssqc` otherwise, including when it declares none.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PRJ_MANIFEST` | `quakec.toml` | this project |
| `PRJ_SSQC_OUTPUT` | `progs.dat` | id's engine, `PR_LoadProgs` in `WinQuake/pr_edict.c` |
| `PRJ_CSQC_OUTPUT` | `csprogs.dat` | FTE, as the corpus's client QuakeC names it |
| `PRJ_MENUQC_OUTPUT` | `menu.dat` | FTE, as the corpus's menu QuakeC names it |
| `PRJ_BUILD_DIR` | `build` | ADR-0008, "Build output" |

## PRJ-4 The two forms of progs.src

**Source:** `qcc/qcc.c`, `main`, and `qcc/cmdlib.c`, `COM_Parse` (Quake-Tools); FTEQCC
`QCC_main` (the `*qcc_token == '#'` test), `QCC_ContinueCompile` and `QCC_COM_Parse`.

### Behaviour

- **PRJ-4.1** A module's source is read in list form or source form, decided by its first
  token as QC-23.2 splits tokens, with `/* */` comments also skipped: source form when the
  first token starts with `#`, list form otherwise. The file's extension does not matter.
- **PRJ-4.2** In list form the tokens are split as QC-23.2 says, and in addition `/* */`
  comments are skipped and a line whose first non-blank byte is `#` followed by a name is a
  directive (PRJ-5), not tokens. Neither changes a list id's `qcc` accepts: no such list has
  a `/*` or a line starting `#` and a name, because each would be a file name it cannot open.
- **PRJ-4.3** In list form the first token is the declared output, resolved against the
  directory of the `progs.src`, and every later token is a source file, resolved against the
  same directory and compiled in the order listed (QC-23.3). The `progs.src` itself is not
  compiled. A list with no token is error `E0401` (QCD-5.1).
- **PRJ-4.4** In source form the file is itself the first file compiled: its text is
  QuakeC, and the files it brings in with `#include` and `#includelist` are compiled where
  they appear. It declares its output with `#output` or `#pragma progs_dat` (PRJ-5.6), or
  not at all.
- **PRJ-4.5** A module's source that cannot be read is error `E0402` (QCD-5.2) and the
  module has no files.

## PRJ-5 Directives the graph follows

**Source:** FTEQCC `QCC_PR_Precompiler`, `ParsePrecompilerIf`, `QCC_PR_FalsePreProcessorIf`,
`QCC_FindBestInclude`, `QCC_JoinPaths` and `QCC_PR_SimpleGetToken`; QC-9 for the directives
both share.

Only the directives that decide which files are read are followed here. Every other
directive, and the macro expansion of ordinary text, is the preprocessor's (QC-9) and the
dialect profiles' work, and so are the errors in a directive's form: the graph skips a
malformed directive without a diagnostic.

### Behaviour

- **PRJ-5.1** A directive is a line, outside any `/* */` comment, whose first non-blank
  byte is `#` followed, after optional blanks, by a name (QC-9.1). A line ending in `\`
  continues on the next line. Directive names are compared exactly; pragma names (PRJ-5.6 to
  PRJ-5.9) without regard to ASCII case. Text in string literals and comments never starts a
  directive or a comment. A UTF-8 byte order mark at the start of a file is skipped, as
  FTEQCC's `QCC_LoadFile` (`qcc_cmdlib.c`) skips it.
- **PRJ-5.2** `#include "path"` and `#include <path>` compile the named file at that point.
  The path is resolved against the directory of the file holding the directive, then against
  each include directory in order (the module's `include_dirs`, then those added by
  `#pragma includedir`); the first that names a readable file is used. None is error `E0111`
  (QCD-8.4), and the line is skipped.
- **PRJ-5.3** `#includelist` starts a list that runs to a line `#endlist`. Each word of the
  lines between is a file, found as `#include` finds one and compiled in order. Words are
  split at blanks and at `,`, `;`, `(`, `)` and `]`, `//` ends the line, a pair of `"` around
  a word is removed, and directive lines inside the list are followed (so `#ifdef` can skip
  entries). A word that names no readable file is error `E0402` (QCD-5.2). A file that ends
  inside a list is error `E0406` (QCD-5.6), and the list ends there.
- **PRJ-5.4** `#define NAME text` and `#undef NAME` define and remove macros for the
  conditionals; a function-like `#define` defines its name. Macros from a module's `defines`
  are defined first, and macros run on across the files of the module in compilation order
  (QC-9.8).
- **PRJ-5.5** `#ifdef NAME`, `#ifndef NAME`, `#if expr`, `#elif expr`, `#elifdef NAME`,
  `#elifndef NAME`, `#else` and `#endif` skip lines as in C (FTEQCC reads the last two
  `#elif` forms in `QCC_PR_FalsePreProcessorIf`): in a skipped block only these directives
  are read, for nesting, and comments are not tracked. An
  `#if` or `#elif` expression is made of decimal integers, names, `defined NAME`,
  `defined(NAME)`, parentheses, `!`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&` and `||` with C's
  precedence. A name's value is its macro's text read as a decimal integer, and 0 when the
  macro is undefined or its text is not a number. An expression that does not parse is false.
  A block is closed in the file that opens it (QC-9.12); an `#else`, `#elif` or `#endif`
  without a block is skipped.
- **PRJ-5.6** `#output "path"` and `#pragma progs_dat "path"` declare the module's output,
  resolved against the directory of the file holding them; a later one replaces an earlier
  one.
- **PRJ-5.7** `#pragma sourcefile a b ...` registers each token (QC-23.2 rules) as another
  program's source, resolved against the directory of the module's source. A file
  registered twice is one program.
- **PRJ-5.8** `#pragma includedir "path"` adds an include directory, resolved against the
  directory of the file holding it, after the ones already in use, for the rest of the
  module.
- **PRJ-5.9** `#pragma once` marks the file holding it: a later `#include`, `#includelist`
  entry or list entry naming it again compiles nothing.
- **PRJ-5.10** An `#if` or `#elif` expression whose value stands inside more than 63
  parentheses and `!`s together is false, as one that does not parse is: the reader nests at
  most `MAX_DEPTH` = 64 levels (`qc-preproc`'s `cond.rs`), the value itself one, so no input
  can exhaust its stack. `&&`, `||` and the comparisons chain without nesting. Under an
  FTEQCC profile the limit is FTEF-14.12's.
- **PRJ-5.11** Under the FTEQCC dialect (GMQCC's is PRJ-5.12) the file being compiled, FTEQCC's
  `compilingfile`, stands where PRJ-5.2, PRJ-5.3, PRJ-5.8 and PRJ-5.9 say the file holding the
  directive: an `#include` and an `#includelist` entry are resolved against its directory
  (`QCC_FindBestInclude(msg, compilingfile, ...)`, `dialect-fteqcc.md` FTEF-14.40), a `#pragma
  includedir` path is joined to it, and `#pragma once` marks it. The file being compiled is the
  source of the source form, an entry of a list-form `progs.src` or of an `#includelist`, never an
  `#include`d file, which is read for the file that includes it. So with `progs.src` listing
  `src/a.qc`, which includes `x/b.qh`, which includes `c.qh`, the graph reads `src/c.qh`, as the
  pinned FTEQCC does.
- **PRJ-5.12** Under the GMQCC dialect an `#include` is found beside the file holding it, then
  beside the file being compiled, then in the module's include directories (PRJ-5.2), which GMQCC
  itself has none of: its `ftepp_include_find` tries `ftepp_include_find_path(file,
  ftepp->includename)` and then `ftepp_include_find_path(file, ftepp->itemname)`, and
  `ftepp_directive_include` sets `ftepp->includename = filename;` to the included file while it
  reads it (graphitemaster/gmqcc@b2a319efb41069b9a250e76991dbe505ff8d030d, `ftepp.cpp`). GMQCC's
  preprocessor has no `#includelist` and passes `#pragma` lines to its compiler (`ftepp_out(ftepp,
  "#", false);`), so it is silent on them; this dialect keeps PRJ-5.3, PRJ-5.8 and PRJ-5.9 for an
  `#includelist` entry, `#pragma includedir` and `#pragma once`, the file holding the directive, as
  the C preprocessor Xonotic is read with marks the header that holds `#pragma once`, and the
  preprocessor does the same. So with `progs.src` listing `src/a.qc`, which includes `x/b.qh`, which
  includes `c.qh`, the graph reads `src/x/c.qh`, and `src/c.qh` when that is missing. Xonotic, which
  the C preprocessor reads before GMQCC compiles it, relies on the first of these.

## PRJ-6 The file graph

**Source:** this project; the order is QC-23.3 and FTEQCC's.

### Behaviour

- **PRJ-6.1** A module's file graph is the list of files the module compiles, in
  compilation order: each file appears where it starts to be read, before the files it
  brings in. Each entry records its path, how it came in (the source-form root, a list entry,
  an `#includelist` entry or an `#include`) and the entry of the file that brought it in.
- **PRJ-6.2** A file brought in more than once is compiled, and appears, each time, except
  as PRJ-5.9 says. Files in the source directory that nothing brings in are not part of the
  module (QC-23.6).
- **PRJ-6.3** A file that brings in a file already being read, directly or through others,
  reads it again from the start, as FTEQCC (`QCC_Include` has no guard) and the C
  preprocessor Xonotic builds with do; `#pragma once` is how such a file stops it. Bringing
  in a file while `PRJ_MAX_DEPTH` files are being read is error `E0408` (QCD-5.8), and that
  file is skipped. The vanilla preprocessor is stricter (QC-9.11); that is a dialect
  difference, and the graph follows the compilers that accept such projects.
- **PRJ-6.4** A graph holds at most `PRJ_MAX_FILES` entries. Bringing in one more is error
  `E0407` (QCD-5.7), and the graph stops there.
- **PRJ-6.5** Every diagnostic names the file and the 1-based line of the directive or list
  entry that raised it, or line 0 when it is about the whole source: one that cannot be read
  or has no token.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PRJ_MAX_FILES` | 65536 | this project: a bound on graphs that bring files in exponentially often |
| `PRJ_MAX_DEPTH` | 200 | this project: a bound on files that bring themselves in with no `#pragma once` |

### Test vectors

The unit tests in `crates/qc-project` hold the small cases. The corpus was checked on
2026-10-16 against three compilers, each reading the same files:

- **id's `qcc`** at the pinned commit (`cargo xtask oracles id101`): the source files of the
  2091 functions in the `progs.dat` it compiles from id 1.01 appear in the graph's order
  (`cargo xtask project-corpus --progs`, which CI runs). Swapping two lines of the
  `progs.src` makes the check fail.
- **FTEQCC**, the pinned build `cargo xtask oracles build` makes (P1-26), run with `-srcfile`
  in a copy of each module's directory by `cargo xtask project-corpus --fteqcc`, which CI
  runs on the codebases FTEQCC compiles (P1-33). With default verbosity (`verbose ==
  VERBOSE_PROGRESS`, 0) it prints `compiling %s` for each list entry and `including %s` for
  each `#includelist` entry, and nothing for an `#include`, from `QCC_FindBestInclude`
  (`engine/qclib/qcc_pr_lex.c`), which `QCC_ContinueCompile` (`engine/qclib/qccmain.c`) calls
  with `includetype == 2` for a list entry, `QCC_PR_Precompiler` with `true` for an
  `#includelist` entry and `false` for an `#include`; the guard is `if (includetype && verbose
  >= VERBOSE_PROGRESS)`. So the files it reports are the graph's `Via::Listed` and
  `Via::IncludeList` entries, compared in order. "Prefix" means FTEQCC stopped early, on a
  compile error or a crash (signal 11, cause not investigated), and every entry it printed was
  in the graph's order; the check accepts a prefix. The first recording was on arm64 macOS on
  2026-10-16.
- **The C preprocessor** (`cc -xc -E -H`, Apple clang 21) for Xonotic, which builds with it
  (`qcsrc/Makefile`, `CPP := cc -xc -E`): its include trace is the whole graph, `#include`
  entries and all. `quakec.toml` gave the defines of `qcsrc/tools/qcc.sh`, with
  `__STDC__=1`, which the C preprocessor defines itself, and `include_dirs = ["."]`; it is
  `corpus/projects/xonotic/quakec.toml`, which `cargo xtask corpus fetch xonotic` writes
  into the codebase.

| Codebase | Modules (source form unless noted) | Graph entries | Against |
| --- | --- | --- | --- |
| id 1.01 | `progs.src` (list) | 31 | `qcc`: same order; FTEQCC: 31 same |
| rerelease, Scourge of Armagon, Dissolution of Eternity, Capture the Flag | `progs.src` (list) | 33, 55, 61, 22 | FTEQCC: all same |
| Dimension of the Machine | `progs.src` | 46 | FTEQCC: 45 entries same |
| Dawn of the Machine | `progs.src` | 64 | FTEQCC: prefix of 6 (compile error in `ai.qc`) |
| Copper, Starts with a Bang, CRMod 7 | `progs.src`, `csprogs.src` (list) | 61 + 5, 62 + 6, 72 + 2 | FTEQCC: all same |
| Arcane Dimensions | `progs.src`, `csqc_progs.src` (list) | 139 + 3 | FTEQCC: all same |
| Alkaline | `progs.src` (list), `csprogs.src` (list, registered) | 92 + 3 | FTEQCC: `csprogs.src` same, `progs.src` prefix of 29 (crash) |
| progs_dump, Re:Mobilize | `progs.src` (list) | 60, 64 | FTEQCC: prefixes of 28 (crash) |
| LibreQuake | `progs.src`, `__QW__` undefined | 35 | FTEQCC: 34 same |
| Conduit, Quake Reignited | `progs.src` (list) | 44, 31 | FTEQCC: all same |
| Nuclide | 11 root `.src` files, menu modules from `menu.dat` | 604 | FTEQCC: `hud.src` 13 and `menu-fn` 95 same, `menu-vgui` prefix of 53 (crash) |
| FreeCS, in a Nuclide tree at `cstrike/src` | top-level `progs.src` lists 3 programs; 8 modules in all | 843 without FreeHL; 855, with no error, in the `freecs-tree` tree that adds it at `valve/src` (in CI, on macOS) | FTEQCC, without FreeHL: `hud` and the 4 rules same; `client` and `server` prefixes of 14 and 211; the 20 files of FreeHL were E0402. `shared/defs.h` includes `"animations.h"` and the file is `Animations.h`: on macOS client and server each have warning W0402 there (PRJ-7.2, checked in CI), and on a case-sensitive file system (Linux) both have E0111 there and 853 entries, as FTEQCC would fail to open it (PRJ-7.1) |
| Nazi Zombies: Portable | `progs/ssqc.src`, `csqc.src`, `menu.src` with `#pragma includedir` | 113 | FTEQCC: `menu` 21 same, `csqc` and `ssqc` prefixes of 19 and 4 (crash); `hash_table.qc`, which `tools/qc-compiler-gnu.sh` generates before compiling, is E0402 |
| Xonotic | `quakec.toml` (`corpus/projects/xonotic/`): server, client, menu `progs.inc` | 1764, 1195, 1006 | C preprocessor: all three the same order (in CI: no error) |

## PRJ-7 Matching a name to a file

**Source:** FTEQCC at the commit above: `QCC_FindBestInclude` in `engine/qclib/qcc_pr_lex.c`,
which `QCC_PR_Precompiler` calls for `#include` and each `#includelist` entry and
`QCC_ContinueCompile` (`engine/qclib/qccmain.c`) for each list entry, and `QCC_FileSize` and
`QCC_ReadFile` in `engine/qclib/qcctui.c`, which `QCC_FinishCompile` installs as
`funcs.funcs.parms->FileSize = QCC_FileSize;` and `funcs.funcs.parms->ReadFile =
QCC_ReadFile;`. id's `qcc`: `main` in `qcc/qcc.c` and `LoadFile` and `SafeOpenRead` in
`qcc/cmdlib.c` (Quake-Tools at the commit above).

`QCC_FindBestInclude` joins the name to the including file's directory, then to each include
directory, and takes the first path the file size probe accepts:

```c
if (qccprogfuncs->funcs.parms->FileSize(fullname) == -1)
{
	includepath++;
	continue;
}
```

and when none does, `QCC_Error(ERR_COULDNTOPENFILE, "Couldn't open file %s", newfile);`. The
probe and the read are both a plain `fopen`, with no case folding and no directory listing:

```c
static int PDECL QCC_FileSize (const char *fname)
{
	long    length;
	FILE *f;
	f = fopen(fname, "rb");
	if (!f)
		return -1;
```

and `QCC_ReadFile` opens with `f = fopen(fname, "rb");`. id's `qcc` builds each list entry's
path with `sprintf (filename, "%s%s", sourcedir, com_token);` and reads it with `LoadFile`,
whose `SafeOpenRead` does `handle = open(filename,O_RDONLY | O_BINARY);` and, when that fails,
`Error ("Error opening %s: %s",filename,strerror(errno));`. Neither compiler matches case
itself: each leaves it to the host file system, so the same project opens on a file system
that ignores case (macOS and Windows by default) and fails on one that does not (Linux).

### Behaviour

- **PRJ-7.1** A name that brings a file into the graph (a list entry, PRJ-4.3; an
  `#include`, PRJ-5.2; an `#includelist` entry, PRJ-5.3), once resolved, is opened as the
  host file system opens that path, as both compilers do: exactly where the file system
  matches case, and ignoring case where it does not. The entry's path is the resolved name
  in the name's own case. So a name that differs in case from its file opens on macOS and
  Windows and is `E0402` or `E0111` on Linux, where FTEQCC and id's `qcc` fail too.
- **PRJ-7.2** When a name opens a file, each path component the name spells (the components
  it adds to the directory it was resolved against, once `.` and `..` are folded away) is
  compared with the entry the directory holding it stores. When an entry with exactly that
  name exists it matches; otherwise the entry whose name differs only in case is the stored
  one. If any component differs, the graph reports warning `W0402` (QCD-7.16) at the line of
  the entry or directive, naming the name and the stored spelling of the components it
  spells, once per entry brought in. The stored names are read from the directory, not
  inferred from whether the file opened, so the warning is the same on every OS where the
  file opens: a project that would fail on a case-sensitive file system says so on macOS and
  Windows too. The directory the name was resolved against is not compared.
- **PRJ-7.3** Names that bring no file into the graph are opened as PRJ-7.1 says and not
  compared: a module's `src`, the `progs.src` and `csprogs.src` of PRJ-1.2, `#pragma
  sourcefile` entries, include directories and declared outputs.

### Why not FS-3.7

The game's file system matches loose files ignoring case on every OS (FS-3.7, a deliberate
divergence): it fixes the answer of id's DOS and Windows engine builds, so that an install
copied from a CD in upper case or a mod made on Windows runs the same everywhere, and nothing
else reads those files. A project's source names are different: they are read by a compiler
on the modder's own machine or a build machine, and both compilers above take that host's
answer. Matching them ignoring case would open, with no
error, a project that FTEQCC cannot build on Linux, and would have to pick one of two files
whose names differ only in case, which a case-sensitive file system allows. So the graph
agrees with the compilers on the host it runs on (PRJ-7.1) and warns where the answer would
change on another host (PRJ-7.2).
