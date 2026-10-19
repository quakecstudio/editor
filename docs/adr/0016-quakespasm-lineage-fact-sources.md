# ADR-0016: Use the QuakeSpasm lineage's builtin tables as fact-only sources

- **Status:** Accepted
- **Date:** 2026-10-19
- **Decided by:** Justin Plock
- **Issue:** [#410](https://github.com/quakecstudio/editor/issues/410)
- **Requirements:** LANG-6, PRIN-8, AGT-9, LIC-6, LIC-7

## Question

May the engine-target database take builtin facts (names, numbers, signatures) from the
builtin tables of QuakeSpasm, Ironwail and vkQuake, at pinned commits, as ADR-0014 and
ADR-0015 let it take them from FTEQW's and DarkPlaces' QuakeC definition files?

## Context

LANG-6 asks for an engine-target database "with diagnostics for unsupported builtins", and
PRIN-8 for a warning "when an engine lacks a builtin". [ADR-0015](0015-darkplaces-client-menu-fact-sources.md)
lists the five fact sources the database had, all QuakeC definition files, and repeats
ADR-0014's rule: "Taking facts from a further file still needs the same decision for that
file." None of the five describes QuakeSpasm, Ironwail or vkQuake, so until this decision
the QuakeSpasm table held only id's builtins, and the Ironwail and vkQuake tables id's and
the 2021 rerelease's, inferred from their READMEs (ENGT-3.1, now withdrawn). That gives
false `W0311` warnings: a call of `finaleFinished` (`#79`), which QuakeSpasm implements, was
reported when targeting QuakeSpasm, and a call of FRIK_FILE's `strlen` (`#114`), which
Ironwail and vkQuake implement, when targeting either.

These engines declare their builtins in C, not QuakeC. What the files and their repositories
say, read at the pinned commits on 2026-10-19 (each commit the tip of the repository's
`master` that day):

- `sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`:
  - `Quake/pr_cmds.c` (SHA-256
    `47081c0293520bbe2a3adefd6629dfbbe051b1e7ce6bfb0d29d79ec94b7a84a1`), whose builtin table
    is the positional array `static builtin_t pr_builtin[]`, element `N` being builtin `#N`.
  - `Quake/pr_edict.c` (SHA-256
    `735ef2b75b0fd37631d1b92f3bcd012895cf874cf710a8e3ae65feedd4bc0f49`), whose
    `static const exbuiltin_t exbuiltins[]` maps the rerelease's `#0:ex_` names to builtin
    numbers.
  - Both files open with the same header, verbatim:

    ```
    /*
    Copyright (C) 1996-2001 Id Software, Inc.
    Copyright (C) 2002-2009 John Fitzgibbons and others
    Copyright (C) 2010-2014 QuakeSpasm developers

    This program is free software; you can redistribute it and/or
    modify it under the terms of the GNU General Public License
    as published by the Free Software Foundation; either version 2
    of the License, or (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    See the GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program; if not, write to the Free Software
    Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA  02111-1307, USA.

    */
    ```

  - The repository's `LICENSE.txt` (SHA-256
    `edaef632cbb643e4e7a221717a6c441a4c1a7c918e6e4d56debc3d8739b233f6`) is the GNU General
    Public License, beginning "GNU GENERAL PUBLIC LICENSE" / "Version 2, June 1991"; GitHub
    reports the repository's license as `GPL-2.0`.
- `andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd`, `Quake/pr_cmds.c`
  (SHA-256 `7a5799ab25c4ffbbc69b20cc288a0e49b553ab2f22a0357e3dfd6113708a20c0`), whose
  `builtindef_t pr_builtindefs[]` lists each builtin as `{"name", server function, client
  function, number, extension}`. Its header, verbatim:

  ```
  /*
  Copyright (C) 1996-2001 Id Software, Inc.
  Copyright (C) 2002-2009 John Fitzgibbons and others
  Copyright (C) 2010-2014 QuakeSpasm developers
  Copyright (C) 2016      Spike

  This program is free software; you can redistribute it and/or
  modify it under the terms of the GNU General Public License
  as published by the Free Software Foundation; either version 2
  of the License, or (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

  See the GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA  02111-1307, USA.

  */
  ```

  The repository's `LICENSE.txt` (SHA-256
  `8177f97513213526df2cf6184d8ff986c675afb514d4e68a404010521b880643`) is the GNU General
  Public License, version 2; GitHub reports `GPL-2.0`.
- `Novum/vkQuake@27789d7333a46674474d871137bb71e056719628`:
  - `Quake/pr_cmds.c` (SHA-256
    `fdcfdd176306cfc7ba7c56bc1acd73c4008e9e1749f3bfadc19e262cad862b80`), whose positional
    arrays `pr_ssqcbuiltins[]` and `pr_csqcbuiltins[]` hold the server and client builtins.
    Its header is QuakeSpasm's above, byte for byte.
  - `Quake/pr_ext.c` (SHA-256
    `15001b28c32f3c9a691b481286e9b1e9fa774a17b32c7c3a530d3d064e775dab`), the QSS-style
    extension list: `extensionbuiltins[]` gives each extension builtin as `{"name", server
    function, client function, number, "type string", "description"}`, and `qcextensions[]`
    the extension names. After a first line `/* vim: set tabstop=4: */`, its header is,
    verbatim:

    ```
    /*
    Copyright (C) 1996-1997 Id Software, Inc.
    Copyright (C) 2016      Spike

    This program is free software; you can redistribute it and/or
    modify it under the terms of the GNU General Public License
    as published by the Free Software Foundation; either version 2
    of the License, or (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    See the GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program; if not, write to the Free Software
    Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA  02111-1307, USA.

    */
    ```

  - The repository's `LICENSE.txt` is the one Ironwail's is (the same SHA-256,
    `8177f97513213526df2cf6184d8ff986c675afb514d4e68a404010521b880643`), and GitHub reports
    `GPL-2.0`.

Every one of the five files grants the GPL "either version 2 of the License, or (at your
option) any later version", which [ADR-0004](0004-preview-host-base.md) also records for
QuakeSpasm ("also GPL-2.0-or-later"). No header and no license file forbids taking names,
numbers or signatures from these files or redistributing such facts, and the or-later grant
is compatible with this GPL-3.0-or-later project ([ADR-0002](0002-license-model.md)).

## Options

| Option | Consequences |
| --- | --- |
| A. Fact-only sources now (chosen) | The five files are pinned by commit and SHA-256 in `crates/qc-engines/sources.toml`, and `cargo xtask engines` reads the facts from their C array initialisers, by new rules (ENGT-2.11 to ENGT-2.17): numbers, names, the type strings vkQuake's table gives, and extension names. The QuakeSpasm, Ironwail and vkQuake tables become what each engine implements, which ends their false `W0311` warnings, and Ironwail and vkQuake gain client QuakeC tables. The follow-up implementation entry, P2-58, is delivered with this decision |
| B. Wait for P4-22 | The three engines keep the inferred tables of ENGT-3.1 until the license compliance review (P4-22, #133) rules on these files, and modders targeting them keep getting false `W0311` warnings |

B was not chosen for the reason ADR-0014 gave for its sources: the facts are what the
database needs, and taking facts alone needs no license to the files' expression. Making
these engines porting upstreams, so that their code may be ported rather than only
described, is a different question with its own decision (P3-30); this ADR does not
decide it.

## Decision

Justin Plock decided on 2026-10-19 ([#410](https://github.com/quakecstudio/editor/issues/410#issuecomment-6277207582):
"yes they can become fact only sources") that the builtin tables of QuakeSpasm, Ironwail and
vkQuake, at pinned commits, are **fact-only sources** of the engine-target database. They
are five files: `sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`'s
`Quake/pr_cmds.c` and `Quake/pr_edict.c`,
`andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd`'s `Quake/pr_cmds.c`, and
`Novum/vkQuake@27789d7333a46674474d871137bb71e056719628`'s `Quake/pr_cmds.c` and
`Quake/pr_ext.c`. From them the database takes only facts: builtin numbers and engine names,
QuakeC names or the C function that stands for a builtin, signatures where a table gives a
type string, and extension names. No comment and no code of any of them is copied into the
repository; the reader drops C comments unread. None of them nor its repository becomes a
porting upstream: none is added to the upstream table of `docs/porting.md`, and nothing in
this repository is ported from them. Each file is pinned by commit and SHA-256, with its
repository's license file, in `crates/qc-engines/sources.toml`; `cargo xtask engines`
fetches the pinned files, checks the checksums and generates
`crates/qc-engines/src/tables.rs`, and the reading rules are ENGT-2.11 to ENGT-2.17 in
`docs/spec/engine-targets.md`.

This decision stands beside [ADR-0014](0014-engine-fact-sources.md) and ADR-0015 and
supersedes neither: ADR-0015 still lists the five QuakeC fact sources, and this ADR the five
C ones.

This is not legal advice; the license compliance review before the first public release
(P4-22, #133) confirms the details.

## Consequences

- The database has ten fact sources. `crates/qc-engines` builds QuakeSpasm's server QuakeC
  table from `pr_builtin` and the `exbuiltins` names, Ironwail's server and client QuakeC
  tables from `pr_builtindefs`, and vkQuake's from `extensionbuiltins`, `pr_ssqcbuiltins`
  and `pr_csqcbuiltins` (ENGT-3.9, ENGT-3.11), with vkQuake's extension names from
  `qcextensions` (ENGT-3.10). ENGT-1.4, ENGT-3.1, ENGT-3.3 and ENGT-3.8 are withdrawn and
  replaced.
- `cargo xtask engines` gains a C reader (`xtask/src/engines/c.rs`): tokens with comments
  dropped, `#define` expansion inside the array, and columns chosen per reading in
  `sources.toml`. It evaluates no `#if`, so a macro defined in a conditional block is an
  error when the array uses it.
- The C sources give no signature for QuakeSpasm's and Ironwail's own builtins and no QuakeC
  name for a positional array's entries, so those carry an empty signature and the C
  function's name unless id's table names them.
- Taking facts from a further file still needs the same decision for that file.
- CI regenerates the tables from the pinned files and fails on any difference
  (`cargo xtask engines --check`), so a change upstream never enters the tables without a
  new pin.
- In `plan/`, P4-29 moves to Phase 2 without its dependency on the Phase 3 gate (#106), and
  the follow-up its acceptance criterion asks for, P2-58, records the implementation
  delivered in the same pull request.
- P4-22 (#133) reviews these sources with the rest of the release's license compliance.

## Links

- Issue [#410](https://github.com/quakecstudio/editor/issues/410), Justin's decision comment
  of 2026-10-19, and the pull request that merged this ADR.
- Related ADRs: [ADR-0014](0014-engine-fact-sources.md) and
  [ADR-0015](0015-darkplaces-client-menu-fact-sources.md) (the QuakeC fact sources);
  [ADR-0002](0002-license-model.md) (the project license and the P4-22 review);
  [ADR-0004](0004-preview-host-base.md) (the three engines' licenses).
- Sources, all read on 2026-10-19:
  - <https://github.com/sezero/quakespasm/blob/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5/Quake/pr_cmds.c>
  - <https://github.com/sezero/quakespasm/blob/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5/Quake/pr_edict.c>
  - <https://github.com/sezero/quakespasm/blob/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5/LICENSE.txt>
  - <https://github.com/andrei-drexler/ironwail/blob/08d578136ff43d7d1ef38e636dfbfd3e844be7cd/Quake/pr_cmds.c>
  - <https://github.com/andrei-drexler/ironwail/blob/08d578136ff43d7d1ef38e636dfbfd3e844be7cd/LICENSE.txt>
  - <https://github.com/Novum/vkQuake/blob/27789d7333a46674474d871137bb71e056719628/Quake/pr_cmds.c>
  - <https://github.com/Novum/vkQuake/blob/27789d7333a46674474d871137bb71e056719628/Quake/pr_ext.c>
  - <https://github.com/Novum/vkQuake/blob/27789d7333a46674474d871137bb71e056719628/LICENSE.txt>
  - `crates/qc-engines/sources.toml`, `docs/spec/engine-targets.md`, `docs/porting.md`
    (upstreams).
