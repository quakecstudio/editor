# ADR-0025: Port ericw-tools' vis and light as well as its qbsp

- **Status:** Accepted
- **Date:** 2026-11-14
- **Decided by:** Justin Plock
- **Issue:** [#179](https://github.com/quakecstudio/editor/issues/179) (P7-04), where the full compile's time showed the gap
- **Requirements:** MAP-8, MAP-11, LIC-2

## Question

May `qs-map-vis` and `qs-map-light` port from ericw-tools' `vis/` and `light/`, as
`qs-map-bsp`'s extended dialect ports from its `qbsp/` (ADR-0023)?

## Context

P7-04 asks for compile-time budgets for the map compilers. On the reference Mac (Apple M5), a
full compile of LibreQuake's `lq-e1m1` takes 287 s in `qs-map-vis` against 1.29 s in
ericw-tools' vis; the BSP (3.04 s against 2.22 s) and light (0.86 s against 8.0 s) are close or
faster. `qs-map-vis` is id's vis (VIS-1 to VIS-4), already threaded (VIS-4.7), and against id's
vis it is within 1.5 times on the maps id's vis reads (`lq-e0m1` 2.77 s against 2.02 s, `lq-e0m3`
12.16 s against 8.60 s); id's vis stops on `lq-e1m1`, past its fixed tables. The gap is
algorithmic: ericw-tools' `vis/flow.cc` and `vis/vis.cc` prune the portal flood in ways id's
`flow.c` does not. ADR-0023 lists only ericw-tools' `qbsp/`, so `vis/` could be read but not
ported.

What the files say about their licence at `ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266`,
the commit `oracles/maps.toml` pins, read on 2026-11-14 from the oracle build's copy:

- `COPYING` is the GPL version 2 text, as ADR-0023 records.
- Every file of `light/` and `include/light/`, and `vis/main.cc`, `vis/state.cc`,
  `vis/soundpvs.cc`, `include/vis/vis.hh` and `include/vis/leafbits.hh`, carries a notice
  granting "either version 2 of the License, or (at your option) any later version".
- `vis/flow.cc` (650 lines) and `vis/vis.cc` (822 lines), which hold the flood the speedup is
  in, carry no notice. Under ADR-0023's rule they could not be ported. FTEQW's `qcc_*.c` files
  were in the same position under a GPL-2.0 `COPYING`, and ADR-0017 ported them on Justin's
  "i don't care if it's v2 or v3. either is fine."

## Options

| Option | Consequences |
| --- | --- |
| A. ericw-tools' `vis/` and `light/` are porting upstreams too, files with no notice included as under ADR-0017 (chosen) | `qs-map-vis` ports the pruned flood and full vis fits a budget; `qs-map-light` may port ericw-tools' light features when they are needed; the differentials against id's vis and light stay as the proof of the id behaviour |
| B. Only files with an or-later notice | `flow.cc` and `vis.cc` stay read-only, so the speedup must be designed fresh from id's flood; full vis stays far from ericw-tools' time until it is |
| C. Keep vis and light id's only | Full vis of a large map stays minutes long, and P7-04's budget would record that |

## Decision

Justin Plock decided on 2026-11-14, in the session working P7-04: "add ericw-tools to the
list", option A:

1. `ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266` is a porting upstream for
   `qs-map-vis` (its `vis/` and `include/vis/`) and `qs-map-light` (its `light/` and
   `include/light/`), as well as for `qs-map-bsp`'s extended dialect (ADR-0023). One row of
   `docs/porting.md` covers all three.
2. ADR-0017's clean-port policy applies, as in ADR-0023: port the behaviour, not the
   structure, cite ericw-tools' functions as the authority, write fresh Rust in the crate's
   design with no globals and CONV-17 throughout. Bundled third-party code (`3rdparty/`, the
   submodules) and code under another notice stay out.
3. A file with no notice (`vis/flow.cc`, `vis/vis.cc`) is ported under the repository's
   `COPYING`, as FTEQW's were (ADR-0017); a port of one keeps the copyright line of the file it
   was split from, `vis/main.cc`'s `Copyright (C) 1996-1997  Id Software, Inc.`.
4. A port must not change what id's tools produce where the crate reproduces them: vis's
   output is the same visibility, proved against id's vis and against ericw-tools' vis by the
   differentials.

## Consequences

- `docs/porting.md`'s ericw-tools row and `.claude/rules/porting.md` name `vis/` and `light/`.
- #179 ports the pruned flood into `qs-map-vis` and sets the full-compile budget on it.
- The licence compliance review ([#133](https://github.com/quakecstudio/editor/issues/133),
  P4-22) covers the two files with no notice with FTEQW's.

## Links

- Pull request: this ADR's.
- Related ADRs: [ADR-0017](0017-port-fteqcc.md) (the clean-port policy, files with no notice),
  [ADR-0023](0023-ericw-tools-porting-upstream.md) (ericw-tools' qbsp).
- Sources, read on 2026-11-14 at the pinned commit:
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/COPYING>
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/vis/flow.cc>
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/vis/vis.cc>
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/vis/main.cc>
  - <https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/light/light.cc>
