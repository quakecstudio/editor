# ADR-0018: Make QuakeSpasm, Ironwail and vkQuake porting upstreams for the preview host

- **Status:** Accepted
- **Date:** 2026-10-20
- **Decided by:** Justin Plock
- **Issue:** [#448](https://github.com/quakecstudio/editor/issues/448)
- **Requirements:** LIC-2, ENG-2

## Question

May the preview host port code from QuakeSpasm (`sezero/quakespasm`), Ironwail
(`andrei-drexler/ironwail`) and vkQuake (`Novum/vkQuake`), so that each gets a row in the
upstream table of `docs/porting.md`, rather than only being read for description and facts?

## Context

ENG-2 makes the preview host "our own engine in Rust, ported from id's GPL source", and LIC-2
requires ported code to keep its upstream's copyright notice and cite the file and function it
came from. Port headers are checked by `cargo xtask licenses` (rules H1 to H5 in
`docs/porting.md`), and rule H5 refuses any upstream that is not in the table. Before this
decision the table held id's two repositories and, since [ADR-0017](0017-port-fteqcc.md),
FTEQW's `engine/qclib`. `docs/porting.md` already said that adding an upstream "(ericw-tools,
QuakeSpasm, ...) is a PR that adds a row here after checking that its license allows use under
GPL-3.0-or-later, and records the copyright line its files carry".

The three engines are already part of the plan in other roles:

- [ADR-0004](0004-preview-host-base.md) records all three as "GPL-2.0-or-later" and makes
  QuakeSpasm the reference for the rerelease and modern-mod behaviour, which id's 1996 engine
  does not have.
- [ADR-0012](0012-bsp-loader-default.md) has `qs-bsp` follow QuakeSpasm's BSP loader, "read for
  description and not ported".
- [ADR-0016](0016-quakespasm-lineage-fact-sources.md) takes builtin facts from five of their
  files, and says: "Making these engines porting upstreams, so that their code may be ported
  rather than only described, is a different question with its own decision (P3-30); this ADR
  does not decide it."
- [ADR-0005](0005-default-preview-target.md) makes id's engine the oracle the host is proved
  against, "built from the pinned id-Software/Quake source as an oracle in CI only".

P3-22, the preview host's engine spec, has to know which sources it may port from before it is
written, and L-13 decides which of the later engines' improvements the host adopts.

What the three repositories say about their licence, at the commits ADR-0016 pinned (each the
tip of `master` on 2026-10-19). The files were downloaded again on 2026-10-20 and their SHA-256
sums match ADR-0016's.

- `sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`. `Quake/pr_cmds.c` (SHA-256
  `47081c0293520bbe2a3adefd6629dfbbe051b1e7ce6bfb0d29d79ec94b7a84a1`) opens with this header,
  verbatim, as ADR-0016 quotes it:

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

  The repository's `LICENSE.txt` (SHA-256
  `edaef632cbb643e4e7a221717a6c441a4c1a7c918e6e4d56debc3d8739b233f6`) is the GPL version 2
  text.
- `andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd`. `Quake/pr_cmds.c`
  (SHA-256 `7a5799ab25c4ffbbc69b20cc288a0e49b553ab2f22a0357e3dfd6113708a20c0`) opens with this
  header, verbatim, as ADR-0016 quotes it:

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
  `8177f97513213526df2cf6184d8ff986c675afb514d4e68a404010521b880643`) is the GPL version 2
  text.
- `Novum/vkQuake@27789d7333a46674474d871137bb71e056719628`. `Quake/pr_cmds.c` (SHA-256
  `fdcfdd176306cfc7ba7c56bc1acd73c4008e9e1749f3bfadc19e262cad862b80`) carries QuakeSpasm's
  header above, byte for byte. `Quake/pr_ext.c` (SHA-256
  `15001b28c32f3c9a691b481286e9b1e9fa774a17b32c7c3a530d3d064e775dab`), after a first line
  `/* vim: set tabstop=4: */`, opens with this header, verbatim, as ADR-0016 quotes it:

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

  The repository's `LICENSE.txt` has the same SHA-256 as Ironwail's and is the GPL version 2
  text.

Every header quoted grants the GPL "either version 2 of the License, or (at your option) any
later version", which allows use under GPL-3.0-or-later ([ADR-0002](0002-license-model.md)).
Not every file under `Quake/` carries that grant. A scan of the first 40 lines of every `.c` and
`.h` file under `Quake/` at the three commits found it in 136 of 157 files for QuakeSpasm, 142
of 166 for Ironwail and 158 of 186 for vkQuake. The rest are bundled third-party code under
other licences (`lodepng.c`, `miniz.c`, `strlcpy.c`, `jsmn.h` and the like) or files with no
notice at all (`resource.h`, `qs_bmp.h`, vkQuake's `r_ssao.c` and others). The copyright lines
also vary by file: most carry the three lines of QuakeSpasm's header, some add others
(`Copyright (C) 2007-2008 Kristian Duske`, `Copyright (C) 2016 Axel Gneiting`, ...), and some
have only one (`Copyright (C) 2022 A. Drexler` in Ironwail's `steam.c`).

## Options

| Option | Consequences |
| --- | --- |
| A. Porting upstreams under ADR-0017's clean-port policy (chosen) | The three get rows in `docs/porting.md`, and P3-22 and the preview host's ports (P3-24, P3-26, P3-27) may cite their functions where the host follows them, chiefly the rerelease's engine behaviour, which has no id source. `cargo xtask licenses` accepts their `Ported from` headers. id's engine stays the oracle |
| B. Keep them for description and facts only | The host ports only id's code. The rerelease's engine behaviour is described from QuakeSpasm and written fresh, as `qs-bsp` was under ADR-0012, and L-13's adopted improvements are rewritten from descriptions rather than ported |

B was not chosen: the three engines are GPL-2.0-or-later, as id's code is, so describing their
code instead of porting it adds work without a licence reason, and it leaves the rerelease's
behaviour, which ADR-0004 already makes QuakeSpasm the reference for, without a citable source.

## Decision

Justin Plock decided on 2026-10-20
([#448](https://github.com/quakecstudio/editor/issues/448#issuecomment-6304194139)):
"approved. QuakeSpasm (sezero/quakespasm), Ironwail (andrei-drexler/ironwail) and vkQuake
(Novum/vkQuake) become porting upstreams for the preview host under ADR-0017's clean-port policy
(port behaviour, not structure; cite functions; clean Rust; oracle differential as proof). Their
files are explicitly GPL-2.0-or-later."

So:

- `sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`,
  `andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd` and
  `Novum/vkQuake@27789d7333a46674474d871137bb71e056719628` are porting upstreams for the preview
  host, listed in the upstream table of `docs/porting.md`. Moving a pin is a PR that changes the
  row and checks the ported files' headers again.
- Only a file whose own header grants the GPL "either version 2 of the License, or (at your
  option) any later version" may be ported. Bundled third-party code and files with no notice
  may not.
- A file ported from one of them keeps every `Copyright` line of the header of each upstream
  file it ports, in that file's order, above ours.
- The port follows ADR-0017's clean-port policy: port the behaviour, not the structure; the
  upstream's source is the authority, read and cited function by function; the Rust is written
  fresh inside the preview host's own design, with no globals, no transliterated control flow
  and CONV-17 throughout; the oracle differential stays as the proof.
- id's engine stays the oracle ([ADR-0005](0005-default-preview-target.md) is unchanged). These
  are porting sources, not oracles, and none of them is built or run in CI. Where the host
  follows one of them instead of id's code, the difference is a spec statement with its reason
  and an expected difference in the engine differential (P3-25), as L-13 already says.
- id's code stays the first source: as `docs/porting.md` says, a spec statement cites id's code
  unless the behaviour has no id source, and then it cites the registered upstream it follows.

ADR-0016 is unchanged: its five files remain fact-only sources of the engine-target database,
and that database takes facts from them, not code.

This is not legal advice; the licence compliance review before the first public release (P4-22,
[#133](https://github.com/quakecstudio/editor/issues/133)) confirms the details.

## Consequences

Carried by this ADR's pull request (`Closes #448`):

- `docs/porting.md` gains the three rows, each with its commit, its licence and the copyright
  lines its files carry, and says how a port from them keeps its upstream file's lines.
  `.claude/rules/porting.md` summarises it.
- `cargo xtask licenses` rule H4 reads every copyright line a row lists. A header citing one of
  these upstreams must carry at least one of that row's lines, the first upstream's opening
  its `Copyright` lines, above ours and in the order of the `Ported from` lines; the other lines its upstream file carries are kept by the porter and
  checked in review, since the check does not fetch upstream files. A row with one line, such
  as id's or FTEQW's, works as before.
- In `plan/`, P3-30's deliverable names this file, P3-22 may cite the three engines' functions,
  and L-13's adopted changes are ported from them under this decision.

Harder or ruled out:

- Porting any bundled third-party file of the three repositories, or a file with no licence
  notice.
- Building or running any of the three in CI as an oracle or a reference; QuakeSpasm's
  performance numbers (P3-29) are measured by hand on the reference machines.

## Links

- Issue [#448](https://github.com/quakecstudio/editor/issues/448), Justin's decision comment of
  2026-10-20, and the pull request that merged this ADR.
- Related ADRs: [ADR-0002](0002-license-model.md) (licence model, P4-22),
  [ADR-0004](0004-preview-host-base.md) (the three engines' licences, QuakeSpasm as the
  rerelease reference), [ADR-0005](0005-default-preview-target.md) (id's engine as the oracle),
  [ADR-0012](0012-bsp-loader-default.md) (QuakeSpasm's BSP loader, described),
  [ADR-0016](0016-quakespasm-lineage-fact-sources.md) (the same commits as fact-only sources),
  [ADR-0017](0017-port-fteqcc.md) (the clean-port policy).
- Sources, read on 2026-10-20 from the archives of the pinned commits:
  - <https://github.com/sezero/quakespasm/blob/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5/Quake/pr_cmds.c>
  - <https://github.com/sezero/quakespasm/blob/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5/LICENSE.txt>
  - <https://github.com/andrei-drexler/ironwail/blob/08d578136ff43d7d1ef38e636dfbfd3e844be7cd/Quake/pr_cmds.c>
  - <https://github.com/andrei-drexler/ironwail/blob/08d578136ff43d7d1ef38e636dfbfd3e844be7cd/LICENSE.txt>
  - <https://github.com/Novum/vkQuake/blob/27789d7333a46674474d871137bb71e056719628/Quake/pr_cmds.c>
  - <https://github.com/Novum/vkQuake/blob/27789d7333a46674474d871137bb71e056719628/Quake/pr_ext.c>
  - <https://github.com/Novum/vkQuake/blob/27789d7333a46674474d871137bb71e056719628/LICENSE.txt>
  - The `Quake/` directory of each archive, for the licence and copyright-line scan.
