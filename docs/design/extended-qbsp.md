# The extended dialect from ericw-tools' qbsp

How `qs-map-bsp`'s extended dialect (QBSP-14) is ported from ericw-tools' qbsp at
`ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266`, under
[ADR-0023](../adr/0023-ericw-tools-porting-upstream.md) and ADR-0017's clean-port policy. The id
dialect stays id's qbsp (QBSP-1 to QBSP-13) and shares none of this code but the map reader and
the BSP writer's lumps.

## Why a second pipeline

ericw-tools does not run id's qbsp in double precision. For each entity and hull it loads the
brushes with their contents (`Brush_LoadEntity`), sorts and chops them (`ChopBrushes`), builds a
tree from the brushes themselves (`BrushBSP`, Quake II's way), portals the tree
(`MakeTreePortals`), fills the outside from the void (`FillOutside`), builds a second, precise
tree with the sides only the void touches left out of the splits, portals and fills it again,
then makes faces from the visible brush sides (`MarkVisibleSides`, `MakeFaces`), prunes
(`PruneNodes`), fixes T-junctions (`TJunc`) and exports nodes, clip nodes, leaves and faces.
Every vector is `qvec3d`, `qvec<double, 3>`. id's qbsp instead builds faces by CSG and splits on
them, in `float`. The two give the same solid space in principle and different trees and
precision in practice, which the map compiler differential (P6-07) measures.

## Modules

Each is a module of `crates/qs-map-bsp/src/extended/`, named for the concept, each with its own
tests; the `Ported from` header names the ericw-tools file and functions.

| Module | Concept | ericw-tools |
| --- | --- | --- |
| `vec` | Double-precision vectors, planes, bounds and the plane table with its hash and opposite planes | `include/common/qvec.hh`; `qbsp/map.cc` `mapdata_t::add_or_find_plane` |
| `game` | The game seam: contents and their kinds, the clipping hulls' sizes; Quake's definition | `include/common/bspfile.hh` `gamedef_t`, `common/bspfile.cc` `gamedef_q1_like_t` |
| `winding` | Convex polygons: from a plane, clipped, split, area, centre, bounds | `include/common/polylib.hh` (`winding_base_t`) |
| `map` | Map brushes from `qs-map`'s `MapFile`: faces, planes, texinfo, contents from texture names (skip, hint, clip, liquids, sky), bevels, bounds, origin brushes, the world's extent | `qbsp/map.cc` `ParseBrush`, `ParseBrushFace`, `Brush_GetContents`, `AddBrushBevels`, `FindTexinfo`, `ProcessMapBrushes` |
| `brush` | BSP brushes for a hull: windings, the clipping hulls' expansion, per-entity contents, the world brush entities | `qbsp/brush.cc` `LoadBrush`, `CreateBrushWindings`, `Brush_LoadEntity` |
| `chop` | Removing what brushes overlap | `qbsp/brushbsp.cc` `ChopBrushes`, `SubtractBrush` |
| `tree` | The brush BSP: split planes, splitting brushes, leaves; pruning | `qbsp/brushbsp.cc` `BrushBSP`, `BuildTree_r`, `SelectSplitPlane`, `SplitBrush`; `qbsp/tree.cc` `PruneNodes` |
| `portals` | Tree portals and the sides each shows | `qbsp/portals.cc` `MakeTreePortals`, `MarkVisibleSides` |
| `outside` | The fill from the void, leaks and their line, the detail fill | `qbsp/outside.cc` `FillOutside`, `FillDetail` |
| `faces` | Faces from visible sides, merged and subdivided, listed in leaves | `qbsp/faces.cc`, `qbsp/merge.cc`, `qbsp/csg.cc` `SplitFace` |
| `tjunc` | T-junction fixing by minimum-weight triangulation | `qbsp/tjunc.cc` |
| `output` | Planes, texinfo, vertices, edges, faces, nodes, leaves and clip nodes into `qs-bsp`'s `BspFile` | `qbsp/writebsp.cc`, `qbsp/faces.cc` `EmitFaces` |
| `prt` | The portal file | `qbsp/prtfile.cc`, `common/prtfile.cc` |
| `process` | The compile: each hull for each model, the entity and texture lumps | `qbsp/qbsp.cc` `ProcessFile`, `ProcessEntity`; `qbsp/map.cc` `WriteEntitiesToString` |

## A game seam for Quake II

Quake II support is a later goal (Justin, 2026-11-13). ericw-tools' qbsp compiles both games
through a game definition (`target_game`, `gamedef_t`): contents (`contentflags_t`), which
contents are solid, detail or liquid, the clipping hulls' sizes, and the BSP format. The port
keeps that seam: a `Game` with what the pipeline asks of a game, Quake its only implementation
for now. Quake II's own parts, areas and area portals (`EmitAreaPortals`), its brush and
surface flags and its BSP format, are not ported yet but have their place: a second `Game`
and the modules that only it calls. `-notriggermodels`, regions and the debug writers are not
ported.

## Order and proof

The modules were ported in the order above, each with its spec statements (QBSP-15, a section
of `docs/spec/qbsp.md`) and the tests that cite them. `Compiler` compiles the extended dialect
through `process` alone (QBSP-14.10); the id dialect's pipeline keeps none of it. `cargo xtask
map-diff` compares every corpus map with ericw-tools' qbsp and, where id's qbsp reads it, the id
dialect with id's: leaks, entities, faces within tolerance and sampled traces in every hull.
Not ported: the `-tjunc` levels below `mwt` and the other options' non-default values, regions,
`-notriggermodels`, super detail, Quake II's areas and the debug writers.
