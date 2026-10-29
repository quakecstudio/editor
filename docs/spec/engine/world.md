# World collision

Prefix: `WORLD` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how the server finds what a moving box hits, as implemented by `WinQuake/world.c`
in [id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, with the hulls `WinQuake/model.c` builds, the link
lists of `WinQuake/common.c` and `BoxOnPlaneSide` of `WinQuake/mathlib.c`. Implemented by
`qs-sim`.

The `#ifdef QUAKE2` blocks of `world.c` (rotated brush models) and `#ifdef PARANOID` are not
compiled into Quake and are not described. An `id386` build replaces `SV_HullPointContents`
with the assembly of `worlda.s`; this spec follows the C.

Every value is a C `float` unless it says otherwise, and each operation rounds to the type it
is stored in: `frac = (t1 + DIST_EPSILON)/(t1-t2)` is computed in `double`, since
`DIST_EPSILON` is a `double` constant, and rounded to `float` when stored. An x87 build can
keep more precision between operations; the engine differential (P3-25) is where that shows.

## WORLD-1 Hulls

**Source:** `WinQuake/model.c`, `Mod_LoadClipnodes`, `Mod_MakeHull0` and `Mod_LoadBrushModel`;
`WinQuake/world.c`, `SV_InitBoxHull`, `SV_HullForBox` and `SV_HullForEntity`.

A hull is a tree of clipnodes over planes, with the indices `firstclipnode` and
`lastclipnode` a walk may visit and the box `clip_mins`, `clip_maxs` it was built for.

### Behaviour

- **WORLD-1.1** Hull 0 of a brush model is its drawing nodes read as clipnodes
  (`Mod_MakeHull0`): clipnode `i` has node `i`'s plane, and each child is the child node's
  index, or, for a leaf, the leaf's `contents`. Its `firstclipnode` is the model's
  `headnode[0]` and its `lastclipnode` the BSP's node count less one. Its box is all zeros.
- **WORLD-1.2** Hulls 1 and 2 are the BSP's clipnodes (`Mod_LoadClipnodes`), with
  `firstclipnode` the model's `headnode[1]` and `headnode[2]` and `lastclipnode` the clipnode
  count less one. Hull 1's box is (-16, -16, -24) to (16, 16, 32), the player's; hull 2's is
  (-32, -32, -24) to (32, 32, 64). Every submodel of a BSP shares its nodes, clipnodes and
  planes and differs only in its head nodes.
- **WORLD-1.3** A box is turned into a hull of six clipnodes (`SV_InitBoxHull`,
  `SV_HullForBox`): clipnode `i` uses plane `i`, whose type is `i>>1` and whose normal is 1 on
  that axis; its child `i&1` is `CONTENTS_EMPTY` and its other child is `i + 1`, or
  `CONTENTS_SOLID` for clipnode 5. The planes' distances are `maxs[0]`, `mins[0]`, `maxs[1]`,
  `mins[1]`, `maxs[2]`, `mins[2]`; `firstclipnode` is 0 and `lastclipnode` 5.
- **WORLD-1.4** `SV_HullForEntity (ent, mins, maxs)` gives the hull a box of `mins` to `maxs`
  is clipped against, and the offset to subtract from the box's points. For an entity whose
  `solid` is `SOLID_BSP` it fails with `Sys_Error ("SOLID_BSP without MOVETYPE_PUSH")` unless
  its `movetype` is `MOVETYPE_PUSH`, and with `Sys_Error ("MOVETYPE_PUSH with a non bsp
  model")` unless its model is a brush model. The hull is chosen by `size = maxs[0] -
  mins[0]`: hull 0 when `size < 3`, hull 1 when `size <= 32`, else hull 2; the offset is
  `clip_mins - mins + origin`. For any other entity it is the box hull of `ent.mins - maxs` to
  `ent.maxs - mins` and the offset is the entity's `origin`.

## WORLD-2 Point contents

**Source:** `WinQuake/world.c`, `SV_HullPointContents`, `SV_PointContents` and
`SV_TruePointContents`.

### Behaviour

- **WORLD-2.1** `SV_HullPointContents (hull, num, p)` walks from clipnode `num` while it is 0
  or more. A clipnode outside `firstclipnode` to `lastclipnode` fails with `Sys_Error
  ("SV_HullPointContents: bad node number")`. The distance is `p[type] - dist` for a plane of
  type 0 to 2, else `DotProduct (normal, p) - dist`; below 0 the walk takes child 1, else
  child 0. The contents value reached is the result. A negative stored plane type would read
  outside `p` in id's C; it is taken as a general plane here, our choice.
- **WORLD-2.2** `SV_PointContents (p)` is the contents of the world's hull 0 from clipnode
  0, with the currents `CONTENTS_CURRENT_0` (-9) to `CONTENTS_CURRENT_DOWN` (-14) read as
  `CONTENTS_WATER`. `SV_TruePointContents` keeps them.

## WORLD-3 A line through a hull

**Source:** `WinQuake/world.c`, `SV_RecursiveHullCheck`.

`SV_RecursiveHullCheck (hull, num, p1f, p2f, p1, p2, trace)` follows the line from `p1` to
`p2`, which runs from `p1f` to `p2f` of the whole move, and returns false once it has found
the impact.

### Behaviour

- **WORLD-3.1** At a contents value (`num < 0`) it returns true. For any contents but
  `CONTENTS_SOLID` it clears `allsolid` and sets `inopen` for `CONTENTS_EMPTY`, else
  `inwater`; for `CONTENTS_SOLID` it sets `startsolid`.
- **WORLD-3.2** A clipnode outside `firstclipnode` to `lastclipnode` fails with `Sys_Error
  ("SV_RecursiveHullCheck: bad node number")`.
- **WORLD-3.3** `t1` and `t2` are the distances of `p1` and `p2` from the clipnode's plane,
  as WORLD-2.1 computes them. When both are 0 or more the check goes on in child 0 only, and
  when both are below 0 in child 1 only, with the same arguments.
- **WORLD-3.4** Otherwise the crossing is put `DIST_EPSILON`, 1/32, on the near side: `frac =
  (t1 + DIST_EPSILON)/(t1-t2)` when `t1 < 0`, else `(t1 - DIST_EPSILON)/(t1-t2)`, then kept
  within 0 to 1. `midf = p1f + (p2f - p1f)*frac`, `mid[i] = p1[i] + frac*(p2[i] - p1[i])`, and
  `side` is 1 when `t1 < 0`, else 0.
- **WORLD-3.5** The check goes on in child `side` from `p1` to `mid`, returning false when
  that does. Then, when the contents of child `side^1` at `mid` (WORLD-2.1) is not
  `CONTENTS_SOLID`, it returns the check of child `side^1` from `mid` to `p2`, over `midf` to
  `p2f`.
- **WORLD-3.6** Else, when `allsolid` is set it returns false. Otherwise this is the impact:
  the trace's plane is the clipnode's for `side` 0 and its negation (normal and distance) for
  `side` 1. While the hull's contents at `mid` from `firstclipnode` is `CONTENTS_SOLID`,
  `frac -= 0.1` and, when `frac` is then below 0, the trace's `fraction` becomes `midf` and its
  `endpos` `mid`, and it returns false ("backup past 0"); else `midf` and `mid` are computed
  again from the new `frac`. When `mid` is out of the solid, `fraction` is `midf`, `endpos` is
  `mid`, and it returns false.

## WORLD-4 Clipping a move to one entity

**Source:** `WinQuake/world.c`, `SV_ClipMoveToEntity`.

### Behaviour

- **WORLD-4.1** `SV_ClipMoveToEntity (ent, start, mins, maxs, end)` starts from a trace that
  is all zero but for `fraction` 1, `allsolid` set and `endpos` `end`. It takes the hull and
  offset of WORLD-1.4, runs WORLD-3 from the hull's `firstclipnode` with `p1f` 0 and `p2f` 1,
  from `start - offset` to `end - offset`, then adds the offset to `endpos` when `fraction` is
  not 1. The trace's entity is `ent` when `fraction < 1` or `startsolid` is set, else none.

## WORLD-5 Area nodes

**Source:** `WinQuake/world.c`, `SV_CreateAreaNode` and `SV_ClearWorld`; `WinQuake/common.c`,
`ClearLink`, `RemoveLink` and `InsertLinkBefore`.

The world's box is divided into area nodes, each holding the entities whose box crosses its
dividing plane, so a move tests only the entities near it.

### Behaviour

- **WORLD-5.1** `SV_ClearWorld` builds the nodes from the world model's `mins` and `maxs`,
  numbering them as they are made: a node, then its child 0's subtree, then its child 1's. A
  node at depth `AREA_DEPTH` (4) is a leaf with no axis. Any other node's axis is 0 when its
  box's size on x is greater than on y, else 1; its `dist` is `0.5 * (maxs[axis] +
  mins[axis])`; child 0 covers the part of its box from `dist` to `maxs[axis]` and child 1
  the part from `mins[axis]` to `dist`. That makes 31 nodes of the `AREA_NODES` (32) id
  reserves.
- **WORLD-5.2** Each node keeps two lists, its triggers and its solids. Linking an entity
  puts it at the end of one (`InsertLinkBefore` the list's head), and unlinking takes it out,
  leaving the others in order. An entity is in at most one list.

## WORLD-6 Linking an entity

**Source:** `WinQuake/world.c`, `SV_UnlinkEdict`, `SV_LinkEdict`, `SV_FindTouchedLeafs` and
`SV_TouchLinks`; `WinQuake/mathlib.h`, `BOX_ON_PLANE_SIDE`; `WinQuake/mathlib.c`,
`BoxOnPlaneSide`.

### Behaviour

- **WORLD-6.1** `SV_LinkEdict (ent, touch_triggers)` first unlinks the entity when it is
  linked. The world, edict 0, and a free edict are not linked.
- **WORLD-6.2** The entity's `absmin` and `absmax` are `origin + mins` and `origin + maxs`,
  then widened: by 15 on x and y for an entity whose `flags` has `FL_ITEM` (256), "to make
  items easier to pick up", else by 1 on every axis.
- **WORLD-6.3** Its leaves are found again when its `modelindex` is not 0
  (`SV_FindTouchedLeafs` from the world's node 0): a leaf whose `contents` is `CONTENTS_SOLID`
  adds nothing; any other leaf adds its number less one, while the entity has fewer than `MAX_ENT_LEAFS` (16); a node
  goes on into child 0 when `BOX_ON_PLANE_SIDE` of `absmin`, `absmax` and its plane has bit 1
  and into child 1 when it has bit 2.
- **WORLD-6.4** `BOX_ON_PLANE_SIDE (emins, emaxs, p)` for a plane of type 0 to 2 is 1 when
  `p->dist <= emins[type]`, else 2 when `p->dist >= emaxs[type]`, else 3. For any other plane
  it is `BoxOnPlaneSide`: `dist1` is the dot product of the normal with the corner whose
  coordinate on each axis is `emaxs` where the normal is 0 or more and `emins` where it is
  negative (the plane's `signbits`), and `dist2` with the opposite corner; bit 1 is set when
  `dist1 >= p->dist` and bit 2 when `dist2 < p->dist`.
- **WORLD-6.5** An entity whose `solid` is `SOLID_NOT` is not put in a list. Any other is
  put in the first node, walking from node 0, that is a leaf or that its box crosses: the walk
  takes child 0 when `absmin[axis] > dist` and child 1 when `absmax[axis] < dist`. It goes in
  the node's triggers when its `solid` is `SOLID_TRIGGER`, else its solids.
- **WORLD-6.6** When `touch_triggers` is set, `SV_TouchLinks` runs from node 0. For each
  entity in the node's triggers, in order, it skips the entity itself, one whose `touch` is 0
  or whose `solid` is not `SOLID_TRIGGER`, and one whose box does not overlap the entity's
  (any `absmin` above the other's `absmax`). For the rest it saves `self` and `other`, sets
  `self` to the trigger, `other` to the entity and `time` to the server's time, runs the
  trigger's `touch`, and restores `self` and `other`. Then, at a node that is not a leaf, it
  goes on into child 0 when `absmax[axis] > dist` and into child 1 when `absmin[axis] < dist`.
- **WORLD-6.7** `SV_TouchLinks` takes the next entity of a list before it runs a `touch`, so
  entities a touch adds to the list after it are visited, and the next one is visited even
  when the touch moves it to the end of the same list, after which the walk goes on from
  there and the entities it moved past are not visited. When a touch takes that next entity
  out of the list, id's loop follows its cleared links, which is undefined; here the rest of
  that list is skipped, our choice.
- **WORLD-6.8** `SV_UnlinkEdict` takes the entity out of its list; an entity not in one is
  left as it is.

## WORLD-7 Moving a box through the world

**Source:** `WinQuake/world.c`, `SV_Move`, `SV_MoveBounds`, `SV_ClipToLinks` and
`SV_TestEntityPosition`.

`SV_Move (start, mins, maxs, end, type, passedict)` is the trace of a box of `mins` to `maxs`
from `start` to `end`. `type` is `MOVE_NORMAL` (0), `MOVE_NOMONSTERS` (1) or `MOVE_MISSILE`
(2); `passedict` is an entity the move ignores, or none.

### Behaviour

- **WORLD-7.1** The trace starts as the move clipped to the world, edict 0 (WORLD-4). The box
  used against monsters is ±15 on every axis for `MOVE_MISSILE`, else `mins` to `maxs`. The
  bounds of the whole move are, on each axis, `start + mins2 - 1` to `end + maxs2 + 1` when
  `end > start`, else `end + mins2 - 1` to `start + maxs2 + 1`, with the monster box.
- **WORLD-7.2** `SV_ClipToLinks` from node 0 goes through each node's solids in order. It
  skips an entity whose `solid` is `SOLID_NOT` and the `passedict`; fails with `Sys_Error
  ("Trigger in clipping list")` at one whose `solid` is `SOLID_TRIGGER`; skips any but
  `SOLID_BSP` entities for `MOVE_NOMONSTERS`; skips one whose box does not overlap the move's
  bounds; and skips one whose `size[0]` is 0 when the `passedict`'s is not ("points never
  interact").
- **WORLD-7.3** Before clipping to an entity, when the trace so far has `allsolid` set,
  `SV_ClipToLinks` returns at once. When there is a `passedict`, an entity whose `owner` is
  the `passedict`, and the `passedict`'s `owner`, are skipped. The entity is clipped with the
  monster box when its `flags` has `FL_MONSTER` (32), else with `mins` to `maxs`.
- **WORLD-7.4** The entity's trace replaces the trace so far when it has `allsolid` or
  `startsolid` set or a smaller `fraction`; it then names the entity, and keeps `startsolid`
  set when the trace so far had it. id's `else if (trace.startsolid)` after that test can
  never be reached.
- **WORLD-7.5** At a node that is not a leaf, `SV_ClipToLinks` goes on into child 0 when the
  move's `boxmaxs[axis] > dist` and into child 1 when `boxmins[axis] < dist`.
- **WORLD-7.6** `SV_TestEntityPosition (ent)` is the world when `SV_Move (origin, mins, maxs,
  origin, MOVE_NORMAL, ent)` starts in solid, else none.

## Constants

| Name | Value | Source |
| --- | --- | --- |
| `WORLD_DIST_EPSILON` | 0.03125 | `WinQuake/world.c`, `#define DIST_EPSILON (0.03125)` |
| `WORLD_AREA_DEPTH` | 4 | `WinQuake/world.c`, `#define AREA_DEPTH 4` |
| `WORLD_AREA_NODES` | 32 | `WinQuake/world.c`, `#define AREA_NODES 32` |
| `WORLD_MAX_ENT_LEAFS` | 16 | `WinQuake/progs.h`, `#define MAX_ENT_LEAFS 16` |
| `WORLD_ITEM_REACH` | 15 | `WinQuake/world.c`, `absmin[0] -= 15` in `SV_LinkEdict` |
| `WORLD_MISSILE_BOX` | 15 | `WinQuake/world.c`, `clip.mins2[i] = -15` in `SV_Move` |
| `WORLD_HULL1_MINS`, `WORLD_HULL1_MAXS` | (-16, -16, -24), (16, 16, 32) | `WinQuake/model.c`, `Mod_LoadClipnodes` |
| `WORLD_HULL2_MINS`, `WORLD_HULL2_MAXS` | (-32, -32, -24), (32, 32, 64) | `WinQuake/model.c`, `Mod_LoadClipnodes` |
| `WORLD_CONTENTS_EMPTY`, `_SOLID`, `_WATER` | -1, -2, -3 | `WinQuake/bspfile.h` |
| `WORLD_CONTENTS_CURRENT_0`, `_CURRENT_DOWN` | -9, -14 | `WinQuake/bspfile.h` |
| `WORLD_FL_MONSTER`, `WORLD_FL_ITEM` | 32, 256 | `WinQuake/server.h` |
| `WORLD_MOVE_NORMAL`, `_NOMONSTERS`, `_MISSILE` | 0, 1, 2 | `WinQuake/world.h` |
