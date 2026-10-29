# Server physics

Prefix: `PHYS` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the server frame, the thinking it runs and how each kind of entity moves, as
implemented by `WinQuake/sv_phys.c` in [id-Software/Quake](https://github.com/id-Software/Quake)
at commit `bf4ac424ce754894ac8f1dae6a3981954bc9852d`, on the world queries of
[`world.md`](world.md). Implemented by `qs-sim`.

The `#ifdef QUAKE2` blocks are not compiled into Quake and are not described. `frametime` is
`host_frametime`, the frame's length in seconds; `sv.time` is a `double`, and every other value
is a `float` unless a statement says otherwise. A movetype is compared as the float it is
stored as. The sounds the frame starts are recorded for the client; `SV_StartSound`'s
message is not described here.

## PHYS-1 The frame

**Source:** `WinQuake/sv_phys.c`, `SV_Physics`.

### Behaviour

- **PHYS-1.1** A frame first sets `self` and `other` to the world and `time` to `sv.time`, and
  runs `StartFrame`.
- **PHYS-1.2** Then every edict from 0 to `num_edicts - 1` that is not free, in order: when
  `force_retouch` is not 0 it is linked touching triggers (WORLD-6); then edicts 1 to
  `maxclients` run client physics (PHYS-8), and any other by its `movetype`: `MOVETYPE_PUSH`
  (7) PHYS-3, `MOVETYPE_NONE` (0) PHYS-2.2, `MOVETYPE_NOCLIP` (8) PHYS-2.3,
  `MOVETYPE_STEP` (4) PHYS-5, and `MOVETYPE_TOSS` (6), `MOVETYPE_BOUNCE` (10),
  `MOVETYPE_FLY` (5) and `MOVETYPE_FLYMISSILE` (9) PHYS-4. Any other movetype is
  `Sys_Error ("SV_Physics: bad movetype %i")` with the movetype truncated to an int.
- **PHYS-1.3** After the last edict, `force_retouch` is lowered by 1 when it is not 0, and
  `sv.time` advances by `frametime`. `num_edicts` is read at each step, so an edict spawned
  during the frame is moved in the same frame.

## PHYS-2 Thinking

**Source:** `WinQuake/sv_phys.c`, `SV_RunThink`, `SV_Physics_None`, `SV_Physics_Noclip`.

### Behaviour

- **PHYS-2.1** `SV_RunThink (ent)` does nothing when `nextthink` is 0 or less or later than
  `sv.time + frametime`. Otherwise `nextthink` becomes 0, `time` is `nextthink` raised to
  `sv.time` if earlier ("don't let things stay in the past"), `self` is the entity and `other`
  the world, and `think` runs. It reports whether the entity is still in use.
- **PHYS-2.2** `MOVETYPE_NONE` only thinks.
- **PHYS-2.3** `MOVETYPE_NOCLIP` thinks, and when still in use adds `frametime * avelocity` to
  `angles` and `frametime * velocity` to `origin` and links without touching triggers.

## PHYS-3 Pushers

**Source:** `WinQuake/sv_phys.c`, `SV_Physics_Pusher`, `SV_PushMove`, `SV_PushEntity`.

Doors, platforms and trains are `SOLID_BSP` with `MOVETYPE_PUSH`. Their clock is their own,
`ltime`, which advances only while they move.

### Behaviour

- **PHYS-3.1** `SV_Physics_Pusher` takes `movetime = nextthink - ltime`, at least 0, when
  `nextthink < ltime + frametime`, else `frametime`. When `movetime` is not 0 it pushes
  (PHYS-3.2). Then, when `nextthink` is later than `ltime` before the push and no later than
  `ltime` after it, `nextthink` becomes 0, `time` is `sv.time`, `self` the pusher, `other` the
  world, and `think` runs.
- **PHYS-3.2** `SV_PushMove (pusher, movetime)` with no `velocity` only adds `movetime` to
  `ltime`. Otherwise `move = velocity * movetime`; the pusher's `origin` moves by it, `ltime`
  advances by `movetime`, and the pusher is linked without touching triggers.
- **PHYS-3.3** Then each edict from 1 in order that is in use and not `MOVETYPE_PUSH`,
  `MOVETYPE_NONE` or `MOVETYPE_NOCLIP` is checked. One standing on the pusher (`FL_ONGROUND`
  with the pusher as `groundentity`) is moved. Any other is skipped when its `absmin` is at or
  past the moved pusher's `absmax`, or its `absmax` at or before the moved `absmin`, on any
  axis, or when it is not in solid where it stands (WORLD-7.6).
- **PHYS-3.4** A moved entity loses `FL_ONGROUND` unless it is `MOVETYPE_WALK` (3), its old
  `origin` is kept, and, with the pusher `SOLID_NOT` meanwhile and `SOLID_BSP` after,
  `SV_PushEntity (check, move)` moves it.
- **PHYS-3.5** When it is still in solid after that: one whose `mins[0]` equals its `maxs[0]`
  is left where it was pushed; a `SOLID_NOT` or `SOLID_TRIGGER` one ("corpse") gets `mins[0]`,
  `mins[1]`, `maxs` all 0 on x and y and `maxs` equal to `mins`, and is left. Any other blocks:
  it goes back to its old `origin` and is linked touching triggers; the pusher goes back, is
  linked without touching, and loses `movetime` from `ltime`; the pusher's `blocked` runs with
  `self` the pusher and `other` the entity, when it has one; every entity moved before it goes
  back to its old `origin`, linked without touching; and the push ends.
- **PHYS-3.6** `SV_PushEntity (ent, push)` moves the entity's box by `push`: with
  `MOVE_MISSILE` for `MOVETYPE_FLYMISSILE`, `MOVE_NOMONSTERS` for a `SOLID_TRIGGER` or
  `SOLID_NOT` entity, else `MOVE_NORMAL`, ignoring itself. Its `origin` becomes the trace's end,
  it is linked touching triggers, and when the trace names an entity the two touch (PHYS-6.1).
  `velocity` is not changed.

## PHYS-4 Toss, bounce and fly

**Source:** `WinQuake/sv_phys.c`, `SV_Physics_Toss`, `SV_CheckWaterTransition`.

### Behaviour

- **PHYS-4.1** The entity thinks (PHYS-2.1) and stops when no longer in use. One with
  `FL_ONGROUND` stops there.
- **PHYS-4.2** Otherwise its velocity is bounded (PHYS-6.3) and, unless it is `MOVETYPE_FLY` or
  `MOVETYPE_FLYMISSILE`, gravity is added (PHYS-6.4). `angles` gain `frametime * avelocity`,
  and it is pushed by `velocity * frametime` (PHYS-3.6).
- **PHYS-4.3** When the push went the whole way, or freed the entity, that is all. Otherwise
  `velocity` is clipped against the plane hit (PHYS-6.2) with an overbounce of 1.5 for
  `MOVETYPE_BOUNCE`, else 1. When the plane's normal z is above 0.7 and the velocity's z is
  below 60 or it is not `MOVETYPE_BOUNCE`, it lands: `FL_ONGROUND` is set, `groundentity` is
  the entity hit, and `velocity` and `avelocity` become 0.
- **PHYS-4.4** Then, after a move that hit something, the water is checked: with `cont` the
  contents at `origin` (WORLD-2.2), a
  `watertype` of 0 ("just spawned here") becomes `cont` with `waterlevel` 1; else a `cont` of
  `CONTENTS_WATER` or below makes `watertype` `cont` and `waterlevel` 1, and any other makes
  `watertype` `CONTENTS_EMPTY` and `waterlevel` `cont`. Crossing into or out of liquid starts
  the sound `misc/h2ohit1.wav`.

## PHYS-5 Stepping monsters

**Source:** `WinQuake/sv_phys.c`, `SV_Physics_Step`.

### Behaviour

- **PHYS-5.1** A `MOVETYPE_STEP` entity with none of `FL_ONGROUND`, `FL_FLY` and `FL_SWIM`
  falls: gravity is added, the velocity bounded, it slides for `frametime` (PHYS-7) and is
  linked touching triggers; when it then has `FL_ONGROUND` and its velocity's z before the
  fall was below `sv_gravity * -0.1`, the sound `demon/dland2.wav` starts.
- **PHYS-5.2** Then, falling or not, it thinks (PHYS-2.1) and its water is checked (PHYS-4.4).

## PHYS-6 Contact and forces

**Source:** `WinQuake/sv_phys.c`, `SV_Impact`, `ClipVelocity`, `SV_CheckVelocity`,
`SV_AddGravity`.

### Behaviour

- **PHYS-6.1** `SV_Impact (e1, e2)` sets `time` to `sv.time`, runs `e1`'s `touch` with `self`
  `e1` and `other` `e2` when it has one and is not `SOLID_NOT`, then `e2`'s the other way round
  on the same terms, and restores `self` and `other`.
- **PHYS-6.2** `ClipVelocity (in, normal, out, overbounce)` is `out = in - normal * backoff`
  per axis, with `backoff = DotProduct (in, normal) * overbounce`, each component between -0.1
  and 0.1 (`STOP_EPSILON`), exclusive, made 0. It returns 1 when the normal's z is above 0
  ("floor") and 2 when it is 0 ("step"), or both bits.
- **PHYS-6.3** `SV_CheckVelocity` sets a NaN component of `velocity` or `origin` to 0 and
  bounds each component of `velocity` to `±sv_maxvelocity` (2000).
- **PHYS-6.4** `SV_AddGravity` lowers `velocity[2]` by `gravity * sv_gravity * frametime`,
  `gravity` the entity's progs field of that name when the progs has it and it is not 0, else 1;
  `sv_gravity` is 800.

## PHYS-7 Sliding

**Source:** `WinQuake/sv_phys.c`, `SV_FlyMove`.

`SV_FlyMove (ent, time, steptrace)` moves the entity's box along its velocity for `time`,
sliding along what it hits, and returns what blocked it: 1 a floor, 2 a wall or step, 4 a dead
stop.

### Behaviour

- **PHYS-7.1** It makes up to 4 moves. Each stops the loop when `velocity` is 0 on every axis;
  else moves the box from `origin` toward `origin + time_left * velocity` (`MOVE_NORMAL`,
  ignoring itself). All in solid sets `velocity` to 0 and returns 3. A move that covered any
  distance sets `origin` to the trace's end, remembers `velocity` as the original and forgets
  the planes. A move that went the whole way stops the loop.
- **PHYS-7.2** A move that hit names an entity (else `Sys_Error ("SV_FlyMove: !trace.ent")`).
  A plane whose normal z is above 0.7 sets bit 1 and, when the entity hit is `SOLID_BSP`,
  `FL_ONGROUND` with that entity as `groundentity`; a normal z of 0 sets bit 2 and stores the
  trace as `steptrace`. The two touch (PHYS-6.1), and when that freed the entity the loop stops.
  `time_left` loses `time_left * fraction`.
- **PHYS-7.3** A sixth plane sets `velocity` to 0 and returns 3. Else the plane is added, and
  the original velocity is clipped against each plane in turn (PHYS-6.2, overbounce 1) until
  one gives a velocity not into any other plane (dot product below 0): that becomes `velocity`.
  When none does, two planes give `velocity` as their crease, the cross product of their
  normals scaled by its dot product with `velocity`; any other number sets `velocity` to 0 and
  returns 7.
- **PHYS-7.4** When the new `velocity` is against the velocity the move started with (dot
  product 0 or less), it becomes 0 and the blocked bits so far are returned. After the moves,
  the blocked bits are returned.

## PHYS-8 Clients

**Source:** `WinQuake/sv_phys.c`, `SV_Physics_Client`.

### Behaviour

- **PHYS-8.1** A client slot whose client is not connected does nothing in the frame
  ("unconnected slot"). A connected client's physics (`PlayerPreThink`, its movetype,
  `PlayerPostThink`) is not ported yet; a frame with one fails.

## Constants

| Name | Value | Source |
| --- | --- | --- |
| `PHYS_GRAVITY` | 800 | `WinQuake/sv_phys.c`, `sv_gravity = {"sv_gravity","800"}` |
| `PHYS_MAXVELOCITY` | 2000 | `WinQuake/sv_phys.c`, `sv_maxvelocity = {"sv_maxvelocity","2000"}` |
| `PHYS_FRICTION` | 4 | `WinQuake/sv_phys.c`, `sv_friction = {"sv_friction","4"}` |
| `PHYS_STOPSPEED` | 100 | `WinQuake/sv_phys.c`, `sv_stopspeed = {"sv_stopspeed","100"}` |
| `PHYS_STOP_EPSILON` | 0.1 | `WinQuake/sv_phys.c`, `#define STOP_EPSILON 0.1` |
| `PHYS_MAX_CLIP_PLANES` | 5 | `WinQuake/sv_phys.c`, `#define MAX_CLIP_PLANES 5` |
| `PHYS_BUMPS` | 4 | `WinQuake/sv_phys.c`, `numbumps = 4` in `SV_FlyMove` |
| `PHYS_BOUNCE` | 1.5 | `WinQuake/sv_phys.c`, `backoff = 1.5` in `SV_Physics_Toss` |
