# The view

Prefix: `VIEW` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes where the player's eye and weapon are drawn from and the colour the screen is tinted
with: roll from sideways movement, bobbing, pitch drift, damage kicks, idle sway, the stepping
smoothing, the intermission view, and the palette blends for water, damage, bonus items and
powerups, as implemented by `WinQuake/view.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. The GLQuake path (`#ifdef GLQUAKE`) is the one
described where the two differ, since the preview host draws through the GPU and blends the
view as GLQuake does; the software path's palette shifting is noted where it differs. To be
implemented by the preview host's client (P3-26, #240).

`cl.time` is the client's clock, `frametime` is `host_frametime`, and `cl.viewangles` are the
player's input angles in degrees (`PITCH` 0, `YAW` 1, `ROLL` 2). The view entity is
`cl_entities[cl.viewentity]`, the player's own entity as the server last sent it; the weapon is
`cl.viewent`. A setting named below is a cvar read by name; the value given is its default,
which the preview host uses unless a scenario sets it (DIFF-2.4).

## VIEW-1 Roll and bob

**Source:** `WinQuake/view.c`, `V_CalcRoll`, `V_CalcBob`.

### Behaviour

- **VIEW-1.1** `V_CalcRoll (angles, velocity)`: `side = velocity . right` of `angles`'
  right vector (the same `AngleVectors` as the server, PHYS-8); the sign is -1 when `side < 0`
  and 1 otherwise, and `side` becomes its absolute value. The roll is `sign * cl_rollangle *
  side / cl_rollspeed` while `side < cl_rollspeed`, and `sign * cl_rollangle` from there.
  The server calls it too, for the player's `angles[ROLL]` (PHYS-8); the client adds it to the
  view (VIEW-3.2).
- **VIEW-1.2** `V_CalcBob`: `cycle = (cl.time - (int) (cl.time / cl_bobcycle) * cl_bobcycle)
  / cl_bobcycle`, the fraction of the current bob cycle; for `cycle < cl_bobup` it is `pi *
  cycle / cl_bobup`, else `pi + pi * (cycle - cl_bobup) / (1 - cl_bobup)`. `bob = |velocity
  xy| * cl_bob`, then `bob * 0.3 + bob * 0.7 * sin (cycle)`, clamped to at most 4 and at least
  -7. `velocity` is `cl.velocity`, the player's velocity as the server last sent it.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `VIEW_ROLLSPEED` | 200 | `view.c`, `cl_rollspeed` default |
| `VIEW_ROLLANGLE` | 2.0 | `view.c`, `cl_rollangle` default |
| `VIEW_BOB` | 0.02 | `view.c`, `cl_bob` default |
| `VIEW_BOBCYCLE` | 0.6 | `view.c`, `cl_bobcycle` default |
| `VIEW_BOBUP` | 0.5 | `view.c`, `cl_bobup` default |
| `VIEW_BOB_MAX` | 4 | `view.c`, `V_CalcBob` |
| `VIEW_BOB_MIN` | -7 | `view.c`, `V_CalcBob` |

## VIEW-2 Pitch drift

**Source:** `WinQuake/view.c`, `V_StartPitchDrift`, `V_StopPitchDrift`, `V_DriftPitch`.

The view pitch drifts back toward `cl.idealpitch`, the pitch the server sends with the client
data (CLIENT-3), when the player runs forward without looking up or down.

### Behaviour

- **VIEW-2.1** `V_StartPitchDrift` does nothing when `cl.laststop == cl.time` ("something
  else is keeping it from drifting"); otherwise, when `cl.nodrift` is set or `cl.pitchvel` is
  0, it sets `pitchvel = v_centerspeed`, clears `nodrift` and sets `driftmove = 0`.
  `V_StopPitchDrift` sets `laststop = cl.time`, `nodrift` and `pitchvel = 0`. The `centerview`
  command starts the drift; looking with the keys or the mouse stops it (`cl_input.c`,
  not described).
- **VIEW-2.2** `V_DriftPitch`, each frame before the view is built: while the player is not on
  the ground, or noclipping, or a demo plays, `driftmove = 0` and `pitchvel = 0` and nothing
  drifts. With `nodrift` set, `driftmove` is 0 when `|cl.cmd.forwardmove| < cl_forwardspeed`
  and grows by `frametime` otherwise, and when it passes `v_centermove` the drift starts
  (VIEW-2.1); nothing moves this frame.
- **VIEW-2.3** Otherwise `delta = idealpitch - viewangles[PITCH]`; 0 sets `pitchvel = 0`.
  Else `move = frametime * pitchvel`, `pitchvel += frametime * v_centerspeed`, and the pitch
  moves toward `idealpitch` by `move`, by `|delta|` with `pitchvel = 0` when `move` would
  overshoot.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `VIEW_CENTERMOVE` | 0.15 | `view.c`, `v_centermove` default |
| `VIEW_CENTERSPEED` | 500 | `view.c`, `v_centerspeed` default |
| `VIEW_FORWARDSPEED` | 200 | `cl_input.c`, `cl_forwardspeed` default |

## VIEW-3 The eye and the weapon

**Source:** `WinQuake/view.c`, `V_CalcRefdef`, `V_CalcViewRoll`, `V_AddIdle`,
`V_BoundOffsets`, `CalcGunAngle`, `V_CalcIntermissionRefdef`, `V_RenderView`.

### Behaviour

- **VIEW-3.1** Each frame that is not an intermission and not paused, the view is built from
  the view entity: the entity's `angles[YAW]` is set to `viewangles[YAW]` and its
  `angles[PITCH]` to `-viewangles[PITCH]` ("the model should face"); `bob` is VIEW-1.2; the
  eye is `origin + (0, 0, viewheight + bob) + (1/32, 1/32, 1/32)` ("never let it sit exactly
  on a node line"), where `viewheight` is the client data's; the view angles are
  `viewangles`.
- **VIEW-3.2** `V_CalcViewRoll` adds VIEW-1.1 of the view entity's angles and `cl.velocity` to
  the view roll. While a damage kick is pending (`v_dmg_time > 0`), roll gets `v_dmg_time /
  v_kicktime * v_dmg_roll` and pitch `v_dmg_time / v_kicktime * v_dmg_pitch`, and
  `v_dmg_time` loses `frametime`. When `STAT_HEALTH` is 0 or less the roll is 80 ("dead view
  angle") and nothing else is added.
- **VIEW-3.3** `V_AddIdle` adds `v_idlescale * sin (cl.time * cycle) * level` to each of roll,
  pitch and yaw with that angle's `v_i<angle>_cycle` and `v_i<angle>_level`; `v_idlescale`
  is 0 by default, so nothing moves outside the intermission.
- **VIEW-3.4** The eye then moves by `scr_ofsx * forward + scr_ofsy * right + scr_ofsz * up`
  of the entity's angles with the pitch negated (0 by default, and forced to 0 with more than
  one client), and `V_BoundOffsets` clamps it to within 14 units of the entity's origin on x
  and y, and from 22 below to 30 above on z.
- **VIEW-3.5** The weapon's angles start as `viewangles`; `CalcGunAngle` then sets its yaw to
  `viewangles[YAW] + yaw` and its pitch to `-(viewangles[PITCH] + pitch)`, where `yaw` and
  `pitch` are the smoothed gun lag: each frame the target lag is 0 for both (the code
  computes `angledelta (yaw - yaw) * 0.4` and the same for pitch, which is 0), clamped to
  plus or minus 10, and the lag moves from its previous value toward the target by at most
  `frametime * 20`; so the gun swings back to the view angles at 20 degrees per second after
  any lag. Then each of the weapon's roll, pitch and yaw loses the VIEW-3.3 idle term.
- **VIEW-3.6** The weapon's origin is the entity's `origin + (0, 0, viewheight)`, plus
  `forward * bob * 0.4` (the forward of VIEW-3.4's angles) and `bob` on z, plus 1 on z at
  `scr_viewsize` 110, 2 at 100, 1 at 90 and 0.5 at 80. Its model is
  `model_precache[STAT_WEAPON]`, its frame `STAT_WEAPONFRAME`; then `cl.punchangle` (the
  client data's `punchangle`) is added to the view angles.
- **VIEW-3.7** Stepping is smoothed: while the player is on the ground and the entity's z
  rose above the remembered `oldz`, `oldz` rises by `steptime * 80` (with `steptime = cl.time
  - cl.oldtime`, at least 0), never above the entity's z and never more than 12 below it, and
  both the eye and the weapon drop by `origin.z - oldz`; otherwise `oldz` is the entity's z.
- **VIEW-3.8** During an intermission the eye is the view entity's origin and the view
  angles its angles, the weapon has no model, and VIEW-3.3 runs with `v_idlescale` 1.
- **VIEW-3.9** The view is not built while `con_forcedup` (the console covers the screen
  before a map is loaded); while paused the previous view stands.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `VIEW_KICKTIME` | 0.5 | `view.c`, `v_kicktime` default |
| `VIEW_KICKROLL` | 0.6 | `view.c`, `v_kickroll` default |
| `VIEW_KICKPITCH` | 0.6 | `view.c`, `v_kickpitch` default |
| `VIEW_IYAW_CYCLE` | 2 | `view.c`, `v_iyaw_cycle` default |
| `VIEW_IROLL_CYCLE` | 0.5 | `view.c`, `v_iroll_cycle` default |
| `VIEW_IPITCH_CYCLE` | 1 | `view.c`, `v_ipitch_cycle` default |
| `VIEW_IYAW_LEVEL` | 0.3 | `view.c`, `v_iyaw_level` default |
| `VIEW_IROLL_LEVEL` | 0.1 | `view.c`, `v_iroll_level` default |
| `VIEW_IPITCH_LEVEL` | 0.3 | `view.c`, `v_ipitch_level` default |
| `VIEW_IDLESCALE` | 0 | `view.c`, `v_idlescale` default |
| `VIEW_DEAD_ROLL` | 80 | `view.c`, `V_CalcViewRoll` |
| `VIEW_OFFSET_XY` | 14 | `view.c`, `V_BoundOffsets` |
| `VIEW_OFFSET_DOWN` | 22 | `view.c`, `V_BoundOffsets` |
| `VIEW_OFFSET_UP` | 30 | `view.c`, `V_BoundOffsets` |
| `VIEW_GUN_LAG_MAX` | 10 | `view.c`, `CalcGunAngle` |
| `VIEW_GUN_LAG_SPEED` | 20 | `view.c`, `CalcGunAngle` (`host_frametime * 20`) |
| `VIEW_STEP_SPEED` | 80 | `view.c`, `V_CalcRefdef` (`steptime * 80`) |
| `VIEW_STEP_MAX` | 12 | `view.c`, `V_CalcRefdef` |
| `VIEW_NUDGE` | 1/32 | `view.c`, `V_CalcRefdef` |

## VIEW-4 Colour shifts

**Source:** `WinQuake/view.c`, `V_ParseDamage`, `V_BonusFlash_f`, `V_SetContentsColor`,
`V_CalcPowerupCshift`, `V_CalcBlend`, `V_UpdatePalette`, `BuildGammaTable`.

The client keeps `NUM_CSHIFTS` shifts, each a colour and a percent: `CSHIFT_CONTENTS` (0),
`CSHIFT_DAMAGE` (1), `CSHIFT_BONUS` (2) and `CSHIFT_POWERUP` (3).

### Behaviour

- **VIEW-4.1** `svc_damage` (CLIENT-1.4) carries `armor` and `blood` bytes and a `from` point.
  `count = blood * 0.5 + armor * 0.5`, at least 10; `cl.faceanimtime = cl.time + 0.2`; the
  damage shift's percent grows by `3 * count`, clamped to 0 to 150, and its colour is
  (200, 100, 100) when `armor > blood`, (220, 50, 50) when `armor` is not 0, else
  (255, 0, 0). The kick is set from the direction `from - origin` of the view entity,
  normalised: `v_dmg_roll = count * (dir . right) * v_kickroll`, `v_dmg_pitch = count * (dir
  . forward) * v_kickpitch` with the entity's angles' vectors, and `v_dmg_time = v_kicktime`.
- **VIEW-4.2** The `bf` command (sent by the server when an item is picked up) sets the bonus
  shift to (215, 186, 69) at 50 percent.
- **VIEW-4.3** The contents shift follows the contents of the leaf the eye is in (BSP-6):
  `CONTENTS_EMPTY` and `CONTENTS_SOLID` give (130, 80, 50) at 0, `CONTENTS_LAVA` (255, 80, 0)
  at 150, `CONTENTS_SLIME` (0, 25, 5) at 150, and any other (water) (130, 80, 50) at 128.
- **VIEW-4.4** The powerup shift is, by the first of `cl.items` set: `IT_QUAD` (0, 0, 255) at
  30, `IT_SUIT` (0, 255, 0) at 20, `IT_INVISIBILITY` (100, 100, 100) at 100,
  `IT_INVULNERABILITY` (255, 255, 0) at 30; none gives percent 0.
- **VIEW-4.5** Each frame the damage percent loses `frametime * 150` and the bonus percent
  `frametime * 100`, each stopping at 0.
- **VIEW-4.6** GLQuake's blend (`V_CalcBlend`) folds the shifts in index order into one
  RGBA: for a shift with `a2 = (percent * gl_cshiftpercent / 100) / 255` not 0, `a = a + a2 *
  (1 - a)`, `a2 = a2 / a`, and each colour channel becomes `c * (1 - a2) + destcolor * a2`;
  the result is the colour over 255 and `a` clamped to 0 to 1, drawn over the whole view. The
  software renderer instead shifts the palette, each entry `c += (percent * (destcolor - c))
  >> 8` per shift in order, through the gamma table; the preview host draws the blend.
- **VIEW-4.7** The gamma table maps `i` to `(int) (255 * pow ((i + 0.5) / 255.5, gamma) +
  0.5)` clamped to 0 to 255, the identity at gamma 1 (the default).

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `VIEW_NUM_CSHIFTS` | 4 | `client.h`, `NUM_CSHIFTS` |
| `VIEW_DAMAGE_MIN_COUNT` | 10 | `view.c`, `V_ParseDamage` |
| `VIEW_DAMAGE_MAX_PERCENT` | 150 | `view.c`, `V_ParseDamage` |
| `VIEW_DAMAGE_FADE` | 150 | `view.c`, `V_UpdatePalette` (per second) |
| `VIEW_BONUS_FADE` | 100 | `view.c`, `V_UpdatePalette` (per second) |
| `VIEW_CSHIFTPERCENT` | 100 | `view.c`, `gl_cshiftpercent` default |
| `VIEW_FACE_ANIM_TIME` | 0.2 | `view.c`, `V_ParseDamage` |
