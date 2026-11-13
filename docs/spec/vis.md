# Visibility (vis)

Prefix: `VIS` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes what id's `vis` does: reading the portal file `qbsp` writes (QBSP-12), finding which
leaves each leaf may see, compressing each leaf's row into the BSP's visibility lump (BSP-6),
and setting each leaf's ambient sound levels. It is the contract for our `qs-map-vis` (P6-05) and
the vis half of the map compiler differential (P6-07). The engine's side, decompressing a row,
is BSP-6.

`vis` is in `qutils/` of id-Software/Quake-Tools, which is not a portable upstream until the
license review rules on it ([`../porting.md`](../porting.md#upstreams), P4-22). It may be read
and run as an oracle (`cargo xtask oracles maps`), so its behaviour is described from it and no
code is ported:

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| vis | [id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools)@`c0d1b91c74eb654365ac7755bc837e497caaca73` | `qutils/VIS/VIS.C`, `VIS.H`, `FLOW.C`, `SOUNDPVS.C`; `qutils/COMMON/BSPFILE.H` |

`vis` is built without `DOUBLEVEC_T`, as `qbsp` is (QBSP), so `vec_t` is `float`.

## VIS-1 The run and its modes

**Source:** vis `VIS.C`, `main`, `CalcVis`, `CalcPortalVis`.

### Behaviour

- **VIS-1.1** `vis [-threads n] [-level 0-4] [-fast] [-v] file` reads `file` with its extension
  replaced by `.bsp`, and the portal file with it replaced by `.prt`; any other option starting
  with `-` stops the run with `Unknown option "<option>"`. It writes the visibility lump, the
  leaves' `visofs` and their ambient levels back into the BSP.
- **VIS-1.2** A portal file that cannot be opened prints `LoadPortals: couldn't read <name>` and
  `No vising performed.` and exits with status 1, leaving the BSP unchanged.
- **VIS-1.3** The fast mode, `-fast`, takes each portal's visible leaves to be the leaves it
  might see (VIS-3), with no portal flow. The full mode, the default, runs the portal flow
  (VIS-4) at test level `-level`, 2 when not given.
- **VIS-1.4** Our `qs-map-vis` offers the same two modes: fast is VIS-1.3's fast mode, full its
  full mode at level 2. Levels 0, 1, 3 and 4 are options of the full mode.

## VIS-2 Reading the portal file

**Source:** vis `VIS.C`, `LoadPortals`, `PlaneFromWinding`.

### Behaviour

- **VIS-2.1** The file starts with the text `PRT1`, the number of vis leaves and the number of
  portals (QBSP-12.2). Another first word stops the run with `LoadPortals: not a portal file`,
  and a header not read stops it with `LoadPortals: failed to read header`.
- **VIS-2.2** Each portal is its point count, two leaf numbers and its points, each read as a
  `double` and stored as a `float`. A point count over `MAX_POINTS_ON_WINDING`, or a leaf number
  greater than the number of leaves (as unsigned, so a negative number too), stops the run.
- **VIS-2.3** Each portal of the file becomes two one-way portals: one in the first leaf,
  looking into the second, on the plane of its winding negated; one in the second leaf, looking
  into the first, on the winding's plane. Their plane is the winding's as QBSP-12.3 computes it
  (`cross(p0 - p1, p2 - p1)`, normalised, through `p0`).
- **VIS-2.4** A leaf given more than `MAX_PORTALS_ON_LEAF` portals stops the run with
  `Leaf with too many portals`.
- **VIS-2.5** Each leaf's row of visible leaves is `bitbytes` bytes in memory, the leaf count
  rounded up to a multiple of 64 and divided by 8; the row written to the BSP is
  `(leafs + 7) >> 3` bytes before compression (BSP-6.2).
- **VIS-2.6** A leaf number equal to the number of leaves passes VIS-2.2's check but names a leaf
  past the end of vis's array, which is undefined in C; this spec fixes the answer: it stops the
  run as a greater one does. Both one-way portals share the file portal's winding, its points in
  the file's order.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `VIS_ON_EPSILON` | 0.1 | `VIS.H`, `ON_EPSILON` |
| `VIS_MAX_POINTS_ON_WINDING` | 64 | `VIS.H`, `MAX_POINTS_ON_WINDING` |
| `VIS_MAX_PORTALS_ON_LEAF` | 128 | `VIS.H`, `MAX_PORTALS_ON_LEAF` |

## VIS-3 What a portal might see

**Source:** vis `FLOW.C`, `BasePortalVis`, `SimpleFlood`.

A first-order bound: the leaves a flood through portals that face each other can reach.

### Behaviour

- **VIS-3.1** For a portal `p`, another portal `t` is a candidate when some point of `t`'s
  winding is more than `ON_EPSILON` in front of `p`'s plane, and some point of `p`'s winding is
  more than `ON_EPSILON` behind `t`'s plane.
- **VIS-3.2** `p` might see the leaf it looks into, and every leaf a flood from it reaches
  through candidate portals only, each leaf entered once. The count of such leaves orders the
  portal flow (VIS-4.1).

## VIS-4 The portal flow

**Source:** vis `VIS.C`, `GetNextPortal`, `LeafThread`; `FLOW.C`, `PortalFlow`,
`RecursiveLeafFlow`, `ClipToSeperators`, `CheckStack`.

### Behaviour

- **VIS-4.1** Portals are flowed one at a time, each time the one not yet flowed that might see
  the fewest leaves (VIS-3.2), the earlier of equal ones; a portal already flowed passes on its
  exact result, a later one its might-see set.
- **VIS-4.2** A flow from portal `p` walks leaves depth first from the leaf `p` looks into. Each
  leaf reached is marked visible to `p`. From a leaf, each of its portals is followed only when
  the leaf it looks into is in every might-see set on the path so far, and the path's might-see
  set, narrowed by this portal's (its exact set when it has been flowed), still holds a leaf not
  yet marked.
- **VIS-4.3** A portal whose plane is the reverse of the previous portal's (normals compared with
  `EQUAL_EPSILON`) is not followed. The portal's winding is clipped to the front of `p`'s plane;
  for the first step past `p` that is all, and from the second step on it is also clipped to the
  front of the previous portal's plane, and the source (the windings of `p` so far) is clipped to
  the back of this portal's plane. A winding clipped away ends that path.
- **VIS-4.4** At test level 1 and up the target is clipped by the separating planes from the
  source to the pass (the previous portal); at level 2 and up also by those from the pass to the
  source, flipped; at level 3 and up the source is clipped by the planes from the target to the
  pass, and at level 4 by those from the pass to the target, flipped. A separating plane goes
  through an edge of the first winding and a point of the second, has every other point of the
  first on one side (more than `ON_EPSILON`) and every other point of the second on the other or
  on it, with at least one off it; a plane whose normal's squared length is below `ON_EPSILON`
  is skipped.
- **VIS-4.5** A leaf already on the path stops the run with `CheckStack: leaf recursion`.
- **VIS-4.6** A leaf sees every leaf one of its portals sees, and itself. A leaf whose portals
  see it already stops the run with `Leaf portals saw into leaf`.
- **VIS-4.7** vis flows portals on one thread unless built for the Alpha's threads, so each
  portal sees every earlier one as flowed. Our `qs-map-vis` flows them in VIS-4.1's order in
  batches of 32, on as many threads as the machine has; a portal of a batch sees the others of
  its batch as not yet flowed (their might-see sets, VIS-4.2), so the result is the same on any
  machine, and with a batch of one it is vis's single-thread run. A portal's exact set is a part
  of its might-see set, so a batch only narrows less, never hiding a leaf vis shows.

## VIS-5 The visibility lump

**Source:** vis `VIS.C`, `LeafFlow`, `CompressRow`.

### Behaviour

- **VIS-5.1** Each vis leaf's row is compressed as BSP-6.3 reads it: a non-zero byte is copied;
  a zero byte is written followed by the count of zero bytes in its run, at most 255, a longer
  run starting a new run.
- **VIS-5.2** Rows are written in leaf order, one after the other with no sharing, and vis leaf
  `n`'s `visofs` is its row's offset; the BSP's leaf is `n + 1`, leaf 0 being the shared solid
  leaf (QBSP-13.1). A lump past `MAX_MAP_VISIBILITY` stops the run with
  `Vismap expansion overflow`.
- **VIS-5.4** Our `qs-map-vis` writes the visibility lump after the BSP's other lumps, on a
  four-byte boundary, and points the directory at it, setting each vis leaf's `visofs` and
  `ambient_level` in place; every other byte of the file is kept.
- **VIS-5.3** The average printed, `average leafs visible`, counts a leaf's visible leaves with
  bit `i & 3` of byte `i >> 3` instead of bit `i & 7`, so it is not the true average; nothing
  else reads the count.

### Test vectors

| Row bytes | Compressed | Statements |
| --- | --- | --- |
| `ff 00 00 01` | `ff 00 02 01` | VIS-5.1 |
| `00` | `00 01` | VIS-5.1 |
| 300 bytes of `00` | `00 ff 00 2d` | VIS-5.1 |

## VIS-6 Ambient sounds

**Source:** vis `SOUNDPVS.C`, `CalcAmbientSounds`, `SurfaceBBox`; `COMMON/BSPFILE.H`,
`AMBIENT_WATER`, `AMBIENT_SKY`, `AMBIENT_SLIME`, `AMBIENT_LAVA`.

Each leaf's four `ambient_level` bytes (BSP-12) are the engine's automatic water, sky, slime and
lava sounds.

### Behaviour

- **VIS-6.1** A face marked by a leaf that a leaf sees is an emitter by its texture name, compared
  ignoring case over the bytes given: `*water` (6 bytes) water, `sky` (3) sky, `*slime` (6) water,
  `*lava` with its NUL (6 bytes, so only the name `*lava` itself) lava, `*04water` (8) water. Any
  other face is not.
- **VIS-6.2** The distance `vis` computes from a face to the leaf is replaced by 0.25 before it
  is used (`maxd = 0.25;`), so each kind has distance 0.25 when the leaf sees any emitter of it,
  and 1020 otherwise.
- **VIS-6.3** A kind at distance below 100 has level 255. Any other has level
  `(1 - dists[2] * 0.002) * 255`, clamped at 0, where `dists[2]` is the slime distance whatever
  the kind; slime faces count as water (VIS-6.1), so `dists[2]` is always 1020 and the level 0.
  So each of water, sky and lava is 255 when the leaf sees one, else 0, and slime is always 0.
- **VIS-6.4** A face whose texture's slot is a missing texture (offset -1, BSP-4.2) is read from
  before the textures lump, which is undefined in C; this spec fixes the answer: such a face is
  not an emitter.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `VIS_AMBIENT_FAR` | 1020 | `SOUNDPVS.C`, the initial distance |
| `VIS_AMBIENT_NEAR` | 100 | `SOUNDPVS.C`, `dists[j] < 100` |
