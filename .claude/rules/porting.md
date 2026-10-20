# Porting from id's source

Read before porting any of id's code. The full process, with the worked example, is
[`docs/porting.md`](../../docs/porting.md); this is the checklist that must hold on every port.

The project is GPL-3.0-or-later ([ADR-0002](../../docs/adr/0002-license-model.md)), so id's
GPL source may be ported directly. A port is done when four things exist together.

1. **The Rust code**, whose file header is, in this order:

   ```rust
   // SPDX-License-Identifier: GPL-3.0-or-later
   // Copyright (C) 1996-1997 Id Software, Inc.
   // Copyright (C) 2026 QuakeC Studio contributors
   //
   // Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_NewString
   ```

   One `Ported from <owner>/<repo>@<40-hex commit>: <path>, <Function>[, <Function>...]` line
   per upstream file, the commit from the upstream table in `docs/porting.md`, and that
   upstream's copyright line above ours. `cargo xtask licenses` enforces it (rules H1 to H5).
   The item's rustdoc names id's function and says `Implements <SECTION>`.
2. **A spec section** in `docs/spec/`, written with the C open: the source with its commit,
   numbered statements covering the edge cases and any undefined behaviour, named constants
   with citations, and test vectors.
3. **Tests that cite the statements**, one or more per statement, each preceded by
   `// spec <ID>: <paraphrase>`.
4. **A differential result** showing the port and id's code agree: the oracle suite
   ([#35](https://github.com/quakecstudio/editor/issues/35)), or before it exists a recorded
   local run (the command, the inputs and the result) in the PR and the spec's test vectors.

Rules that are easy to miss:

- Port only from an upstream and commit listed in `docs/porting.md`. `Quake-Tools/qutils/`
  (the map tools) is not portable until the license review rules on it (P4-22).
- FTEQCC (`fte-team/fteqw`, `engine/qclib/`) is portable under ADR-0017's clean-port policy:
  port its behaviour, not its structure; cite its functions as the authority; write the Rust
  fresh in our lexer, preprocessor, CST and semantic model, with no globals, no transliterated
  control flow and CONV-17 throughout; one implementation with dialect profiles; the oracle
  differentials stay as the proof. Its copyright line is `Copyright (c) 2004-2025 FTE's team
  and its contributors`.
- QuakeSpasm (`sezero/quakespasm`), Ironwail (`andrei-drexler/ironwail`) and vkQuake
  (`Novum/vkQuake`) are portable for the preview host under the same policy (ADR-0018), but
  only files whose own header grants GPL "version 2 ... or (at your option) any later
  version"; never their bundled third-party code. A port keeps every `Copyright` line of the
  upstream file's header, in its order, above ours. id's engine stays the oracle (ADR-0005):
  none of the three is built or run in CI.
- Never commit id's C. Oracles fetch it by commit and checksum.
- Keep the behaviour, not the C: no globals (they become an explicit context), errors as
  `Err` values, every wrap, truncation and float comparison written out with the `#[expect]`
  reason citing the spec statement. The no-panic lints apply in full.
- Name the Rust function after id's in snake case: `ED_NewString` becomes `ed_new_string`.
- A deliberate divergence from id's behaviour is a spec statement with its reason, and the
  differential suite lists it as expected.
- The PR body lists `Spec:` IDs and the upstream commit, and ticks the porting checklist in
  `docs/porting.md`.
