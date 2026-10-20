# Porting from id's source

QuakeC Studio is GPL-3.0-or-later ([ADR-0002](adr/0002-license-model.md)), so we port id's
formats, VM and tools to Rust directly, with the C open beside us. A port is done when four
things exist together:

1. the Rust code, with id's copyright line and a citation of the file, function and commit;
2. a section in [`docs/spec/`](spec/README.md) that states the behaviour as numbered,
   testable statements;
3. tests that cite those statements;
4. a differential test showing the port and id's code agree.

The [worked example](#worked-example) at the end does all four for one function.
Requirements: LIC-2 and PM-3 in [`plan/requirements.yaml`](../plan/requirements.yaml).

## Upstreams

Port only from a pinned commit of an upstream listed here. The commit is the one the oracles
build ([#35](https://github.com/quakecstudio/editor/issues/35)); the copyright line is the one
a ported file must keep.

| Upstream | Commit | License | Copyright line to keep |
| --- | --- | --- | --- |
| [`id-Software/Quake`](https://github.com/id-Software/Quake) | `bf4ac424ce754894ac8f1dae6a3981954bc9852d` | GPL-2.0-or-later (file headers) | `Copyright (C) 1996-1997 Id Software, Inc.` |
| [`id-Software/Quake-Tools`](https://github.com/id-Software/Quake-Tools), `qcc/` only | `c0d1b91c74eb654365ac7755bc837e497caaca73` | GPL-2.0-or-later (file headers) | `Copyright (C) 1996-1997 Id Software, Inc.` |
| [`fte-team/fteqw`](https://github.com/fte-team/fteqw), `engine/qclib/` only ([ADR-0017](adr/0017-port-fteqcc.md)) | `f937b9d88f71fc4429db5fe56c6a98d922711b2e` | GPL-2.0: `README.md` "FTEQW is supplied to you under the terms of the same license as the original Quake sources, the GNU General Public License Version 2.", `engine/qclib/LICENSE` the GPL-2.0 text; the `qcc_*.c` files carry no notice, so a ported file keeps the README's line | `Copyright (c) 2004-2025 FTE's team and its contributors` |

**Not yet portable:** `Quake-Tools/qutils/` (qbsp, vis, light and the other map tools). Its
files carry no license notice and its `COPYING` is GPL-2.0 with no "or later" grant, so it is
not established that it can be combined with GPL-3.0-or-later code. The license compliance
review ([#133](https://github.com/quakecstudio/editor/issues/133), P4-22) rules on it before
Phase 6 ports the map tools. Until then the map tools may be read and used as oracles, not
ported.

Adding an upstream (ericw-tools, QuakeSpasm, ...) is a PR that adds a row here after
checking that its license allows use under GPL-3.0-or-later, and records the copyright line its
files carry. FTEQW was added by Justin Plock's decision of 2026-10-20 (ADR-0017); the licence
compliance review (P4-22) confirms how its version 2 terms combine with ours.

### Porting FTEQCC: behaviour, not structure

FTEQCC is ported under a clean-port policy ([ADR-0017](adr/0017-port-fteqcc.md)), so the
FTEQCC dialect stays one supportable implementation, not an amalgamation of two compilers:

- port FTEQCC's behaviour, not its structure;
- its source is the authority, read and cited function by function (`QCC_PR_CheckCompConst`,
  `QCC_PR_ExpandPreProcessorMacro`, ...), in the `Ported from` header and the rustdoc;
- the Rust is written fresh inside the existing lexer, preprocessor, CST and semantic model;
- no globals and no transliterated control flow, and [CONV-17](conventions.md) throughout;
- one implementation with dialect profiles, never a parallel compiler;
- the oracle differentials (`cargo xtask oracles`) stay as the proof.

## Which source is authoritative

| Layer | Behaviour | Authoritative source |
| --- | --- | --- |
| L0 formats | progs.dat header, statements, defs, functions, strings | `WinQuake/pr_comp.h`, `progs.h`, `pr_edict.c` (`PR_LoadProgs`) |
| L0 formats | PAK | `WinQuake/common.c` (`COM_LoadPackFile`) |
| L0 formats | WAD2 | `WinQuake/wad.c`, `wad.h` |
| L0 formats | BSP29, MDL | `WinQuake/bspfile.h`, `modelgen.h`, `model.c` (`Mod_LoadBrushModel`, `Mod_LoadAliasModel`); for BSPs, `model.c` and `gl_model.c` are authoritative for the WinQuake and GLQuake loader modes |
| L0 formats | BSP loading by default | QuakeSpasm (`sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`, `Quake/gl_model.c`) as `docs/spec/bsp.md` BSP-20 describes it, read for description and not ported ([ADR-0012](adr/0012-bsp-loader-default.md)) |
| L0 formats | .lno, BSP2, .map | No portable id source: `.lno` and BSP2 come from FTEQCC, GMQCC and ericw-tools once registered above; `.map` from `qutils/QBSP/MAP.C` after the P4-22 ruling |
| L1 VM | Opcodes, calls, local stack, runaway guard | `WinQuake/pr_exec.c` |
| L1 VM | Edicts, fields, system globals, entity lump parsing | `WinQuake/pr_edict.c`, `progdefs.h`, `progs.h` |
| L1 VM | Builtins | `WinQuake/pr_cmds.c` |
| L2 syntax | Lexer, `$frame` and other `$` commands, `progs.src` | `Quake-Tools/qcc/pr_lex.c`, `qcc.c` |
| L2, L3 | The FTEQCC dialect: lexer, preprocessor, parser, semantics | `fte-team/fteqw` `engine/qclib/qcc_pr_lex.c`, `qcc_pr_comp.c`, `qcc_cmdlib.c` ([ADR-0017](adr/0017-port-fteqcc.md)) |
| L3 semantics | Types, definitions, scopes, type checking | `Quake-Tools/qcc/pr_comp.c`, `qcc.h` |
| L4 compiler | Code generation, progs.dat output, `progdefs.h` CRC | `Quake-Tools/qcc/pr_comp.c`, `qcc.c` |
| L5 to L7 | Commands, agent, UI | No id source; nothing to port |

When sources disagree:

- **The engine wins over the compiler** for what a progs.dat means (L0, L1): the game is what
  players run. The compiler's view governs only what our compiler emits (L4).
- **WinQuake wins over QuakeWorld** (`QW/`). A spec that follows QuakeWorld says so in its
  statement.
- **The pinned commit wins over memory, forums and other ports.** QuakeSpasm and others are
  useful for understanding, but a spec statement cites id's code unless the behaviour has no
  id source (then it cites the registered upstream it follows). The FTEQCC dialect has no id
  source: FTEQW's `engine/qclib` at its pinned commit is its authority.

## File headers

Every source file starts with the SPDX line ([`conventions.md`](conventions.md#source-file-headers)).
A file containing ported code has this header, in this order:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_NewString
```

- One `Ported from` line per upstream file:
  `Ported from <owner>/<repo>@<40-hex commit>: <path>, <Function>[, <Function>...]`.
  A file ported from several id files has several lines.
- The upstream's copyright line, copied from the upstream table, comes right after the SPDX
  line and above ours. A file ported from two upstreams, such as an ADR-0017 port that also
  follows id's code, keeps both lines, in the order of its `Ported from` lines:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_lex.c, PR_LexString
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_LexString
```

- The item that holds the port also names id's function in its rustdoc and links its spec
  section (`Implements EXAMPLE-1`).

### The rule `cargo xtask licenses` enforces

[#2](https://github.com/quakecstudio/editor/issues/2) (P0-02) implements this rule. It applies
to every file the SPDX scan covers; Markdown is exempt because a spec cites id's code without
porting it. The **header** is the leading block of comment lines (`//`, `#`, or one `/* */`
block for C), ending at the first line that is blank or not a comment.

1. **H1.** A file whose header contains `ported from` (any case), `id software` (any case) or
   `id-Software/` is a ported file.
2. **H2.** Every header line containing `ported from` (any case) matches
   `^(//|#|\s*\*)?\s*Ported from [\w.-]+/[\w.-]+@[0-9a-f]{40}: \S+, \w+(, \w+)*$`.
3. **H3.** A ported file has at least one line that passes H2.
4. **H4.** For each upstream named in an H2 line, the header contains that upstream's
   copyright line from the table above (for `id-Software/*`:
   `Copyright (C) 1996-1997 Id Software, Inc.`, compared with runs of spaces collapsed).
   These lines, each once (`id-Software/Quake` and `id-Software/Quake-Tools` share one), are
   the header's first `Copyright` lines, in the order the `Ported from` lines first name
   their upstreams, so every upstream's line is above ours.
5. **H5.** An upstream named in an H2 line must be in the table above.

The error names the file, the rule and what is missing. These headers are the fixtures for
the check's tests:

Passes: the two headers above.

Fails H4, ported from id but without id's copyright line:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_NewString
```

Fails H3, says it is based on id's code without a citation:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Based on id Software's pr_exec.c.
```

Fails H2, citation without the commit:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake: WinQuake/pr_exec.c, PR_ExecuteProgram
```

Fails H5, an upstream that is not registered:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from example/quake-port@0123456789abcdef0123456789abcdef01234567: src/vm.c, run
```

## Writing the port

- **Name it after id's function**, in snake case: `ED_NewString` becomes `ed_new_string`.
  The rustdoc says which id function it is.
- **Keep the behaviour, not the C.** Idiomatic Rust is fine (iterators instead of pointer
  walks, slices instead of NUL-terminated strings) as long as every observable result is
  the same. Keep id's structure where it carries behaviour, such as the order checks run in.
- **No global state.** id's globals (`pr_globals`, `pr_strings`, `sv`) become parameters or
  fields of an explicit context.
- **Spell out C's arithmetic.** The no-panic lints in [`conventions.md`](conventions.md)
  force each wrap, saturate, truncate or checked conversion to be written; pick the one the
  C does on the reference platform and cite the spec statement in the `#[expect]` reason.
  Float comparisons stay exact.
- **Errors are values.** `Sys_Error`, `Host_Error` and `PR_RunError` become `Err` variants;
  the spec says which error each case raises.
- **Undefined behaviour in the C** (signed overflow, out-of-range float-to-int, reads past a
  buffer) gets a spec statement that fixes the answer, normally what the original x86 build
  did, and says why. The port implements the statement, not a guess.
- **Never commit id's C.** The oracles fetch it by commit and checksum. Only the Rust port,
  which is a derived work under the GPL, lives in the repo. id's comments may be carried over.

## Writing the spec

Each port has a section in `docs/spec/`, written with the C open, following
[`spec/README.md`](spec/README.md) and [`spec/_template.md`](spec/_template.md): a stable ID,
the source with its commit, numbered behaviour statements covering the edge cases, named
constants with citations, and test vectors. If the port behaves differently from id's code
on purpose, the difference is a statement with its reason, and the differential suite lists
it as an expected divergence.

## Differential tests

The unit tests pin each statement; the differential tests prove the port and id's code
agree. Together they must cover:

- **Every statement.** Each statement has a unit test that cites it, with vectors checked
  against id's code.
- **The same inputs through both.** id's function runs in the oracles
  ([#35](https://github.com/quakecstudio/editor/issues/35)) and the port runs in Rust on the
  same inputs; results are compared byte for byte, or for the VM, globals and edicts after
  every statement ([#37](https://github.com/quakecstudio/editor/issues/37), using the harness
  in [#51](https://github.com/quakecstudio/editor/issues/51)).
- **Edges.** Each named constant at, below and above its limit; empty and maximal input;
  malformed input; every undefined-behaviour statement.
- **Breadth.** The corpus (id 1.01 and rerelease QuakeC, public mods) and, for parsers and
  the VM, fuzz-generated input.
- **The bar** in [`testing.md`](testing.md) (TEST-6): zero divergences, and at least 90% of the
  oracle's statements or code paths exercised; for the id VM differential suite, zero
  divergences, with statement coverage measured and reported but not gated
  ([ADR-0013](adr/0013-differential-coverage-bar.md)).
- **Sensitivity.** A deliberately broken port fails the differential test. cargo-mutants on
  the port finds no surviving mutant that the differential test should catch.

The oracles are built by `cargo xtask oracles` ([`oracles/README.md`](../oracles/README.md)):
id's VM as a trace runner, with id's engine functions extracted unchanged from the pinned
source where the mock engine runs them, and id's qcc. A function the oracles do not compile
yet is checked as the earlier ports were: build it from the pinned commit locally, run it on
the vectors, and record the command, the inputs and the result in the PR and the spec's test
vectors section; then add it to the oracles.

## Porting PR checklist

- [ ] Header follows [File headers](#file-headers); `cargo xtask licenses` passes.
- [ ] The rustdoc names id's function and says which spec section it implements.
- [ ] The spec section exists, cites the file, function and commit, and every statement has
      a test that cites it.
- [ ] Differential result: the oracle suite, or for a function the oracles do not compile
      yet, the recorded local run.
- [ ] The PR body lists `Spec:` IDs and the upstream commit.

## Worked example

id's `ED_NewString` decodes the backslash escapes in string values from a map's entity lump
or a saved game.

| Piece | Where |
| --- | --- |
| id's function | `WinQuake/pr_edict.c`, `ED_NewString`, at [`bf4ac42`](https://github.com/id-Software/Quake/blob/bf4ac424ce754894ac8f1dae6a3981954bc9852d/WinQuake/pr_edict.c#L693-L717) |
| The port | [`crates/qs-porting-example/src/lib.rs`](../crates/qs-porting-example/src/lib.rs): header with id's copyright line and the `Ported from` citation; `ed_new_string` |
| The spec | [`docs/spec/example.md`](spec/example.md), section EXAMPLE-1, statements EXAMPLE-1.1 to EXAMPLE-1.7 |
| The tests | Unit tests in the same file, one per statement, each starting `// spec EXAMPLE-1.n` |

What porting it showed:

- The C's bounds check `i < l-1` never fails for a backslash, so a trailing backslash reads the
  terminating NUL as its escaped byte and still produces a backslash. The spec states this as
  EXAMPLE-1.4 instead of leaving it to the reader of the C.
- The C takes a NUL-terminated string; the port takes a byte slice, so EXAMPLE-1.1 pins that
  input ends at the first NUL.

Differential result, recorded on 2026-10-15 because the oracles do not exist yet: id's
`ED_NewString` was copied unchanged from the pinned commit (lines 693 to 717), built with
the system C compiler with `Hunk_Alloc` replaced by `calloc` (the engine's hunk is zeroed),
and run on every input of up to six bytes drawn from `\`, `n`, `a`, 0xFF and NUL. All 19,531
outputs matched `ed_new_string`. Removing the trailing-backslash case from the port produced
3,255 divergences and failed two unit tests, and cargo-mutants caught all five mutants of the
crate.

The crate is a workspace member, so `cargo xtask ci` builds and tests it once the workspace
([#1](https://github.com/quakecstudio/editor/issues/1)) and xtask
([#2](https://github.com/quakecstudio/editor/issues/2)) land.
