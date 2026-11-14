# ADR-0023: Make ericw-tools' qbsp the extended dialect's porting upstream

- **Status:** Accepted
- **Date:** 2026-11-13
- **Decided by:** Justin Plock
- **Issue:** [#164](https://github.com/quakecstudio/editor/issues/164) (P6-07), where the map compiler differential found the gap
- **Requirements:** MAP-8, MAP-11, LIC-2

## Question

May `qs-map-bsp`'s extended dialect (QBSP-14) be ported from ericw-tools' qbsp, so that it gets
a row in the upstream table of `docs/porting.md`, rather than only being run as an oracle?

## Context

`qs-map-bsp` is id's qbsp in both dialects: the id dialect exactly (QBSP-1 to QBSP-13), and the
extended dialect with id's limits lifted (QBSP-14). id's qbsp computes in single precision:
qutils' `common/mathlib.h` makes `vec_t` a `float` unless `DOUBLEVEC_T` is defined.

The map compiler differential (`cargo xtask map-diff`, P6-07) compiles every corpus map with
`qs-map-bsp` and with the oracles. Against id's qbsp, every trace through every clipping hull
agrees on every map id's qbsp reads. Against ericw-tools' qbsp, after the extended dialect took
world brush entities into the world as ericw-tools does (QBSP-14.8), 16 of LibreQuake's and
progs_dump's maps still differ: 3 leak in ours and not in ericw-tools' (`lq-e2m5`, `lq-e3m4`,
`lqdm3`), 3 leave one clipping hull unfilled (`lq-e0m2`, `lq-e0m3`, `lq-e4m5`), and 10 differ on
1 to 61 traces in 2000. Before it leaks, our compile of `lq-e2m5` reports node portals clipped
away and faces qbsp would refuse.

ericw-tools at `ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266`, the commit
`oracles/maps.toml` pins, is not id's algorithm in double precision. Its `qbsp/qbsp.cc` chops
the brushes (`ChopBrushes`) and builds the tree from them (`BrushBSP`, fast then, when it must,
precise), Quake II's way; its vectors are `qvec3d`, `qvec<double, 3>`
(`include/common/qvec.hh`); and its brush contents (detail, clip, skip, hint, liquids, sky) are
its own. Making our port double precision would close most of the gap but leave the trees
split differently, so the differential's criterion, no collision differences and leaks agreeing
on every corpus map, would stay out of reach.

What the repository says about its licence at that commit, read from the oracle build's copy:

- `COPYING` is the GPL version 2 text (SHA-256
  `32b1062f7da84967e7019d01ab805935caa7ab7321a7ced0e30ebe75e5df1670`).
- Every file of `qbsp/` carries a notice granting "either version 2 of the License, or (at your
  option) any later version". Their copyright lines are `Copyright (C) 1996-1997  Id Software,
  Inc.` and `Copyright (C) 1997       Greg Lewis` on most, and lines of Eric Wasylishen's on
  some.
- `common/qvec.cc` also carries `Copyright (C) 1991-2000 Silicon Graphics, Inc. All Rights
  Reserved.`, for a function from another project; the code under it is not ported.

GPL-2.0-or-later code may be used under GPL-3.0-or-later, this project's licence
([ADR-0002](0002-license-model.md)). ericw-tools' files derive from id's qutils, whose own files
carry no notice and whose `COPYING` grants version 2 alone; ericw-tools' notices grant "or any
later version" over that code. The licence compliance review
([#133](https://github.com/quakecstudio/editor/issues/133), P4-22) confirms how id's part of
those files combines with ours, as it does for FTEQW's version 2 terms (ADR-0017).

## Options

| Option | Consequences |
| --- | --- |
| A. ericw-tools' qbsp as the extended dialect's porting upstream, under ADR-0017's clean-port policy; the id dialect stays id's (chosen) | ericw-tools gets a row in `docs/porting.md`; the extended dialect is rewritten from its pipeline (chopped brushes, the brush BSP, its portals, outside fill, faces and contents, in double precision) and proved against ericw-tools by the differential; the id dialect keeps proving itself against id's qbsp |
| B. Our port of id's qbsp in double precision for the extended dialect | Less code; most leaks and unfilled hulls would likely close, but the trees split differently from ericw-tools', so differences stay and the differential cannot gate on ericw-tools |
| C. Keep the extended dialect as it is | Maps made for ericw-tools leak or compile differently in the IDE than in the compiler mappers use; the differential reports ericw-tools without gating on it |

## Decision

Justin Plock decided on 2026-11-13, in the session working P6-07, asked for "the best most
accurate long-term fix": option A, in this order:

1. `ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266` is a porting upstream for
   `qs-map-bsp`'s extended dialect, listed in the upstream table of `docs/porting.md`. Moving
   the pin is a PR that changes the row, `oracles/maps.toml` and the ported files' headers
   together.
2. The extended dialect is ported from its qbsp under ADR-0017's clean-port policy: port the
   behaviour, not the structure; its functions are cited as the authority in the `Ported from`
   header and the rustdoc; the Rust is written fresh in `qs-map-bsp`'s design, with no globals
   and CONV-17 throughout. Only files whose notice grants version 2 "or (at your option) any
   later version" are ported; bundled third-party code (`3rdparty/`, the submodules) and code
   under another notice (`common/qvec.cc`'s Silicon Graphics function) are not.
3. The id dialect stays id's qbsp exactly (QBSP-1 to QBSP-13).
4. The differential gates on both: the id dialect against id's qbsp, the extended dialect
   against ericw-tools' qbsp.

A file ported from ericw-tools keeps every `Copyright` line of each upstream file's header, in
its order, above ours.

## Consequences

- QBSP-14 is rewritten statement by statement as the extended dialect is ported, each new
  statement citing ericw-tools' function; a statement of the old extended dialect it replaces
  is withdrawn.
- `cargo xtask map-diff` lists each map that still differs from ericw-tools until the port
  closes it; P6-07 closes when none does.
- The licence compliance review (#133) covers ericw-tools' id-derived files with qutils.

## Links

- Pull request: this ADR's.
- Related ADRs: [ADR-0002](0002-license-model.md) (licence model, P4-22),
  [ADR-0017](0017-port-fteqcc.md) (the clean-port policy),
  [ADR-0021](0021-zed-porting-upstream.md) (the last upstream added).
- Sources, read on 2026-11-13 at the pinned commit:
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/COPYING>
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/qbsp/qbsp.cc>
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/qbsp/brushbsp.cc>
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/include/common/qvec.hh>
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/common/qvec.cc>
