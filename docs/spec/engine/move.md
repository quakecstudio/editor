# Monster movement

Prefix: `MOVE` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how the server moves a monster a step, as implemented by `WinQuake/sv_move.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, through the world queries of
[`world.md`](world.md). `walkmove`, `movetogoal` and `checkbottom` (BUILTIN-14.5,
BUILTIN-15.1, BUILTIN-15.6) are built on it. Implemented by `qs-sim`.

The `#ifdef QUAKE2` blocks are not compiled into Quake and are not described. `rand()` is the
engine's random source; each statement says where it is drawn, since the order of draws is
part of the behaviour. Values are C `float`s unless a statement says otherwise.

## MOVE-1 Is the ground under the box

**Source:** `WinQuake/sv_move.c`, `SV_CheckBottom`.

### Behaviour

- **MOVE-1.1** `SV_CheckBottom (ent)` takes the box `origin + mins` to `origin + maxs`. When
  the world's contents (WORLD-2.2) 1 unit below each of the four bottom corners, `mins[2] - 1`,
  are all `CONTENTS_SOLID`, the ground is there.
- **MOVE-1.2** Otherwise it traces a point (`SV_Move` with no box, `MOVE_NOMONSTERS`, ignoring
  `ent`) from the bottom's centre, `(mins + maxs)*0.5` on x and y at `mins[2]`, down
  `2*STEPSIZE` (36). When it hits nothing the ground is not there. Else `mid` is the end's
  height.
- **MOVE-1.3** Then from each corner in turn, (mins, mins), (mins, maxs), (maxs, mins),
  (maxs, maxs) on x and y, the same trace down to the same height: when it hits nothing, or
  `mid` less its end's height is more than `STEPSIZE` (18), the ground is not there. When all
  four pass, it is.

## MOVE-2 One step

**Source:** `WinQuake/sv_move.c`, `SV_movestep`.

`SV_movestep (ent, move, relink)` tries to move `ent` by `move` and says whether it did.
`relink` links it touching triggers (WORLD-6) where a statement says it links.

### Behaviour

- **MOVE-2.1** An entity whose `(int)flags` has `FL_SWIM` (2) or `FL_FLY` (1) makes up to two
  tries. Each moves its box from `origin` to `origin + move` (`SV_Move`, `MOVE_NORMAL`,
  ignoring `ent`). On the first, when its `enemy` is not the world, the end's height is lowered
  by 8 when `dz = origin[2] - enemy.origin[2]` is above 40 and raised by 8 when it is below
  30. A try that hits nothing ends the step: it fails when the entity swims and the contents
  at the trace's end are `CONTENTS_EMPTY` ("swim monster left water"); else `origin` becomes
  the end, the entity is linked when `relink` is set, and the step succeeds. After a first try
  that hit something, a second is made only when the `enemy` is not the world. When no try
  succeeds the step fails, changing nothing.
- **MOVE-2.2** Any other entity's box is moved down from `STEPSIZE` above `origin + move` to
  `STEPSIZE` below it. When that is all in solid the step fails. When it starts in solid it is
  tried again from `origin + move` itself to the same end, and fails when that is all in solid
  or starts in solid.
- **MOVE-2.3** When that trace hits nothing: an entity whose `flags` has `FL_PARTIALGROUND`
  (1024) moves by `move`, is linked when `relink` is set, loses `FL_ONGROUND` (512) and the
  step succeeds ("go ahead and fall"); any other fails, changing nothing ("walked off an
  edge").
- **MOVE-2.4** Else `origin` becomes the trace's end. When the ground is not under it there
  (MOVE-1): an entity with `FL_PARTIALGROUND` is linked when `relink` is set and the step
  succeeds; any other gets its old `origin` back and the step fails.
- **MOVE-2.5** Else `FL_PARTIALGROUND` is cleared, `groundentity` becomes the entity the trace
  hit, the entity is linked when `relink` is set, and the step succeeds.

## MOVE-3 A step in a direction

**Source:** `WinQuake/sv_move.c`, `SV_StepDirection`; `WinQuake/pr_cmds.c`, `PF_changeyaw`.

### Behaviour

- **MOVE-3.1** `SV_StepDirection (ent, yaw, dist)` sets `ideal_yaw` to `yaw`, turns `self` as
  `ChangeYaw` does (BUILTIN-15.4), and tries the step `(cos(y)*dist, sin(y)*dist, 0)` with
  `y = yaw*M_PI*2 / 360` computed in `double` and stored in `float`, the cosine and sine taken
  in `double`, without `relink` (MOVE-2).
- **MOVE-3.2** When the step succeeds but `delta = angles[1] - ideal_yaw` is above 45 and below
  315, the entity "not turned far enough" gets its old `origin` back. Either way it is linked
  touching triggers, and the result is whether the step succeeded.

## MOVE-4 Choosing a new direction

**Source:** `WinQuake/sv_move.c`, `SV_NewChaseDir`, `SV_FixCheckBottom`;
`WinQuake/mathlib.c`, `anglemod`.

`SV_NewChaseDir (actor, enemy, dist)` steps `actor` toward `enemy` by trying directions in
turn (MOVE-3), stopping at the first that succeeds.

### Behaviour

- **MOVE-4.1** `olddir` is `anglemod ((int)(ideal_yaw/45)*45)` and `turnaround` is `anglemod
  (olddir - 180)` (BUILTIN-15.5). With `deltax` and `deltay` the enemy's `origin` less the
  actor's on x and y, `d[1]` is 0 when `deltax > 10`, 180 when `deltax < -10`, else none (-1);
  `d[2]` is 270 when `deltay < -10`, 90 when `deltay > 10`, else none.
- **MOVE-4.2** When both are directions, the diagonal between them is tried first unless it is
  `turnaround`: 45 or 315 when `d[1]` is 0 (for `d[2]` 90 or not), else 135 or 215; id's
  code has 215, not the diagonal 225.
- **MOVE-4.3** Then one `rand()` is drawn. When `(rand()&3) & 1` is set, or else when
  `abs(deltay) > abs(deltax)` (C's integer `abs`, of the values truncated to `int`), `d[1]` and
  `d[2]` swap. Then `d[1]` and then `d[2]` are tried, each when it is a direction and not
  `turnaround`.
- **MOVE-4.4** Then `olddir` is tried. Then a second `rand()` is drawn: when `rand()&1` the
  eight headings 0, 45, ..., 315 are tried in that order, else 315 down to 0, each when it is
  not `turnaround`. Then `turnaround` itself.
- **MOVE-4.5** When none succeeds, `ideal_yaw` becomes `olddir` ("can't move"), and when the
  ground is not under the actor (MOVE-1) its `flags` gain `FL_PARTIALGROUND`.

## MOVE-5 Moving toward a goal

**Source:** `WinQuake/sv_move.c`, `SV_CloseEnough`, `SV_MoveToGoal`.

### Behaviour

- **MOVE-5.1** `SV_MoveToGoal` acts on `self` and its `goalentity`, with `dist` the first
  parameter. When `self`'s `(int)flags` has none of `FL_ONGROUND`, `FL_FLY` and `FL_SWIM` it
  writes 0 to `OFS_RETURN` and stops. Nothing else writes `OFS_RETURN`.
- **MOVE-5.2** When `self`'s `enemy` is not the world and `SV_CloseEnough (self, goal, dist)`,
  it stops: that is, on each axis the goal's `absmin` is not above `self`'s `absmax + dist` and
  its `absmax` is not below `self`'s `absmin - dist`.
- **MOVE-5.3** Else it draws one `rand()`; when `(rand()&3) == 1`, or else when the step
  toward `ideal_yaw` (MOVE-3) fails, it chooses a new direction toward the goal (MOVE-4).

## Constants

| Name | Value | Source |
| --- | --- | --- |
| `MOVE_STEPSIZE` | 18 | `WinQuake/sv_move.c`, `#define STEPSIZE 18` |
| `MOVE_FLY_ADJUST` | 8 | `WinQuake/sv_move.c`, `neworg[2] -= 8` in `SV_movestep` |
| `MOVE_FLY_HIGH`, `MOVE_FLY_LOW` | 40, 30 | `WinQuake/sv_move.c`, `dz > 40`, `dz < 30` |
| `MOVE_DI_NODIR` | -1 | `WinQuake/sv_move.c`, `#define DI_NODIR -1` |
| `MOVE_FL_FLY`, `_SWIM`, `_ONGROUND`, `_PARTIALGROUND` | 1, 2, 512, 1024 | `WinQuake/server.h` |
