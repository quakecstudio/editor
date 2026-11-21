# ADR-0030: Make FTEQW's model converter a porting upstream for model import

- **Status:** Proposed
- **Date:** 2026-11-21
- **Decided by:** pending
- **Issue:** [#1081](https://github.com/quakecstudio/editor/issues/1081) (L-47)
- **Requirements:** PRIN-6, MAP-4, LIC-2

## Question

May the readers of the formats a model is made in (iqe, md5mesh and md5anim, smd, fbx) and the
conversion of a boned, animated mesh to a Quake `.mdl` be ported from the `iqm/` directory of
FTEQW, so that it gets a row in the upstream table of `docs/porting.md`?

## Context

Justin asked on 2026-11-20 for "an issue for model importing and manipulation by porting
modelgen and FTE's tools and introducing new commands for such". L-47 has two halves. Writing a
`.mdl` and making one from positions is id's modelgen (`Quake-Tools/qutils/MODELGEN`), which
ADR-0029 already lets us port, and is done (MDL-6, MDL-7). Importing is the other half: modelgen
reads only Alias `.tri` files, a format no tool of today writes. FTEQW's converter reads what
modellers export now.

The only FTEQW code `docs/porting.md` lists is `engine/qclib/` (ADR-0017). The converter is
elsewhere in that repository, `iqm/`, at the pinned commit
`fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e`. What its own files say, read on
2026-11-21:

- `iqm/LICENSE` is the MIT license text, "Copyright (c) 2010-2016 Lee Salzman" (SHA-256
  `2912d5bc70c2358af5e3a2690531fce2fc3858c687319bdb09a373b5dfba2f85`).
- `iqm/README.txt` begins "FTE's fork of Lee Salzman's commandline IQM exporter, based upon
  'IQM Developer Kit 2015-08-03'." and lists its import formats: ".md5mesh .md5anim .iqe .smd
  .fbx .obj".
- `iqm/iqm.cpp` (6278 lines), `iqm/iqm.h` and `iqm/util.h` carry no notice of their own.
  `iqm.cpp` holds the readers (`loadiqe`, `loadmd5mesh`, `loadmd5anim`, `loadsmd`, `loadfbx`,
  `loadobj`) and the Quake writer (`writemdl`).
- FTEQW's `README.md`, as `docs/porting.md` quotes it and ADR-0029 reads it for the rest of
  the repository: "FTEQW is supplied
  to you under the terms of the same license as the original Quake sources, the GNU General
  Public License Version 2."

So the files are Lee Salzman's under the MIT license, with FTE's changes on top, which carry
no notice and fall under the repository's GPL version 2 by its README, as ADR-0029 read
`engine/qclib/`'s files. Code under the MIT license may be used in a GPL-3.0-or-later work
with its notice kept; FTE's part is used as ADR-0017 and ADR-0029 use the rest of FTEQW.

When built inside FTE (`IQMTOOL`), `writemdl` calls FTE's own image functions for the skin
(`Image_LoadMipsFromMemory`, `Image_ChangeFormat`), which are not in `iqm/`; we have our own
palette matching (`qs-wad`), so none of that is needed.

## Options

| Option | Consequences |
| --- | --- |
| A. `iqm/` as a porting upstream for model import, under ADR-0017's clean-port policy (recommended) | A row in `docs/porting.md`; the iqe, md5, smd and fbx readers and the boned-mesh posing are ported into `qs-mdl` and feed MDL-7's maker; a ported file keeps Lee Salzman's line and FTE's above ours and the MIT notice is added to the third-party notices; `model.import` takes what modellers export |
| B. Write the readers from the formats' own descriptions, porting nothing | No licence question; obj and iqe are simple text formats and md5 is already read by `qs-mdl`, but smd's and fbx's rules would be ours to work out from whatever descriptions of them can be found, which this session did not look for; more work and more defects |
| C. Import obj only | Least work: a static mesh with one pose, so a new item or weapon shape but no animated monster; L-47's acceptance (md5, smd, fbx "as the ADR allows") is met by saying the ADR allows none |

## Decision

Recommended: option A. The licence allows it, on the same reading ADR-0029 made of the rest of
FTEQW plus an MIT notice kept; it is what Justin asked for by name; and fbx in particular is
a format better ported from a reader that works than worked out again. Until this is
accepted nothing of `iqm/` is ported: `model.import` reads obj with a reader written from the
format itself (option B's part that costs little), and the other formats wait.

## Consequences

- If accepted: `docs/porting.md` gains the row, with the commit, "MIT (`iqm/LICENSE`), FTE's
  changes GPL-2.0-or-later as ADR-0029 reads FTEQW" and the lines to keep, `Copyright (c)
  2010-2016 Lee Salzman` then `Copyright (c) 2004-2025 FTE's team and its contributors`;
  `.claude/rules/porting.md` gains its bullet; `cargo xtask notices` carries the MIT text; L-47's
  importers for iqe, md5, smd and fbx are ported and its issue closes on them.
- If option C: L-47's deliverable and acceptance are changed to obj alone.
- Either way the `.mdl` writer and maker stay id's modelgen, proved against id's files.

## Links

- [#1081](https://github.com/quakecstudio/editor/issues/1081), L-47; the pull request that adds
  this ADR.
- Related: [ADR-0017](0017-port-fteqcc.md) (the clean-port policy and FTEQW's first upstream
  row), [ADR-0029](0029-license-review.md) (how FTEQW's and `qutils/`'s licences are read),
  [ADR-0002](0002-license-model.md).
- Sources: `fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e`, `iqm/LICENSE`,
  `iqm/README.txt`, `iqm/iqm.cpp`, `iqm/iqm.h`, `iqm/util.h` and `README.md`, fetched from
  GitHub on 2026-11-21.
