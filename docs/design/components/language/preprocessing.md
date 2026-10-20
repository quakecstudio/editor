# Preprocessing and the project graph

**Crates:** `qc-preproc`, `qc-project` (L2). **Specs:** QC-9, QC-23, PRJ; FTEF-12, FTEF-14, FTEF-18.

## Context and design philosophy

This leaf covers what happens between "a folder of QuakeC" and "a token stream per compiled
file": opening a project, deciding which files a module compiles in which order, and running
the directives and macros of each file. Two L2 crates share it:

- `qc-preproc` - the token-based preprocessor, its `#if` evaluator (`cond.rs`), the FTEQCC
  macro expansion port (`fte_macro.rs`) and the source loaders (`source.rs`).
- `qc-project` - the project model (`quakec.toml` or a bare `progs.src`), and a line-based
  directive reader (`graph.rs`) that follows only the directives that decide files.

id's `qcc` has no preprocessor (QC-9; lib.rs:11-13), so the vanilla directive set is this
project's own definition, named after FTEQCC's. Under an FTEQCC profile the preprocessor
follows FTEQCC as `docs/spec/dialect-fteqcc.md` (FTEF-12, FTEF-14) records it, and since
ADR-0017 the FTEQCC macro expansion is ported from `fte-team/fteqw` `engine/qclib` under a
clean-port policy: behaviour, not structure; one implementation with dialect profiles.

## Components

### Preprocessor

`Preprocessor<L: Loader>` (lib.rs:377-451) is built with a loader and an optional dialect
(`with_dialect`), and `preprocess_file(path)` returns `io::Result<FileOutput>`. The only
`Err` is the loader failing to read the file itself; every other problem is a diagnostic in
the output (lib.rs:445-451). `FileOutput` carries the tokens (`PpToken`), the diagnostics,
and the `#pragma noref` and `#pragma autoproto` offsets the semantic model consumes.

- **Directive dispatch** (lib.rs:679-814). Vanilla runs `#include`, `#define`, `#undef`,
  `#ifdef`, `#ifndef`, `#else`, `#endif`. FTEQCC adds `#if`, `#elif`, `#elifdef`, `#elifndef`,
  `#pragma`, `#warning`/`#error`/`#message`, `#includelist`, `#append`, `#eof` and the build
  directives; vanilla reports E0108 for them (lib.rs:796-799). In vanilla `#else` flips; in
  FTEQCC it takes only when no side was taken (lib.rs:736-743).
- **Macro expansion.** Vanilla uses hide sets as an `Rc` linked list with a per-name live
  count and an iterative `Drop` so a 100000-deep chain does not overflow the stack
  (lib.rs:144-196, QC-9.17). FTEQCC goes through `fte_macro` with no hide sets, FTEQCC's chunk
  and recursion rules (FTEF-14.17) and an expansion bound of 2^20 expansions and 2^22 tokens,
  E0120 (lib.rs:295-300, 1222-1248, FTEF-14.19).
- **Provenance.** Every expanded token records its `Expansion` (name, use span, definition,
  parent), so `origin` gives the place in the compiled file (QC-9.7). `PpToken::spelling` and
  `touches` let a renderer keep FTEQCC's spacing (QC-9.16).
- **Frame macros.** Vanilla frames reset per file, capped at 256 with E0504
  (lib.rs:1590-1603). FTEQCC frames carry across files with W0104 to W0107, E0119, E0504
  (lib.rs:1443-1569, FTEF-18.16, 18.17, 18.21, 18.22). `restart` begins FTEQCC's second
  autoprototyping pass, keeping frames and forgetting defines (FTE-22.3, FTE-22.4).
- **Built-in constants** (lib.rs:1366-1395, FTEF-14.7): `__TIME__`, `__DATE__`, `__QCCVER__`,
  `__FUNC__` give `""`, `__RAND__` gives 0, `__LINE__` is clamped to u16.

### `#if` evaluator (`cond.rs`)

`cond::eval(expr, macros, dialect) -> bool` is shared by the preprocessor and the project
graph "so the two always take the same side" (cond.rs:3-9). Vanilla parses with C precedence
and stops at `//` or `/*`; FTEQCC reads with `ParsePrecompilerIf`'s grouping, `||` and `&&`
at one right-associative level (cond.rs:332-370, FTEF-14.11). Nesting is bounded at 64
(cond.rs:15, PRJ-5.10, FTEF-14.12).

### Sources and loaders (`source.rs`)

`SourceMap` owns file texts and line indexes; `FileId`/`FileSpan` address them. The `Loader`
trait (`load`, `stored_name`) is implemented by `FsLoader` (disk) and `MemoryLoader` (tests,
the semantic model's in-memory files). `stored_name` reports the on-disk spelling of a
case-variant name, choosing the least variant for determinism (source.rs:133-135, 180-192,
PRJ-7.2). qc-project re-exports these loaders (qc-project lib.rs:25), so the L2 file-system
boundary sits in qc-preproc.

### Project model and file graph (qc-project)

- `open`, `open_dir`, `open_file` (lib.rs:243-254) find a `quakec.toml` or a bare
  `progs.src`. Bare discovery is a breadth-first walk over `#pragma sourcefile`
  (lib.rs:419-451), skipping "program list" forms (PRJ-1.5). Bare modules are always
  `Dialect::Vanilla`, `Target::Quake`, with no defines (lib.rs:425, 445-447).
- `manifest.rs` deserialises `quakec.toml` with `deny_unknown_fields`, checks unique module
  names, relative paths and define syntax (PRJ-2.1 to 2.6), and reports errors as strings
  with a line from the TOML span (manifest.rs:60-70).
- `build_graph` (graph.rs) reads `progs.src` in list or source form line by line, following
  `#include`, `#includelist`, conditionals and `#pragma` lines that decide files (PRJ-5),
  word splitting by id's `COM_Parse` (graph.rs:147-196, QC-23.2). Limits: depth 200 (E0408)
  and 65536 files (E0407, which clears the stack) (graph.rs:707-717). Case mismatches are
  W0402 (graph.rs:633-654, PRJ-7.2). Module output paths follow ADR-0008 (PRJ-3).

### Three directive readers

The preprocessor, the project graph and the parser's input layer (`qc-syntax` input.rs) each
decide independently what a directive line is, and they disagree:

| Case | qc-preproc | qc-project graph | qc-syntax input |
|------|-----------|------------------|-----------------|
| `#if`/`#elif` in vanilla | E0108 unknown directive (lib.rs:727, 796-799) | Evaluated (graph.rs:510-515) | Trivia either way |
| `#include <x>` | E0113, needs a quoted string (lib.rs:1083-1094) | Accepted (graph.rs:317-329) | Trivia |
| `\` continuation | Followed under FTEQCC by token gaps (lib.rs:611-623) | Followed when the line's last byte is `\`, any dialect (graph.rs:451-460) | Not followed; directive ends at LF (input.rs:72-80) |

Only `cond::eval` is shared. The result is that a vanilla project's graph can follow an `#if`
or `#include <x>` whose compiled file the preprocessor then rejects, and the editor's
directive trivia can end before the line the preprocessor reads.

## Error model

- `Preprocessor::preprocess_file` returns `Err` only when the loader cannot read the file it
  was asked for; every other problem, an unreadable `#include` among them, is a diagnostic in
  the `FileOutput` (lib.rs:445-451).
- `qc-project` returns `ProjectError { Read, Manifest, NoProject }` when a project cannot be
  opened; a manifest error is a message with the line its TOML span gives
  (manifest.rs:60-70). Problems inside the file graph are diagnostics: E0407, E0408, W0402.

## Verification

- Unit and property tests in `crates/qc-preproc/src/tests/` and `crates/qc-project/src/tests/`,
  citing QC-9, PRJ and FTEF statements.
- `cargo xtask lex-corpus` preprocesses every file the corpus `progs.src` lists, and
  `cargo xtask project-corpus` checks each codebase's file order against qcc's progs.dat and
  FTEQCC's printed order.
- The FTEQCC macro port (ADR-0017) is checked against the pinned FTEQCC by
  `cargo xtask oracles`.
- Fuzz targets `qc_preproc` and `qc_project`.

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Vanilla directive set | The project's own, named after FTEQCC's | None (qcc has no preprocessor) | QC-9; lib.rs:11-13 |
| FTEQCC macro expansion | Port FTEQCC's behaviour into `fte_macro.rs` | Keep reading FTEQW for facts only; port its structure | ADR-0017 option B: clean port, one implementation with profiles |
| Vanilla hide sets | `Rc` linked list with live counts and iterative `Drop` | Recursive hide-set sets | P2-65 (#464): bounded stack for 100000-deep chains (QC-9.17) |
| FTEQCC expansion bound | 2^20 expansions, 2^22 tokens, E0120 | Unbounded | FTEF-14.19 |
| Shared `#if` evaluator | `qc_preproc::cond`, used by both crates | One per crate | Both take the same side (cond.rs:3-9) |
| Graph reader | Separate line-based reader in qc-project | Run the full preprocessor to find files | [inferred] the graph needs only file-deciding directives and must work before files parse |
| Loader boundary | `Loader` trait in qc-preproc, re-exported by qc-project | Trait in qc-project | [inferred] qc-preproc is the lower crate; qc-project depends on it |
| Bare project dialect | Always vanilla, target Quake | Infer from content | [inferred] no manifest to say otherwise |
| Case matching | Report the stored spelling (W0402), pick the least variant | Case-insensitive lookup as FS-3.7 | project-file.md:368-379 explains why PRJ-7 does not follow FS-3.7 |
| Project `Dialect` enum | Own serde enum, `From` into `qc_lexer::Dialect` | One enum | Keeps serde out of the lexer (lib.rs:57-60) |

## Open questions

### Resolved

None recorded.

### Deferred
1. **Three directive readers disagree** (table above). Unify the directive model, share more
   than `cond`, or record each difference as a spec statement?
2. **Manifest defines never reach the preprocessor.** `build_graph` takes them
   (qc-project lib.rs:355), `Preprocessor` has no API to predefine a macro (lib.rs:377-451),
   and `ModuleInput::from_graph` carries none (qc-sema expand.rs:50-64). `cargo xtask
   fteqcc-profile` rejects such modules (xtask/src/fteqcc_profile.rs:86-92).
3. Bare projects are always vanilla (lib.rs:425), so a bare FTEQCC tree reads `#if` with C
   precedence. Intended?
4. `condition()` has a vanilla branch (lib.rs:1033-1037) only reachable from FTEQCC callers
   (lib.rs:727, 836, 862). Dead code?
5. Duplicates: `normalize` (qc-preproc lib.rs:353-373, qc-project graph.rs:121-139); three L2
   `Diagnostic` types. For the CONV-17 audit (P2-60, #452).
6. `src` paths with `..` are accepted, `output` paths are not (manifest.rs:87-98).
7. qc-preproc README.md:64 names `src/tests.rs`; the tests are in `src/tests/mod.rs`. The
   README's QC-8 claim overlaps qc-lexer's; QCD-8 is declared by both qc-preproc and
   qc-project.
8. PRJ-5.x statements are specified in `project-file.md` but `cond` implements them in
   qc-preproc (cond.rs:416-731 tests).
9. Performance: quadratic expansion of a 100000-macro chain with live hide-set nodes, and
   memory exhaustion for growing-argument chains (docs/readiness/p2-r2.md, section 7). No
   budget applies to L2 yet.
10. project-file.md:278-302 records FTEQCC oracle runs that crashed (signal 11, cause not
    investigated) and were accepted as prefixes.
11. `fs_loader_picks_the_least_case_variant` skips itself on case-insensitive hosts
    (qc-preproc tests/mod.rs:885-891), so a macOS run checks less of PRJ-7.2 than a Linux run.

## References

- Files: crates/qc-preproc/src/{lib.rs, cond.rs, fte_macro.rs, source.rs, tests/*},
  crates/qc-project/src/{lib.rs, graph.rs, manifest.rs, tests/*}, both READMEs
- Depends on: qc-lexer (tokens, `Dialect`, codes); serde and toml (qc-project only)
- Used by: qc-sema (`expand.rs`, `db.rs`), qc-engines (`ModuleKind`, `Target`), xtask
  (`lex_corpus`, `project_corpus`, `sema_corpus`, `fteqcc_profile`), fuzz
- Upstream: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e `engine/qclib/qcc_pr_lex.c`,
  `qcc_cmdlib.c` (fte_macro.rs:1-15)
