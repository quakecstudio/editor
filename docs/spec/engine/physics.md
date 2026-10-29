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

**Source:** `WinQuake/sv_phys.c`, `SV_Physics_Client`, `SV_WalkMove`, `SV_CheckWater`,
`SV_CheckStuck`, `SV_WallFriction`, `SV_TryUnstick`.

### Behaviour

- **PHYS-8.1** A client slot whose client is not connected does nothing in the frame
  ("unconnected slot"). A connected client runs `PlayerPreThink` with `self` the client and
  `time` the server's, has its velocity bounded (PHYS-6.3), moves by its movetype as an int,
  is linked touching triggers, and runs `PlayerPostThink` the same way.
- **PHYS-8.2** By movetype: `MOVETYPE_NONE` thinks; `MOVETYPE_WALK` thinks, adds gravity
  unless `SV_CheckWater` reports it waist deep or its `flags` has `FL_WATERJUMP` (2048), checks
  it is not stuck, and walks (PHYS-8.3); `MOVETYPE_TOSS` and `MOVETYPE_BOUNCE` are PHYS-4;
  `MOVETYPE_FLY` thinks and slides for the frame (PHYS-7); `MOVETYPE_NOCLIP` thinks and moves
  by `frametime * velocity`. A think that freed the client ends its frame before the link.
  Any other movetype is `Sys_Error ("SV_Physics_client: bad movetype %i")`.
- **PHYS-8.3** `SV_WalkMove` clears `FL_ONGROUND` and slides for the frame, keeping the trace
  of any vertical wall hit. When that was not blocked by a step (bit 2), or the player was not
  on the ground and is not in water, or is no longer `MOVETYPE_WALK`, or has `FL_WATERJUMP`,
  that is the move. Otherwise from the start it is pushed up 18 (`STEPSIZE`), slid again with
  the old horizontal velocity and no vertical, and, when that made less than 1/32 of progress
  on both x and y, unstuck (PHYS-8.6); a wall then hit slows it (PHYS-8.5); and it is pushed
  down by `-18 + oldvel[2] * frametime`. When the ground below is a floor (normal z above 0.7)
  and the player's own `solid` is `SOLID_BSP`, which it never is, `FL_ONGROUND` and
  `groundentity` are set; when it is not a floor, the move without the step is kept.
- **PHYS-8.4** `SV_CheckWater` sets `waterlevel` 0 and `watertype` `CONTENTS_EMPTY`, then from
  the contents 1 above the feet: liquid makes `watertype` those contents and `waterlevel` 1,
  liquid at the box's middle 2, and liquid at `view_ofs` 3. It reports `waterlevel` above 1.
- **PHYS-8.5** `SV_WallFriction`: with `d` the dot product of the wall's normal and the
  forward of `v_angle`, plus 0.5, when `d` is below 0 the velocity's x and y become those of
  its part along the wall times `1 + d`.
- **PHYS-8.6** `SV_TryUnstick` pushes the player 2 along x, y, -x, -y and the four diagonals
  in turn, slides 0.1 with the old horizontal velocity, and keeps the first that moved it more
  than 4 on x or y, returning its blocked bits; else it puts it back each time and, after all
  eight, stops it and returns 7.
- **PHYS-8.7** `SV_CheckStuck`: a player not in solid has its `origin` kept as `oldorigin`.
  One in solid is tried at `oldorigin`, then at its `origin` moved by -1 to 1 on x and y and 0
  to 17 up, x outermost after z, and is linked touching triggers at the first not in solid;
  when none is free it stays where it was.

## PHYS-9 The client's command

**Source:** `WinQuake/sv_user.c`, `SV_ReadClientMove`, `SV_ClientThink`, `SV_AirMove`,
`SV_WaterMove`, `SV_WaterJump`, `SV_UserFriction`, `SV_Accelerate`, `SV_AirAccelerate`,
`DropPunchAngle`; `WinQuake/view.c`, `V_CalcRoll`; `WinQuake/common.c`, `MSG_WriteAngle`,
`MSG_ReadAngle`; `WinQuake/cl_input.c`, `CL_SendMove`.

### Behaviour

- **PHYS-9.1** `SV_ClientThink` does nothing for `MOVETYPE_NONE`. Otherwise it takes on-ground
  from `FL_ONGROUND`, shortens `punchangle` by `10 * frametime` (not below 0), and stops for a
  player whose `health` is 0 or less. The roll of `angles` becomes 4 times `V_CalcRoll`: the
  velocity's part along the right of `angles`, its sign kept, scaled by 2/200 below 200 and 2
  from there (`cl_rollangle`, `cl_rollspeed`). Unless `fixangle` is set, the pitch becomes
  `-v_angle[0]/3` and the yaw `v_angle[1]`, `v_angle` with `punchangle` added. A player with
  `FL_WATERJUMP` water-jumps (PHYS-9.5), one at `waterlevel` 2 or more that is not
  `MOVETYPE_NOCLIP` swims (PHYS-9.5), and any other air-moves (PHYS-9.2).
- **PHYS-9.2** `SV_AirMove` takes forward and right from `angles`, the forward move 0 when
  `sv.time` is before `teleport_time` and it is backward, and wishes for
  `forward * forwardmove + right * sidemove`, its z `upmove` unless `MOVETYPE_WALK`, then 0.
  The wish is bounded to `sv_maxspeed` (320). `MOVETYPE_NOCLIP` takes it as its velocity; on
  the ground friction then acceleration apply (PHYS-9.3, PHYS-9.4); off it air acceleration.
- **PHYS-9.3** `SV_UserFriction`: with `speed` the horizontal speed, when not 0, a point 16
  ahead along it at the feet is traced 34 down (`MOVE_NOMONSTERS`); hitting nothing makes the
  friction `sv_friction * edgefriction`, 4 times 2, else 4. The speed loses
  `frametime * max(speed, sv_stopspeed) * friction`, `sv_stopspeed` 100, not below 0, and the
  velocity is scaled to it.
- **PHYS-9.4** `SV_Accelerate` adds `min(sv_accelerate * frametime * wishspeed, wishspeed -
  velocity . wishdir)` along the wish when that is above 0, `sv_accelerate` 10.
  `SV_AirAccelerate` does the same toward a wish of at most 30, with `sv_accelerate *
  wishspeed * frametime` for the step, `wishspeed` the bounded speed of PHYS-9.2.
- **PHYS-9.5** `SV_WaterMove` wishes along `v_angle`'s forward and right, its z less 60 with no
  move at all ("drift towards bottom"), else plus `upmove`; the wish's length is bounded to 320
  and scaled by 0.7. The speed loses `frametime * speed * sv_friction`, not below 0; then,
  when the wish is not 0, `min(sv_accelerate * wishspeed * frametime, wishspeed - newspeed)`
  is added along the wish when that is above 0. A water
  jump clears `FL_WATERJUMP` and `teleport_time` once `sv.time` passes `teleport_time` or the
  player is out of water, and keeps the velocity's x and y at `movedir`'s.
- **PHYS-9.6** A command arrives as `CL_SendMove` sends it: each view angle through
  `MSG_WriteAngle`, `((int)f*256/360) & 255`, read back as a signed byte times `360.0/256`;
  each move through `MSG_WriteShort`, the float to an int kept to 16 bits. It sets `v_angle`,
  `button0` from bit 0 and `button2` from bit 1, and `impulse` when it is not 0.

## PHYS-10 Entering the game

**Source:** `WinQuake/sv_main.c`, `SV_ConnectClient`, `SV_WriteClientdataToMessage`;
`WinQuake/host_cmd.c`, `Host_Spawn_f`; `WinQuake/host.c`, `Host_ServerFrame`,
`Host_FilterTime`.

### Behaviour

- **PHYS-10.1** A client connecting runs `SetNewParms` and keeps `parm1` to `parm16`. Spawning,
  its edict's fields are all cleared, `colormap` becomes its number, `team` its colour plus 1,
  `netname` its name; the kept parms are copied back; `time` is the server's and `self` the
  client; and `ClientConnect` then `PutClientInServer` run.
- **PHYS-10.2** After each frame, while the client's `fixangle` is set, its `angles` are sent
  to the client through `MSG_WriteAngle`, which takes them as its view, and `fixangle` is
  cleared.
- **PHYS-10.3** A server frame sets the `frametime` global, runs each spawned client's
  command (PHYS-9), then the physics (PHYS-1).

The frame's length is the host's: `Host_FilterTime` runs a frame once 1/72 of a second has
passed since the last, its `frametime` the time passed, 0.001 to 0.1. `qs-sim` takes the
frametime it is given; the viewer paces its frames that way.

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
| `PHYS_MAXSPEED` | 320 | `WinQuake/sv_user.c`, `sv_maxspeed = {"sv_maxspeed", "320"}` |
| `PHYS_ACCELERATE` | 10 | `WinQuake/sv_user.c`, `sv_accelerate = {"sv_accelerate", "10"}` |
| `PHYS_EDGEFRICTION` | 2 | `WinQuake/sv_user.c`, `sv_edgefriction = {"edgefriction", "2"}` |
| `PHYS_ROLLANGLE`, `PHYS_ROLLSPEED` | 2, 200 | `WinQuake/view.c`, `cl_rollangle`, `cl_rollspeed` |
| `PHYS_STEPSIZE` | 18 | `WinQuake/sv_phys.c`, `#define STEPSIZE 18` before `SV_WalkMove` |
| `PHYS_MAXFPS` | 72 | `WinQuake/host.c`, `realtime - oldrealtime < 1.0/72.0` in `Host_FilterTime` |
