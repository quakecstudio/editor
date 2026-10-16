# ADR-0012: Load BSPs by the FitzQuake lineage's rules by default, with WinQuake and GLQuake as modes

- **Status:** Accepted
- **Date:** 2026-10-16
- **Decided by:** Justin Plock
- **Issue:** None; decided in the Phase 1 orchestrating session and carried out by P1-45
- **Requirements:** MAP-6

## Question

Whose loading rules does the editor follow by default when it reads a BSP, given that id's
WinQuake and GLQuake loaders and the engines modders play on today accept different maps?

## Context

`crates/qs-bsp` reads a BSP with `Bsp::read(bytes, loader)`, where `BspLoader` is `WinQuake` or
`GlQuake` and has no default (P1-43, [#311](https://github.com/quakecstudio/editor/issues/311),
PR [#313](https://github.com/quakecstudio/editor/pull/313)). The two differ in the largest face
extent they accept (BSP-9.4, BSP-9.15) and, once P1-44
([#314](https://github.com/quakecstudio/editor/issues/314)) lands, in GLQuake's
`SURF_UNDERWATER` flag. Every caller has to pick one, and nothing says which the editor itself
uses, for example when it validates a map the modder is about to run.

Modders today play and test on engines descended from FitzQuake: QuakeSpasm, Ironwail and
vkQuake. Their loaders depart from both of id's on purpose. From each project's source at the
commit given, read for this decision:

- QuakeSpasm `Quake/gl_model.c`, `CalcSurfaceExtents`, line 1453, and the same line in
  Ironwail (line 1249) and vkQuake (line 1775):
  `if ( !(tex->flags & TEX_SPECIAL) && s->extents[i] > 2000) //johnfitz -- was 512 in glquake, 256 in winquake`.
  id's `WinQuake/gl_model.c`, line 746, has `s->extents[i] > 512 /* 256 */`.
- QuakeSpasm `Quake/gl_model.c`, the three leaf loaders (lines 1914, 1957 and 2000), Ironwail
  (line 1654) and vkQuake (line 2225): `//johnfitz -- removed code to mark surfaces as SURF_UNDERWATER`,
  where id's `gl_model.c`, line 937, has
  `out->firstmarksurface[j]->flags |= SURF_UNDERWATER;`.
- QuakeSpasm `Quake/gl_model.h`, lines 155 and 156: `short		texturemins[2];` and
  `short		extents[2];`, so the 16-bit storage of BSP-9.14 still holds.
- QuakeSpasm's `CalcSurfaceExtents` also differs in ways the spec does not yet cover: it
  starts the bounds at `FLT_MAX` and `-FLT_MAX` (lines 1403 and 1404) and computes each
  texture coordinate in double precision (lines 1433 to 1436; the comment above says it
  "needs to produce the same result that the light compiler does"); it warns rather than fails on a texture whose size is not a
  multiple of 16 (`Con_Warning ("Texture %s (%d x %d) is not 16 aligned\n", ...)`, line 704);
  and it limits a BSP29 map to 32767 leaves
  (`Host_Error ("Mod_LoadLeafs: %i leafs exceeds limit of 32767.", count);`, line 1884).
  P1-45 lists every such difference.
- DarkPlaces `model_brush.c`, lines 2739 and 2740, fails on lightmap size rather than extent:
  `if (ssize > 256 || tsize > 256)` then `Host_Error("Bad surface extents");`, where
  `ssize = (surface->lightmapinfo->extents[0] >> 4) + 1` (line 2708), and computes the
  extents in `int` (line 2703), not `short`.
- The 2021 rerelease's engine (KEX) has no published source. Not verified: no repository or
  source release was found, so its rules cannot be described from source.

`docs/spec/bsp.md` already describes BSP2 and `.lit` from QuakeSpasm at the same commit, as a
source read for description and not ported (`docs/porting.md` registers no QuakeSpasm upstream).

## Options

| Option | Consequences |
| --- | --- |
| WinQuake's rules only | The strictest loader: rejects faces with extents from 272 to 2000 that QuakeSpasm and its descendants load, so the editor would refuse maps modders ship. Removes `BspLoader`; P1-43's GLQuake vectors become history |
| GLQuake's rules only | id's own hardware renderer, but still rejects extents from 528 to 2000 and marks `SURF_UNDERWATER`, which the engines in use removed. Same refusals, smaller |
| No default (today) | Every caller picks, and the editor's own checks have no stated answer; each new caller repeats the choice, and they can drift apart |
| DarkPlaces' rules | A different lineage with its own limit (256 lightmap samples, `int` extents) and many extensions; few current single-player maps target it, and following it would mean describing a second large engine |
| **FitzQuake lineage, QuakeSpasm as reference (chosen)** | Matches what modders run. Needs a third `BspLoader` variant, the spec statements for its differences, and a recorded run of QuakeSpasm's loader; WinQuake and GLQuake stay selectable |

## Decision

The editor's default BSP loading rules follow the FitzQuake lineage (FitzQuake, QuakeSpasm,
Ironwail, vkQuake), with QuakeSpasm at
`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5` as the reference. Those rules are described in
`docs/spec/bsp.md` from QuakeSpasm's source, as BSP2 and `.lit` already are, and implemented
in `qs-bsp` from that description; QuakeSpasm's code is not ported. `BspLoader::WinQuake` and
`BspLoader::GlQuake` remain, unchanged, as compatibility modes a caller or a user may choose
explicitly. The reason is that the editor should accept and judge maps the way the engines
modders actually play on do, while still being able to answer "would this load in id's
engine" exactly.

## Consequences

- `plan/`: P1-45 adds the spec statements for the FitzQuake-lineage loader, a third
  `BspLoader` variant, the editor's default, vectors and a recorded differential run of
  QuakeSpasm's loader. It follows P1-44 and is a dependency of L0 readiness (P1-R0) and the
  Phase 1 gate (P1-25).
- `docs/spec/bsp.md` gains a section on the FitzQuake-lineage differences, and
  `docs/porting.md`'s table of authoritative sources records that the BSP default follows
  QuakeSpasm as described, while id's loaders stay authoritative for the WinQuake and GLQuake
  modes.
- An explicit choice of `WinQuake` or `GlQuake` keeps working exactly as before; nothing that
  names a loader changes behaviour.
- Harder: a third set of rules to keep in sync with its source, and QuakeSpasm, Ironwail and
  vkQuake may diverge from one another later; QuakeSpasm at the pinned commit is the answer
  until a later ADR moves it.

## Links

- Carried out by P1-45 (plan/backlog/p1.yaml); related to P1-43
  ([#311](https://github.com/quakecstudio/editor/issues/311)) and P1-44
  ([#314](https://github.com/quakecstudio/editor/issues/314)).
- Related ADRs: ADR-0002 (license model).
- Sources, each fetched and read for this decision:
  - [sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5](https://github.com/sezero/quakespasm/tree/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5):
    `Quake/gl_model.c`, `Quake/gl_model.h`
  - [andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd](https://github.com/andrei-drexler/ironwail/tree/08d578136ff43d7d1ef38e636dfbfd3e844be7cd):
    `Quake/gl_model.c`
  - [Novum/vkQuake@47726d899044ded94b20b242091d867b4e1cbb6d](https://github.com/Novum/vkQuake/tree/47726d899044ded94b20b242091d867b4e1cbb6d):
    `Quake/gl_model.c`
  - [DarkPlacesEngine/darkplaces@5d80f03e26871b273da103f3b750820a3718c01a](https://github.com/DarkPlacesEngine/darkplaces/tree/5d80f03e26871b273da103f3b750820a3718c01a):
    `model_brush.c`
  - [id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d](https://github.com/id-Software/Quake/tree/bf4ac424ce754894ac8f1dae6a3981954bc9852d):
    `WinQuake/gl_model.c`
