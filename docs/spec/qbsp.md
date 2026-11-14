# BSP compilation (qbsp)

Prefix: `QBSP` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes what id's map compiler does between reading a `.map` and writing a BSP29 file:
loading brushes and their contents, expanding them for the clipping hulls, CSG, building the
BSP tree, portals, filling the outside and finding leaks, merging faces, fixing T-junctions,
and writing the BSP, the clipping hulls, the portal file and the leak file. Reading the `.map`
itself, the texture axes and the textures lump are [MAP-1 to MAP-7](map.md); the BSP file's
layout is [BSP-1 to BSP-17](bsp.md). This spec is the contract for our map compiler (P6-02,
P6-03), whose tests will cite it, and for the qbsp differential (P6-07).

`qbsp` is in `qutils/` of id-Software/Quake-Tools, which is not a portable upstream until the
license review rules on it ([`../porting.md`](../porting.md#upstreams), P4-22). It may be read
and run as an oracle, so its behaviour is described from it and no code is ported:

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| qbsp | [id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools)@`c0d1b91c74eb654365ac7755bc837e497caaca73` | `qutils/QBSP/QBSP.C`, `BSP5.H`, `BRUSH.C`, `CSG4.C`, `SOLIDBSP.C`, `SURFACES.C`, `PORTALS.C`, `OUTSIDE.C`, `MERGE.C`, `TJUNC.C`, `REGION.C`, `WRITEBSP.C`, `MAKEFILE`, `QBSP.MAK`; `qutils/COMMON/MATHLIB.H`, `BSPFILE.H` |

Every statement below describes that commit. qbsp is built without `DOUBLEVEC_T`
(`MAKEFILE` and `QBSP.MAK` define only `WIN32`, `NDEBUG` and `_CONSOLE`), so `vec_t` is
`float` (`MATHLIB.H`): points, normals, distances and the arithmetic on them are single
precision unless a statement says otherwise.

## QBSP-1 The run

**Source:** qbsp `QBSP.C`, `main`, `ProcessFile`, `CreateHulls`, `CreateSingleHull`,
`ProcessEntity`.

### Behaviour

- **QBSP-1.1** `qbsp [options] source [dest]` reads `source` with `.map` added when it has no
  extension, and writes `dest`, or `source` with its extension replaced by `.bsp`. An option
  other than `-draw`, `-notjunc`, `-nofill`, `-noclip`, `-onlyents`, `-verbose`, `-usehulls`,
  `-hullnum <n>` and `-subdivide <n>` stops the run with `qbsp: Unknown option '<option>'`.
- **QBSP-1.2** Beside the BSP it names `<base>.h1` and `<base>.h2` (the clipping hulls), `<base>.prt`
  (the portal file) and `<base>.pts` (the leak file), where `<base>` is the BSP's path without
  its extension. Unless `-onlyents` is given it deletes the BSP, the portal file and the leak
  file first, and the two hull files unless `-usehulls` is given too.
- **QBSP-1.3** `-onlyents` reads the map and replaces only the entity lump of the existing
  BSP (`UpdateEntLump`, MAP-6); nothing else is compiled.
- **QBSP-1.4** Without `-hullnum`, `-usehulls` or `-noclip`, the hulls are built one after
  the other: hull 1, then hull 2, then hull 0, the drawing hull. Before hull 2 and before hull 0
  the counts of models, planes and clip nodes written so far are reset to 0, so each hull
  numbers its models and its output planes from 0; the brush plane table (QBSP-2) is not
  reset. `-hullnum <n>` builds only hull `n` and exits. `-usehulls` and `-noclip` build only
  hull 0.
- **QBSP-1.5** Each hull processes the entities in file order. An entity with no brushes is
  skipped. Every other entity but the world gets the key `model` set to `*<n>`, `n` the number
  of models written so far in this hull (MAP-6.2).
- **QBSP-1.6** An entity with brushes none of which is loaded in a hull, because each is left
  out (QBSP-4.1, QBSP-4.2) or has no face (QBSP-4.3), stops the run with
  `Entity with no valid brushes`: a brush entity made only of `clip` brushes fails in hull 0,
  and a world made only of water in hull 1.
- **QBSP-1.7** For each entity a hull runs, in order: load its brushes (QBSP-4, with QBSP-5 for
  hulls 1 and 2), CSG (QBSP-6), and a BSP tree (QBSP-7).
  - **Hull 0:** the world's tree is built with the mid-split choice and every other entity's
    with the full heuristic (QBSP-7.3). For the world, unless `-nofill`, it is portalized
    (QBSP-8) and filled (QBSP-9); when the fill succeeds the remaining faces are gathered again
    (QBSP-9.6), merged (QBSP-10), built into a new tree with the full heuristic, portalized
    again, the portal file written (QBSP-12), and T-junctions fixed (QBSP-11). The tree's planes,
    edges and faces, then nodes and leaves are written (QBSP-13).
  - **Hulls 1 and 2:** the tree is built with the mid-split choice. For the world, unless
    `-nofill`, it is portalized and filled; when the fill succeeds the faces are gathered again
    and built into a new tree with the full heuristic, with no merging. The tree's planes and
    clip nodes are written (QBSP-13.6), and the hull's text file (QBSP-13.7).
- **QBSP-1.8** After hull 0, hulls 1 and 2 are read back from their text files (QBSP-13.8),
  the entity lump is written from the entities as modified by QBSP-1.5, then the textures lump
  (MAP-7), and the BSP file.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_SUBDIVIDE_SIZE` | 240 | `QBSP.C`, `subdivide_size`, the default `-subdivide` |

## QBSP-2 Planes

**Source:** qbsp `BRUSH.C`, `PlaneTypeForNormal`, `NormalizePlane`, `FindPlane`.

### Behaviour

- **QBSP-2.1** Every face's plane is stored once in a table of planes and named by its index
  and a side: side 0 when the face faces the same way as the stored plane, 1 when it faces the
  other way.
- **QBSP-2.2** A plane is stored canonical: a normal with a component exactly -1 is negated,
  its distance with it, and the type is `PLANE_X`, `PLANE_Y` or `PLANE_Z` for a normal with that
  component exactly 1. Otherwise the type is `PLANE_ANYX`, `PLANE_ANYY` or `PLANE_ANYZ` for the
  axis with the largest absolute component, ties going to x before y before z, and the normal is
  negated, with the distance, when that component is negative.
- **QBSP-2.3** A face's plane matches a stored plane when the dot product of their canonical
  normals is greater than `1 - ANGLEEPSILON` and their distances differ by less than
  `DISTEPSILON`; the first match in table order is used. A plane whose normal's length is not
  within `ANGLEEPSILON` of 1 stops the run with `FindPlane: normalization error`.
- **QBSP-2.4** The side is 0 when the dot product of the canonical normal and the face's own
  normal is greater than 0, else 1.
- **QBSP-2.5** More than `MAX_MAP_PLANES` stored planes stops the run with
  `numbrushplanes == MAX_MAP_PLANES`.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_ANGLEEPSILON` | 0.00001 | `BRUSH.C`, `ANGLEEPSILON` |
| `QBSP_DISTEPSILON` | 0.01 | `BRUSH.C`, `DISTEPSILON` |
| `QBSP_MAX_MAP_PLANES` | 8192 | `COMMON/BSPFILE.H`, `MAX_MAP_PLANES`; the engine's header allows 32767 (BSP-16) |

### Test vectors

Derived from the C by reading; to be checked against qbsp by the differential (P6-07).

| Input plane | Stored plane, side | Statements |
| --- | --- | --- |
| normal (-1, 0, 0), dist 5 | (1, 0, 0), -5, `PLANE_X`; side 1 | QBSP-2.2, QBSP-2.4 |
| normal (0, 0.6, -0.8), dist 2 | (0, -0.6, 0.8), -2, `PLANE_ANYZ`; side 1 | QBSP-2.2, QBSP-2.4 |
| normal (0.6, 0.8, 0) then (0.6, 0.8, 0) with dist differing by 0.005 | one stored plane | QBSP-2.3 |

## QBSP-3 Windings

**Source:** qbsp `QBSP.C`, `BaseWindingForPlane`, `ClipWinding`, `DivideWinding`;
`CSG4.C`, `SplitFace`.

A winding is a convex polygon on a plane. Every polygon qbsp builds starts as a base winding
and is cut by other planes.

### Behaviour

- **QBSP-3.1** A plane's base winding is a square of side 16384 centred on the point
  `normal * dist`: its up vector is +z for a plane whose normal's largest absolute component is
  x or y, else +x, made perpendicular to the normal; the corners are
  `org - right + up`, `org + right + up`, `org + right - up`, `org - right - up`, with
  `right = up x normal` and both scaled to 8192.
- **QBSP-3.2** Each point is classed against a cutting plane by `d = dot(p, normal) - dist`:
  in front when `d > ON_EPSILON`, behind when `d < -ON_EPSILON`, else on the plane.
- **QBSP-3.3** Clipping keeps the front: a winding with no point in front is removed; one with
  no point behind is kept unchanged; a winding with every point on the plane is kept only when
  the clip keeps on-plane windings. Otherwise the points on the plane are kept, the points in
  front are kept, and where an edge crosses from front to back or back to front a point is
  added at `p1 + (d1 / (d1 - d2)) * (p2 - p1)`, except that a coordinate whose plane normal
  component is exactly 1 or -1 is set to `dist` or `-dist`.
- **QBSP-3.4** Dividing gives both sides: a winding with no point in front is all back, an
  all-on-plane winding included; one with no point behind is all front; otherwise each side
  gets its points, the on-plane points and the crossing points of QBSP-3.3.
- **QBSP-3.5** A face split this way with more than `MAXEDGES` points on a side stops the run
  with `SplitFace: numpoints > MAXEDGES`; a winding needing more than `MAX_POINTS_ON_WINDING`
  points stops it with `NewWinding: <n> points`.
- **QBSP-3.6** A base winding's largest normal component is found with a strict comparison
  from x to z, so of equal components the earlier axis decides its up vector.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_ON_EPSILON` | 0.05 | `BSP5.H`, `ON_EPSILON` (MAP-3.5) |
| `QBSP_BOGUS_RANGE` | 18000 | `BSP5.H`, `BOGUS_RANGE` |
| `QBSP_BASE_WINDING_HALF` | 8192 | `QBSP.C`, `BaseWindingForPlane` |
| `QBSP_MAXEDGES` | 32 | `BSP5.H`, `MAXEDGES` |
| `QBSP_MAX_POINTS_ON_WINDING` | 64 | `BSP5.H`, `MAX_POINTS_ON_WINDING` |

## QBSP-4 Brushes and contents

**Source:** qbsp `BRUSH.C`, `LoadBrush`, `CreateBrushFaces`, `CheckFace`,
`Brush_LoadEntity`.

### Behaviour

- **QBSP-4.1** A brush's contents come from the texture of the face `ParseBrush` stores first,
  which is the brush's last face in the file (`ParseBrush` adds each face to the front of the
  list):
  - `clip` (any case): the brush is left out of hull 0 and is solid in hulls 1 and 2.
  - a name starting `*`, in the world: `CONTENTS_LAVA` after `*lava`, `CONTENTS_SLIME` after
    `*slime` (any case), else `CONTENTS_WATER`. In any other entity the brush is solid.
  - a name starting `sky` (any case), in the world, in hull 0: `CONTENTS_SKY`.
  - anything else, and `sky` in hulls 1 and 2 or outside the world: `CONTENTS_SOLID`.
- **QBSP-4.2** In hulls 1 and 2 a water, slime or lava brush is left out.
- **QBSP-4.3** Each face's polygon is its base winding clipped by every other face's plane
  flipped, so the solid behind every plane is kept, with on-plane windings removed (QBSP-3.3).
  A face whose polygon is clipped away is dropped. A brush left with no face prints
  `WARNING: couldn't create brush faces` and is left out.
- **QBSP-4.4** Each coordinate of a polygon's points within `ZERO_EPSILON` of a whole number
  is set to that whole number. The brush's bounds are the smallest and largest coordinates of
  its points.
- **QBSP-4.5** Each face is checked, and the run stops with the first fault: fewer than 3
  points (`CheckFace: <n> points`); a coordinate beyond `BOGUS_RANGE`
  (`CheckFace: BUGUS_RANGE: <value>`, misspelt as in the source); a point more than
  `ON_EPSILON` off the plane (`CheckFace: point off plane`); an edge shorter than `ON_EPSILON`
  (`CheckFace: degenerate edge`); another point more than `ON_EPSILON` beyond the line of
  an edge, on the side `cross(facenormal, edge)` points to (`CheckFace: non-convex`). The checks
  run point by point in that order. A polygon of zero area with no such fault passes.
- **QBSP-4.6** **Withdrawn** 2026-11-12: it gave the liquids in file order and the solids in
  reverse file order, which is backwards; replaced by QBSP-4.9.
- **QBSP-4.7** In hulls 1 and 2 every face takes texinfo 0.
- **QBSP-4.8** Two faces of a brush on the same plane facing the same way clip each other away
  (QBSP-4.3), so neither is kept.
- **QBSP-4.9** An entity's brushes are ordered for CSG with the water, slime, lava and sky
  brushes first, in reverse file order, then the solid brushes in file order. `ParseBrush`
  puts each brush at the head of the entity's list (`MAP.C`, `b->next = mapent->brushes;`), so
  `Brush_LoadEntity` reads them last first and, putting each at the head of `water` or
  `other`, leaves both lists in file order; it then moves the water list onto the head of
  `other` one brush at a time (`// add all of the water textures at the start`), which reverses
  it. The entity's bounds are the union of its brushes' bounds.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_ZERO_EPSILON` | 0.001 | `BRUSH.C`, `ZERO_EPSILON` |
| `QBSP_CONTENTS_EMPTY` | -1 | `COMMON/BSPFILE.H`, `CONTENTS_EMPTY` |
| `QBSP_CONTENTS_SOLID` | -2 | `COMMON/BSPFILE.H`, `CONTENTS_SOLID` |
| `QBSP_CONTENTS_WATER` | -3 | `COMMON/BSPFILE.H`, `CONTENTS_WATER` |
| `QBSP_CONTENTS_SLIME` | -4 | `COMMON/BSPFILE.H`, `CONTENTS_SLIME` |
| `QBSP_CONTENTS_LAVA` | -5 | `COMMON/BSPFILE.H`, `CONTENTS_LAVA` |
| `QBSP_CONTENTS_SKY` | -6 | `COMMON/BSPFILE.H`, `CONTENTS_SKY` |

### Test vectors

| Brush (last face's texture), entity, hull | Contents | Statements |
| --- | --- | --- |
| `CLIP`, world, hull 0 | left out | QBSP-4.1 |
| `clip`, world, hull 1 | solid | QBSP-4.1 |
| `*SLIME0`, world, hull 0 | `CONTENTS_SLIME` | QBSP-4.1 |
| `*lava1`, `func_door`, hull 0 | solid | QBSP-4.1 |
| `*water0`, world, hull 2 | left out | QBSP-4.2 |
| `sky4`, world, hull 1 | solid | QBSP-4.1 |
| file order S1, W1, S2, W2 | CSG order W2, W1, S1, S2 | QBSP-4.9 |

## QBSP-5 Clipping hull expansion

**Source:** qbsp `BRUSH.C`, `ExpandBrush`, `AddBrushPlane`, `TestAddPlane`, `AddHullPoint`,
`AddHullEdge`, `hull_size`.

For hulls 1 and 2 each brush is grown by the box of the creature that collides with it, so a
point moving through the hull stops where the box would.

### Behaviour

- **QBSP-5.1** Hull 1's box is (-16, -16, -32) to (16, 16, 24); hull 2's is (-32, -32, -64)
  to (32, 32, 24).
- **QBSP-5.2** Each face plane is pushed out by `dot(corner, normal)`, where `corner` takes,
  per axis, the box's maximum when the normal's component is positive, its minimum when it is
  negative, and 0 when it is 0.
- **QBSP-5.3** The six axial planes of the brush's bounds grown by the box are added, each only
  when no face already has the same normal (compared with `EQUAL_EPSILON` per component) and a
  distance within `ON_EPSILON`.
- **QBSP-5.4** For each edge of each face polygon, with direction `e`, for each axis `a` and each
  of the four corners of the box's cross-section in the other two axes, the plane with normal
  `unit(a) x e` through the edge's first point moved to that corner is a bevel candidate, when
  that normal's length is within `ANGLEEPSILON` of 1. A candidate is added, flipped if needed
  so every corner of the box placed at every vertex of the brush is behind it or on it, only when
  all those corners are on one side of it (within `ON_EPSILON`) and no face has the same plane
  facing either way. An edge is considered once.
- **QBSP-5.5** The grown brush's faces are then built again as in QBSP-4.3 to QBSP-4.5.
- **QBSP-5.6** More than `MAX_HULL_POINTS` distinct vertices, `MAX_HULL_EDGES` distinct
  edges or `MAX_FACES` planes in one brush stops the run (`MAX_HULL_POINTS`, `MAX_HULL_EDGES`,
  `AddBrushPlane: numbrushfaces == MAX_FACES`).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_HULL1_MINS`, `QBSP_HULL1_MAXS` | (-16, -16, -32), (16, 16, 24) | `BRUSH.C`, `hull_size[1]` |
| `QBSP_HULL2_MINS`, `QBSP_HULL2_MAXS` | (-32, -32, -64), (32, 32, 24) | `BRUSH.C`, `hull_size[2]` |
| `QBSP_MAX_HULL_POINTS` | 32 | `BRUSH.C`, `MAX_HULL_POINTS` |
| `QBSP_MAX_HULL_EDGES` | 64 | `BRUSH.C`, `MAX_HULL_EDGES` |
| `QBSP_EQUAL_EPSILON` | 0.001 | `COMMON/MATHLIB.H`, `EQUAL_EPSILON` |

### Test vectors

| Input | Output | Statements |
| --- | --- | --- |
| box (0, 0, 0) to (64, 64, 64), hull 1 | box (-16, -16, -32) to (80, 80, 88), six faces, no bevels | QBSP-5.2, QBSP-5.3, QBSP-5.4 |
| box (0, 0, 0) to (64, 64, 64), hull 2 | box (-32, -32, -64) to (96, 96, 88) | QBSP-5.2 |

## QBSP-6 CSG

**Source:** qbsp `CSG4.C`, `CSGFaces`, `CopyFacesToOutside`, `ClipInside`, `FreeInside`,
`SaveOutside`, `BuildSurfaces`; `MERGE.C`, `MergeFaceToList`, `FreeMergeListScraps`.

CSG leaves the skin of the entity's solid: every face, or part of one, that is not inside
another brush.

### Behaviour

- **QBSP-6.1** Each brush `b1`, in the order of QBSP-4.9, starts with copies of its faces, each
  with contents `CONTENTS_EMPTY` on its front and the brush's contents behind. Each other brush
  `b2` whose bounds overlap `b1`'s (a gap on an axis, `b1.mins > b2.maxs` or
  `b1.maxs < b2.mins`, means no overlap; touching bounds overlap) cuts them by each of its
  faces' planes in turn: the parts behind all of `b2`'s planes are inside `b2`, the rest outside.
- **QBSP-6.2** **Withdrawn** 2026-11-12: its rule is right, but it concluded that among solid
  brushes the one earlier in the file keeps a shared face, from QBSP-4.6's wrong order;
  replaced by QBSP-6.7.
- **QBSP-6.3** When `b1` is solid and `b2` is water, slime, lava or sky, the parts inside
  `b2` are kept, their front contents set to `b2`'s; otherwise they are dropped.
- **QBSP-6.4** The parts of `b1` left outside are its faces. For a brush that is not solid
  each face is also added facing the other way, its points reversed and its two contents
  swapped, so it is seen from inside the water.
- **QBSP-6.5** Each face is merged (QBSP-10.1) into the faces kept on its plane so far as it is
  added.
- **QBSP-6.6** The result is one surface per stored plane with faces left, holding the faces of
  both sides, its bounds those of its faces. A face whose front or back contents is not negative
  stops the run with `Bad contents`.
- **QBSP-6.7** A part of `b1` lying on one of `b2`'s planes is inside when it faces the other
  way, or when `b2` comes after `b1` in the order of QBSP-4.9 (`ClipInside`'s `precedence`,
  which `CSGFaces` sets once its loop has passed `b1`: `overwrite = true; // later brushes now
  overwrite`); otherwise it stays outside. So of two coincident faces facing the same way, the
  one of the brush later in the order is kept: among solid brushes, the one later in the file.

## QBSP-7 The BSP tree

**Source:** qbsp `SOLIDBSP.C`, `SolidBSP`, `PartitionSurfaces`, `SelectPartition`,
`ChooseMidPlaneFromList`, `ChoosePlaneFromList`, `FaceSide`, `DividePlane`,
`DivideNodeBounds`, `LinkConvexFaces`, `LinkNodeFaces`; `SURFACES.C`, `SubdivideFace`.

### Behaviour

- **QBSP-7.1** The head node's bounds are the entity's bounds grown by `SIDESPACE` on every side.
- **QBSP-7.2** A node with no surface left that has not been a splitter is a leaf. With one,
  that surface splits it. Otherwise a splitter is chosen from the surfaces that have not been
  one, over the bounds of all the node's surfaces.
- **QBSP-7.3** An axial plane's `value` over the node's bounds is, for its axis `l` and
  `dist = plane.dist * normal[l]`, `(maxs[l] - dist)^2 + (dist - mins[l])^2` plus
  `2 * (maxs[j] - mins[j])^2` for each other axis `j`. The mid-split choice takes, among the
  axial surfaces, the one with the least `value`, ties going to the later in the list; with no
  axial surface it takes the first surface.
- **QBSP-7.4** The splitting surface's faces become the node's faces, each first subdivided
  (QBSP-7.8). Each surface of the node is divided by the splitter's plane: a surface on the same
  plane goes to the front with its faces of side 0 and to the back with its faces of side 1,
  both marked as having split; a surface on a parallel plane with the same normal goes wholly
  front when its distance is greater, else back; any other is split face by face (QBSP-3.4).
  Normals are compared with `EQUAL_EPSILON` per component, distances exactly.
- **QBSP-7.5** A non-axial splitter leaves both children with the parent's bounds; an axial one
  sets the front child's minimum and the back child's maximum on its axis to the plane's
  distance.
- **QBSP-7.6** A leaf's contents are the front contents of its faces: all must agree, else the
  run stops with `Mixed face contents in leafnode`. A leaf with no faces is solid. A leaf whose
  contents is not empty, solid, water, slime, lava or sky stops the run with
  `LinkConvexFaces: bad contents number`.
- **QBSP-7.7** A leaf's marked faces are the node faces (QBSP-7.4) from which the fragments in
  it came.
- **QBSP-7.8** A face whose texinfo does not have `TEX_SPECIAL` is cut while its extent along
  either texture axis, `max - min` of `dot(point, vecs[axis])` over its points, is greater than
  `subdivide_size`: it is split by the plane with that axis's normal at
  `(min + subdivide_size - 16) / length(vecs[axis])`, s before t. A split leaving one side empty
  stops the run with `SubdivideFace: didn't split the polygon`.
- **QBSP-7.9** The full heuristic walks the surfaces in list order, keeping a best split count
  `k` (from 99999) and a best `value` (from 9e30). For each it counts `k'`, the faces of the
  other surfaces its plane splits (a face with points more than `ON_EPSILON` on both sides).
  It skips the surface when `k' > k`. When `k' < k`, or `k' == k` and the plane is axial, it
  becomes the best: an axial plane first computes its `value` and is skipped if that is greater
  than the best `value` while `k' == k`, and otherwise sets the best `value`. A non-axial plane
  never wins a tie, and the best `value` is not reset when one wins with a lower count, so a later
  axial plane with that count must beat the `value` of an earlier axial best.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_SIDESPACE` | 24 | `BSP5.H`, `SIDESPACE` |
| `QBSP_SUBDIVIDE_MARGIN` | 16 | `SURFACES.C`, `SubdivideFace` |

## QBSP-8 Portals

**Source:** qbsp `PORTALS.C`, `PortalizeWorld`, `MakeHeadnodePortals`, `CutNodePortals_r`,
`AddPortalToNodes`.

### Behaviour

- **QBSP-8.1** Six portals bound the head node, on the axial planes of the entity's bounds grown
  by `SIDESPACE`, each clipped by the other five, joining the head node to the outside node,
  whose contents are solid.
- **QBSP-8.2** At each node a new portal on the node's plane is its base winding clipped by the
  plane of every portal of the node, each facing into the node (keeping on-plane windings,
  QBSP-3.3); it joins the front child to the back child. When it is clipped away the run prints
  `WARNING: CutNodePortals_r:new portal was clipped away` and makes no portal.
- **QBSP-8.3** Each portal of the node is divided by the node's plane (QBSP-3.4), and each part
  joins the child on its side to the node at the portal's other end.
- **QBSP-8.4** When the recursion ends every leaf holds the portals to the leaves it touches, and
  to the outside node.

## QBSP-9 The outside fill and leaks

**Source:** qbsp `OUTSIDE.C`, `FillOutside`, `PlaceOccupant`, `PointInLeaf`,
`RecursiveFillOutside`, `ClearOutFaces`, `MarkLeakTrail`; `SURFACES.C`, `GatherNodeFaces`.

### Behaviour

- **QBSP-9.1** Each entity after the world whose origin is not exactly (0, 0, 0) is placed in
  the leaf its origin is in (descending to the front child when
  `dot(normal, origin) - dist > 0`, else the back). One placed in a solid leaf is not an
  occupant.
- **QBSP-9.2** With no occupant the run prints `Hullnum <n>: No entities in empty space -- no
  filling performed` and the fill fails, so the tree is kept as it is.
- **QBSP-9.3** From the leaf on the other side of the outside node's first portal, the leaves
  joined by portals are visited, not passing into solid or sky leaves; water, slime and lava are
  passed through. Reaching an occupied leaf
  is a leak: nothing is filled, the fill fails, and the compile goes on with the tree as built,
  outside faces included.
- **QBSP-9.4** Without a leak every leaf visited is made solid, and every face a solid leaf
  marks (QBSP-7.7) is emptied, so faces seen only from outside are removed.
- **QBSP-9.5** In hull 0, once there is an occupant, the leak file `<base>.pts` is opened
  before the search and closed after it, and nothing is written to it: the trail would be
  written by `MarkLeakTrail`, which runs only while `backdraw` is greater than 0, and
  `backdraw` is a zeroed global that nothing sets. So a leak leaves an empty `.pts` file, and a
  sealed map one too; a map with no occupant (QBSP-9.2) leaves none.
- **QBSP-9.6** After a fill the remaining faces of every node are gathered by plane into
  surfaces again (QBSP-6.6), dropping the emptied ones.
- **QBSP-9.7** With `-verbose`, a leak prints `reached occupant at: (x, y, z)` with the origin of
  entity 0, the world, not of the occupant reached: `hit_occupied` is a zeroed global that
  nothing sets.
- **QBSP-9.8** QuakeC Studio's compiler also gives a leak's trail, which id's does not
  (QBSP-9.5 leaves its leak file empty, and that file stays as id writes it): the origin of the
  entity the search reached, then the middle of each portal on the way from its leaf to the
  outside node that crosses the fewest portals, not passing solid or sky leaves, the last the
  portal onto the outside node; of ways equally short, the one whose portals come first in each
  leaf's list. A map that does not leak has none. Its points are the mean of each portal winding's
  points, so the line through them crosses each portal.

## QBSP-10 Merging faces

**Source:** qbsp `MERGE.C`, `TryMerge`, `MergeFaceToList`, `MergePlaneFaces`, `MergeAll`.

### Behaviour

- **QBSP-10.1** Two faces merge when they have the same side, texinfo and both contents, share
  an edge with its ends equal within `EQUAL_EPSILON` per coordinate and running the other way,
  and the polygon of both is convex at the two ends of that edge, each end's turn within
  `CONTINUOUS_EPSILON`; an end where the edges meet in a straight line is dropped. The merge
  needs at most `MAXEDGES` points in total.
- **QBSP-10.2** A face added to a plane's list is tried against each face in it in turn; on the
  first merge the merged face is added again in its place, so merging repeats until nothing more
  merges.
- **QBSP-10.3** In hull 0 the world's faces are merged again, plane by plane, after the fill
  (QBSP-1.7).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_CONTINUOUS_EPSILON` | 0.001 | `MERGE.C`, `CONTINUOUS_EPSILON` |

## QBSP-11 T-junctions

**Source:** qbsp `TJUNC.C`, `tjunc`, `AddFaceEdges`, `FindEdge`, `AddVert`, `FixFaceEdges`,
`SplitFaceForTjunc`.

A vertex of one face lying on an edge of a neighbour leaves a crack when drawn; qbsp adds it
to the neighbour's edge.

### Behaviour

- **QBSP-11.1** Unless `-notjunc`, every edge of every node face of the world's final tree is
  put on its line, the lines found by direction (made canonical: the first non-zero component,
  beyond `EQUAL_EPSILON`, positive) and by the point of the line nearest the origin, both within
  `EQUAL_EPSILON`; each line keeps the distances along it of its edges' ends, ends within
  `T_EPSILON` of each other being one.
- **QBSP-11.2** Each face edge gets every point of its line strictly between its ends, more than
  `T_EPSILON` from both, inserted in order.
- **QBSP-11.3** A face left with more than `MAXPOINTS` points is cut into faces of at most
  `MAXPOINTS` points along runs of collinear points; the pieces stay linked to the face they came
  from, so a leaf marking it marks them all (QBSP-13.4).
- **QBSP-11.4** More than `MAXWEDGES` lines or `MAXWVERTS` points stops the run
  (`FindEdge: numwedges == MAXWEDGES`, `AddVert: numwverts == MAXWVERTS`).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_T_EPSILON` | 0.01 | `TJUNC.C`, `T_EPSILON` |
| `QBSP_MAXPOINTS` | 28 | `BSP5.H`, `MAXPOINTS` |
| `QBSP_MAXWEDGES` | 0x10000 | `TJUNC.C`, `MAXWEDGES` |
| `QBSP_MAXWVERTS` | 0x20000 | `TJUNC.C`, `MAXWVERTS` |

## QBSP-12 The portal file

**Source:** qbsp `PORTALS.C`, `WritePortalfile`, `NumberLeafs_r`, `WritePortalFile_r`,
`WriteFloat`, `PlaneFromWinding`.

### Behaviour

- **QBSP-12.1** Leaves are numbered in tree order, front child first, skipping solid leaves,
  which get -1: these are the vis leaves.
- **QBSP-12.2** The file is the text `PRT1`, the number of vis leaves and the number of portals,
  each on its own line, then one line per portal between two leaves of the same contents,
  written once from its front leaf: the point count, the two leaf numbers, and each point as
  `(x y z )`.
- **QBSP-12.3** The leaf numbers are front then back, unless the portal's winding faces away from
  its plane (the dot product of its plane's normal and the winding's own is less than 0.99), when
  they are back then front.
- **QBSP-12.4** A coordinate within 0.001 of a whole number is written as that integer, others
  with `%f`; each is followed by a space.
- **QBSP-12.5** A portal between leaves of different contents, water and empty for example, is
  not written, and not counted.

## QBSP-13 Writing the BSP

**Source:** qbsp `WRITEBSP.C`, `BeginBSPFile`, `WriteNodePlanes`, `WriteClipNodes`,
`WriteDrawNodes`, `WriteLeaf`, `BumpModel`, `FindFinalPlane`; `SURFACES.C`,
`MakeFaceEdges`, `GetVertex`, `GetEdge`; `REGION.C`, `GrowNodeRegions`; `QBSP.C`,
`WriteClipHull`, `ReadClipHull`.

### Behaviour

- **QBSP-13.1** Leaf 0 is a solid leaf with no faces, and edge 0 is unused: the first leaf and
  the first edge written are number 1.
- **QBSP-13.2** Each model's planes are written in tree order, front first, each stored plane
  (QBSP-2) once per model, as BSP-3 describes.
- **QBSP-13.3** A point's coordinates within 0.001 of a whole number are set to it. The point
  is then the vertex already written in the same model within `POINT_EPSILON` per coordinate,
  among those in its hash bucket, or a new vertex. The bucket is
  `0.004 * (x + 8000) * 64 + 0.004 * (y + 8000)` converted to an unsigned integer, at most 4095;
  two points within `POINT_EPSILON` in different buckets are two vertices. An edge from `v1` to
  `v2` is the reverse of an edge already written from `v2` to `v1` in the same model, used by one
  face only so far, whose face has the same front contents, and is written as `-n`; otherwise it
  is a new edge.
- **QBSP-13.4** Faces are written node by node in tree order, each node's own faces first; each
  face is one BSP face with its node's plane, its side, its texinfo, its edges as surfedges in
  order, no light styles (all 255) and light offset -1. Faces are not grown into larger regions
  (that code is compiled out).
- **QBSP-13.5** Nodes are written in tree order, front child first. A child that is a solid leaf
  is written as -1, leaf 0; any other leaf is written as a leaf, its contents, bounds,
  visibility offset -1 and marksurfaces (each marked face and the faces T-junction fixing cut from
  it), and named as `-(n + 1)`. Node and leaf bounds are the tree's float bounds converted to the
  file's 16-bit integers by C's conversion, toward zero. Each model's `visleafs` is the number of
  leaves it wrote, and its bounds, floats, are its head node's bounds shrunk by `SIDESPACE + 1`,
  that is the entity's bounds shrunk by 1 on every side. A model whose head node is a leaf
  writes that leaf and no node.
- **QBSP-13.6** A clipping hull's nodes are written in tree order, front first; a leaf child is
  written as its contents. The model's head clip node for that hull is set.
- **QBSP-13.7** Hulls 1 and 2 are passed to the hull 0 pass as text files: the number of models,
  each model's head clip node, a blank line, the number of clip nodes, then one line per clip
  node, `<n> : <nx> <ny> <nz> <dist> : <child0> <child1>`, the numbers with `%f`.
- **QBSP-13.8** Reading a hull file back, its clip nodes are appended after those already
  written, their node children and each model's head clip node offset by the number before
  them, and each plane is the first
  output plane exactly equal in type, distance and normal, or a new one; its type is found from
  the normal read back. So each clip node's plane has passed through `%f`, rounded to six
  decimal places. A hull file whose model count differs from hull 0's stops the run with
  `ReadClipHull: hull had <n> models, base had <m>`.
- **QBSP-13.9** id's qbsp writes only BSP29 (`BSPVERSION` 29, BSP-1.2). It has no BSP2 output;
  when a map needs more than BSP29's fields hold (BSP-16.2), a BSP2 file (BSP-17) is the
  choice of a later compiler, and ours (P6-02) decides it.
- **QBSP-13.10** Exceeding a `MAX_MAP_` limit while writing stops the run with an error naming
  it; the limits are those of `COMMON/BSPFILE.H`, which match BSP-16 but for `MAX_MAP_PLANES`
  (QBSP-2.5).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QBSP_POINT_EPSILON` | 0.01 | `SURFACES.C`, `POINT_EPSILON` |
| `QBSP_BSPVERSION` | 29 | `COMMON/BSPFILE.H`, `BSPVERSION` |

## QBSP-14 The extended dialect

**Source:** ours, but for QBSP-14.7 and QBSP-14.8, which follow ericw-tools' qbsp at
`36eec1da2a194467e6baac2f444c2dd04b57d266` (`qbsp/map.cc`, `qbsp/brush.cc`). id's qbsp is
`Dialect::Id`, which follows QBSP-1 to QBSP-13 exactly; `Dialect::Extended`, the default of
`crates/qs-map-bsp`, is what a map made for a later engine and editor needs, and each difference
is stated here. Later compilers lift the same limits; we follow BSP-17's file format, and
ericw-tools only where a map made in TrenchBroom relies on it (Justin, 2026-11-13).

- **QBSP-14.1** The extended dialect reads the map as `qs-map`'s `MapFile` reads it (MAP-1 to
  MAP-7): a face's points are its numbers as written, in single precision, and its texture axes
  the standard form's (MAP-4) or the Valve 220 form's (MAP-5). Duplicate planes, faces with no
  normal, texture slots and texinfo are then found as `ParseBrush`, `FindMiptex` and
  `FindTexinfo` find them, a texture name kept to its first 15 bytes, with no limit on entities
  and up to 32767 texinfo, BSP29's signed 16-bit field.
- **QBSP-14.2** A brush may have any number of faces and planes, and a grown brush any number
  of vertices and edges: QBSP-5.6's `MAX_FACES`, `MAX_HULL_POINTS` and `MAX_HULL_EDGES` do not
  apply, nor does QBSP-2.5's `MAX_MAP_PLANES`, nor QBSP-3.5's `MAXEDGES` on a split face.
- **QBSP-14.3** A fault qbsp stops on is a warning, and the compile goes on: a face
  `CheckFace` refuses (QBSP-4.5) is kept as it is; a leaf whose faces disagree on their front
  contents (QBSP-7.6) takes the first face's; an entity with brushes none of which is loaded in
  a hull (QBSP-1.6) gets an empty model: in hull 0 one leaf at its origin under one node on the
  plane z = 0, both of whose children are that leaf, and in hulls 1 and 2 the leaf's contents as
  the head. Where any tree's head is a leaf, the extended dialect writes the same node over it
  and names its contents as each clipping hull's head; qbsp names the next node and clip node
  (QBSP-13.5, QBSP-13.6), which may not exist.
- **QBSP-14.4** No `MAX_MAP_` limit (QBSP-13.10) applies. The file is written as BSP29 when
  every value fits BSP29's fields (BSP-23.5), else as `BSP2` (BSP-23.4), with a warning naming
  the first lump that did not fit.
- **QBSP-14.5** The entity lump is written as QBSP-1.8 writes it, but with no limit on the
  length of a key and value's line.
- **QBSP-14.6** **Withdrawn** 2026-11-13: it promised the same BSP from both dialects for any
  map the id dialect compiles; a map with a world brush entity (QBSP-14.8) now differs.
  Replaced by QBSP-14.9.

- **QBSP-14.7** The extended dialect expands each external map instance (MAP-14) before it reads
  the map, with the files its caller reads by the names the instances give, as ericw-tools'
  qbsp does; an instance that cannot be expanded stops the compile. id's dialect reads no
  instance: to id's qbsp a `misc_external_map` is a point entity like any other.
- **QBSP-14.8** The extended dialect takes ericw-tools' world brush entities into the world, as
  `IsWorldBrushEntity` (`qbsp/map.cc`) names them: an entity after the world whose classname,
  in any case (`Q_strcasecmp`), is `func_group`, `func_detail`, `func_detail_wall` or
  `func_detail_fence` gives its brushes to the world, after the world's own, solid in every
  hull, and leaves the entity lump ("If this is the world entity, find all func_group and
  func_detail entities and add their brushes", `qbsp/brush.cc`). ericw-tools gives the detail
  kinds detail contents in hull 0, which change how the world is split and seen but not what is
  solid; this dialect makes them plain solid. A `func_detail_illusionary`, which ericw-tools
  leaves out of the clipping hulls ("func_detail_illusionary don't exist in the collision
  hull"), becomes id's `func_illusionary`, an entity of its own the game draws and nothing
  collides with; a `func_illusionary_visblocker` is left out with its brushes.
- **QBSP-14.9** Everything QBSP-1 to QBSP-13 state that QBSP-14.1 to QBSP-14.5, QBSP-14.7 and
  QBSP-14.8 do not change holds in the extended dialect: the same map compiled by both dialects,
  when the id dialect compiles it and no entity of it is a world brush entity (QBSP-14.8), gives
  the same BSP, portal file and leak file.

## QBSP-15 The extended dialect from ericw-tools

**Source:** ericw-tools at `ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266`,
ported under [ADR-0023](../adr/0023-ericw-tools-porting-upstream.md): `include/common/qvec.hh`,
`include/common/polylib.hh`, `include/common/mathlib.hh`, `qbsp/map.cc`, `qbsp/qbsp.cc`. The
plan is `docs/design/extended-qbsp.md`; until its pipeline is complete the extended dialect
compiles as QBSP-14 says.

ericw-tools works in double precision (`qvec3d`, `qvec<double, 3>`) with its own epsilons:
`NORMAL_EPSILON` 0.000001 and `DIST_EPSILON` 0.0001 for planes, `DEFAULT_ON_EPSILON` 0.1 for a
point on a plane (`mathlib.hh`).

### Behaviour

- **QBSP-15.1** A plane is normalized as `qbsp_plane_t::normalize` does: a normal with a
  component exactly 1.0 or -1.0 is that axis alone, "`plane.normal[(i + 1) % 3] = 0;
  plane.normal[(i + 2) % 3] = 0;`", its type `PLANE_X`, `PLANE_Y` or `PLANE_Z`; any other
  normal's type is `PLANE_ANYX` when "`ax >= ay && ax >= az`", else `PLANE_ANYY` when "`ay >= ax
  && ay >= az`", else `PLANE_ANYZ`. Asked to flip, a plane is turned to face its axis's positive
  side and says it was.
- **QBSP-15.2** The plane table (`mapdata_t::add_plane`) adds each new plane with its opposite
  ("`planes.emplace_back(plane); planes.emplace_back(-plane);`"), the one whose normal is not
  negative along its type's axis first ("`if (positive.get_normal()[static_cast<int32_t>
  (positive.get_type()) % 3] < 0.0) { std::swap(positive, negative);`"), so plane `n ^ 1` is
  plane `n`'s opposite, and gives the number of the plane it was asked for.
- **QBSP-15.3** A plane is found in the table (`find_plane_nonfatal`) when a plane of it lies
  within half `NORMAL_EPSILON` of it in each normal component and half `DIST_EPSILON` in
  distance ("`HALF_NORMAL_EPSILON = NORMAL_EPSILON * 0.5`", "`HALF_DIST_EPSILON = DIST_EPSILON *
  0.5`"); `add_or_find_plane` gives that plane's number, else adds the plane. Where several
  qualify, ericw-tools takes the one its spatial hash finds; the port takes the first added.
- **QBSP-15.4** A box (`aabb3d`) starts empty and widens to each point added.
- **QBSP-15.5** A winding's area is "`total += 0.5 * qv::length(qv::cross(d1, d2))`" over the
  fan of triangles from its first point (`area`); its centre is the mean of its points.
- **QBSP-15.6** A winding drops each point whose directions in and out are within
  `DIST_EPSILON` of parallel: a point is kept when "`qv::dot(v1, v2) < 1.0 - DIST_EPSILON`"
  (`remove_colinear`).
- **QBSP-15.7** The winding of a plane (`from_plane`) is a square reaching the world's extent
  each way from the plane's point nearest the origin: its up vector the z axis, or the x axis
  when the normal's largest component is z, with the normal's part taken out and normalized;
  its right vector "`qv::cross(vup, plane.normal)`"; its corners "`org - vright + vup`", "`org +
  vright + vup`", "`org + vright - vup`", "`org - vright - vup`".
- **QBSP-15.8** A winding is clipped by a plane (`calc_sides`, `clip`) by each point's side
  within the epsilon given: with none in front all of it is behind, with none behind all of it in
  front, and, kept on, one wholly on the plane is in front. A point on the plane goes to both
  parts; an edge crossing gives both the point where it crosses, "`avoid round off error when
  possible`": a component of an axial normal is the plane's distance, 1 giving `dist` and -1
  `-dist`.
- **QBSP-15.9** Contents are ericw-tools' `contentflags_t`, a superset for every game: the
  visible contents solid, sky, detail wall, window, visblocker, aux, lava, slime, water and mist
  (bits 0 to 9, strongest first), the invisible contents origin, player clip, monster clip, area
  portal and projectile clip, and flags among them mirror inside, translucent and detail
  (`bspfile_common.hh`). Detail contents are made as `create_detail_*_contents` makes them: solid
  "`flags |= (EWT_VISCONTENTS_SOLID | EWT_CFLAG_DETAIL)`"; a wall not solid, a detail wall,
  detail; a fence not solid, a window, translucent, detail; illusionary not solid, mist,
  translucent, detail. "Solid" is structural solid: "`(flags & EWT_VISCONTENTS_SOLID) && !(flags
  & EWT_CFLAG_DETAIL)`"; a leaf "`seals_map`" when solid or sky.
- **QBSP-15.10** Two contents in one place combine (`combine_contents`) as both, but structural
  solid, sky or a visblocker on either takes away both's detail and translucency; a cluster
  (`cluster_contents`) is both, solid only when both are.
- **QBSP-15.11** A portal between two contents shows the strongest visible contents that change
  across it (`portal_visible_contents`): none between solid or sky on both sides, else of the
  two's exclusive or, or of both when either suppresses clipping of its own type. A brush's side
  gives a face for it (`portal_generates_face`) when the brush has those contents; on the side's
  back only when the brush mirrors inside, and by default not for a window, aux or detail wall.
  A portal can be seen through (`portal_can_see_through`) unless either side is solid, when both
  have the same visible contents, or would once translucent or detail sides count as empty.
- **QBSP-15.12** Quake's native contents are `CONTENTS_EMPTY` -1 to `CONTENTS_SKY` -6. Contents
  are written (`contents_to_native`) as the first of solid, sky, lava, slime and water they
  have, empty with none; a detail wall, window, visblocker, aux or mist met first cannot be
  written ("not representable in Q1"). Before writing (`contents_remap_for_export`) a window or
  detail wall becomes solid, and mist, aux and a visblocker are cleared. The clipping hulls are
  the point, -16 -16 -32 to 16 16 24, and -32 -32 -64 to 32 32 24 (`get_hull_sizes`).
- **QBSP-15.13** A face's contents come from its texture's name (`face_get_contents`), in any
  case: `origin` origin; `hint` or `hintskip` empty; `clip` player clip; a name starting `*` lava
  ("`*lava`"), slime ("`*slime`") or else water, detail and translucent while `-transwater`, its
  default, is on; a name starting `sky` sky; anything else solid.
- **QBSP-15.14** A face's plane is its three points' (`mapface_t::set_planepts`): the normal of
  "`qv::cross(ab, cb)`" with `ab = p0 - p1` and `cb = p2 - p1`, normalized, a zero vector left
  as it is (`qv::normalize`), and "`double dist = qv::dot(planepts[1], normal)`"; the plane is
  added to the table even when "`length >= NORMAL_EPSILON`" fails, and the face is then dropped
  ("Brush plane with no normal"). A face whose plane is `qv::epsilonEqual` to an earlier face's
  in the brush is dropped (`ParseBrush`, "Brush with duplicate plane"); one on the opposite
  plane is kept. Two planes are `epsilonEqual` when both are axial, of one type and with the
  same normal component on it, and their distances within `DIST_EPSILON`; otherwise when their
  distances are within `DIST_EPSILON` and each normal component within `NORMAL_EPSILON`
  (`qbsp.cc`).
- **QBSP-15.15** A brush's contents (`Brush_GetContents`) are those of its first face whose
  texture's contents (QBSP-15.13) are not empty, "`contentflags_t::make(EWT_VISCONTENTS_EMPTY)`"
  when none is; a later face of another type ends the search ("the former will be used"). The
  entity's `_mirrorinside`, when present, sets mirror inside on when nonzero and off when zero
  (`set_mirrored`), and with no key both flags are cleared; a nonzero `_noclipfaces` sets
  "`EWT_CFLAG_SUPPRESS_CLIPPING_SAME_TYPE`" (`set_clips_same_type(false)`). A
  `func_illusionary_visblocker`'s contents lose every visible contents, detail and translucency
  and become "`EWT_VISCONTENTS_ILLUSIONARY_VISBLOCKER`".
- **QBSP-15.16** A face's texture (`FindMiptex`, Quake) is the name after its last `/`, found
  among the map's textures in any case (`Q_strcasecmp`) or added; it is found while the texture
  definition is read, before the face can be dropped (QBSP-15.14). Its surface flags
  (`SurfFlagsForEntity`): nodraw for `skip`, `*waterskip`, `*slimeskip`, `*lavaskip`, `bevel` or
  `null` (`IsSkipName`); hint for `hint` or `hintskip` (`IsHintName`); `TEX_SPECIAL` for a name
  starting `*` unless the entity's `_litwater`, or else `_splitturb`, is above 0, or one starting
  `sky` (`IsSpecialName`), each in any case. A face nodraw, hint or hintskip gets the standard
  axes of its plane with no shift, rotation or scale, "to save on texinfo". The texinfo is found
  (`FindTexinfo`) by exact comparison or added; its axes are `float`.
- **QBSP-15.17** In a brush with a hint face, every face that is not hint is marked hintskip
  (`ParseBrush`: "any face in a hint brush that isn't HINT are treated as \"hintskip\""); when
  every face is hintskip, none is ("which is invalid; ignoring this"). A brush with no face left
  is not kept (`ParseEntity`), nor any brush of an entity whose `_omitbrushes` is nonzero.
- **QBSP-15.18** A brush's bounds (`CalculateBrushBounds`) are around its sides' polygons: each
  side's plane's square of the world's extent (QBSP-15.7) clipped to the front of the opposite
  of each other side's plane but the bevels' with no epsilon (`clip_front(..., 0)`), until
  nothing is left; a side clipped away keeps the polygon it had.
- **QBSP-15.19** The world's extent (`CalculateWorldExtent`) is "`ceil((extents +
  hull_extents) * 2) + SIDESPACE`", `SIDESPACE` 24: `extents` the largest absolute coordinate of
  any brush corner, each point where three of a brush's planes meet (`GetIntersection`, none
  when "`denom == 0.f`") that no plane has more than `NORMAL_EPSILON` in front
  (`GetBrushExtents`); `hull_extents` the largest absolute coordinate of the hull sizes
  (QBSP-15.12). It is measured after every map is read and before any brush's bounds.
- **QBSP-15.20** Each brush gets bevels (`AddBrushBevels`, `QBSP3`): for each axis and each
  direction, -1 then 1, a side whose normal on the axis "`== dir`" exactly, or a new one at the
  bounds' maximum or minus minimum with the first side's texture; the side found is swapped into
  place so the six come first in the order -x, +x, -y, +y, -z, +z. A brush of six sides is done.
  Otherwise each edge of each side from the seventh on that is at least 0.5 long and, its
  direction snapped (`qv::Snap`, a component within `NORMAL_EPSILON` of 1 or -1 making it that
  axis alone), not along an axis, gives for each axis and direction the plane of "`qv::cross(vec,
  plane.normal)`" through the edge's first point, when that cross product's length is at least
  `ANGLEEPSILON` (`NORMAL_EPSILON`), no side's plane is `epsilonEqual` to it and every side's
  polygon has no point more than `-epsilon` (0.0001) in front of it. Bevels are never drawn and
  never clip the bounds.
- **QBSP-15.21** An origin brush (contents origin, QBSP-15.13) is removed from its entity
  (`ProcessMapBrushes`); in an entity that is not the world and has no `origin` yet, its bounds'
  centre becomes the origin and the `origin` key. An entity whose classname starts `rotate_`, in
  any case, with no origin brush takes the origin of the entity whose `targetname` is its
  `target`, in any case, read into a `qvec3f`, or 0 0 0 with none (`FixRotateOrigin`). An entity
  with either has each face's texture offsets increased by the origin along their axes and each
  plane's distance decreased by "`qv::dot(plane.normal, entity.origin)`", and its bounds found
  again. A brush's chop order is the entity's `_chop_order`, else 1 when `_chop` is 0, else 0;
  the brushes are chopped in the order of chop order then line (`sort_key`).
- **QBSP-15.22** A map brush becomes a brush for a hull (`LoadBrush`, `QBSP3`) with each of its
  sides but, in the drawing hull, the bevels ("don't add bevels for the point hull"). For a
  clipping hull each side not of a `bevel` texture (`no_expand`) has its plane's distance
  increased by "`qv::dot(corner, plane.normal)`", the corner taking on each axis the hull box's
  maximum where the normal is positive, its minimum where negative and 0 where 0, and is no
  longer a bevel. Its polygons (`CreateBrushWindings`) are each side's plane's square clipped to
  the front of the opposite of each other side's plane but the bevels' within `-epsilon`; a side
  with a polygon left marks its map face visible, one without unmarks it, and a polygon with a
  coordinate beyond the world's extent is none. The brush is dropped when its bounds
  (`update_bounds`, around the sides' polygons) reach the extent on an axis, "brush bounds out
  of range", or lie wholly beyond it, "no visible sides on brush". A brush of a Hipnotic
  rotating entity with an origin not 0 0 0 gets the bounds `-delta` to `delta` on every axis,
  `delta` the largest absolute coordinate of its bounds.
- **QBSP-15.23** Each side's polygon is then checked (`CheckFace`): fewer than three points is
  none ("completely clipped away"); a point closer than `-epsilon` to the next is removed
  ("Healing degenerate edge") and the check begins again; a point more than `-epsilon` outside
  the plane through an edge along the side's normal makes the polygon none ("Found a non-convex
  face"). A side can be seen (`side_t::is_visible`) when its map face is visible, and a nodraw
  side only when it is also a hint.
- **QBSP-15.24** An entity's brushes for a hull (`Brush_LoadEntity`) are its own and, for the
  world, those of every later entity whose classname is `func_detail`, `func_group`,
  `func_detail_illusionary`, `func_detail_wall`, `func_detail_fence`,
  `func_illusionary_visblocker` (`IsWorldBrushEntity`) or `func_areaportal`
  (`IsNonRemoveWorldBrushEntity`), in any case, in entity order. For each map brush: in the
  drawing hull an entity with `_super_detail` nonzero gives none, and solid contents are made
  detail illusionary, fence, wall or solid by a `func_detail_illusionary`, `func_detail_fence`,
  `func_detail_wall` or `func_detail` classname; a `func_detail_illusionary` brush is in no
  clipping hull; a clip brush only widens the drawing hull's bounds and is solid in the others;
  a hint brush is empty in the drawing hull and in no other; a brush of an entity other than the
  world is "`create_detail_fence_contents(contentflags_t::make(EWT_VISCONTENTS_SOLID))`", Quake
  not allowing contents in brush models; in a clipping hull a brush neither solid, sky nor a
  fence is left out and every other is solid; the map brush's mirror-inside flags, when set,
  and its suppressed clipping of its own type are put back. The hull's bounds are around every
  brush it loads.
