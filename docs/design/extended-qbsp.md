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
| `winding` | Convex polygons: from a plane, clipped, split, area, centre, bounds | `include/common/polylib.hh` (`winding_base_t`) |
| `map` | Map brushes from `qs-map`'s `MapFile`: faces, planes, texinfo, contents from texture names (skip, hint, clip, liquids, sky), bevels, bounds; world brush entities (QBSP-14.8) | `qbsp/map.cc` `ParseBrush`, `ParseBrushFace`, `Brush_GetContents`, `AddBrushBevels`, `FindTexinfo` |
| `brush` | BSP brushes for a hull: windings, the clipping hulls' expansion, per-entity contents | `qbsp/brush.cc` `LoadBrush`, `CreateBrushWindings`, `ExpandBrush`, `Brush_LoadEntity` |
| `chop` | Removing what brushes overlap | `qbsp/brushbsp.cc` `ChopBrushes`, `SubtractBrush` |
| `tree` | The brush BSP: split planes, splitting brushes, leaves | `qbsp/brushbsp.cc` `BrushBSP`, `BuildTree_r`, `SelectSplitPlane`, `SplitBrush` |
| `portals` | Tree portals | `qbsp/portals.cc` `MakeTreePortals`, `MarkVisibleSides` |
| `outside` | The fill from the void, leaks and their trail | `qbsp/outside.cc` `FillOutside` |
| `faces` | Faces from visible sides, merged and subdivided; pruning | `qbsp/faces.cc`, `qbsp/merge.cc`, `qbsp/tree.cc` |
| `tjunc` | T-junction fixing | `qbsp/tjunc.cc` |
| `output` | Nodes, clip nodes, leaves, faces and the portal file into BSP29 or BSP2 lumps, through the writer the id dialect uses | `qbsp/writebsp.cc`, `qbsp/prtfile.cc` |

Quake II's areas and area portals, `-notriggermodels`, regions and the debug writers are not
ported: none is used by a Quake map's compile.

## Order and proof

The modules are ported in the order above, each with its spec statements (QBSP-15, a section
of `docs/spec/qbsp.md`) and the tests that cite them. Until the last lands, the extended
dialect keeps today's pipeline; the switch is one change in `Compiler::compile_read`, made when
`cargo xtask map-diff` agrees with ericw-tools on the room and the instance map. Then each map
of the corpus that still differs is a defect worked to agreement, the list in `map_diff.rs`
shrinking to none.
