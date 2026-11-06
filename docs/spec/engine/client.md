# Client state

Prefix: `CLIENT` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes what the client keeps of the server's world and how it turns the server's messages
into entities to draw each frame: the entity slots and their two latest states, the baseline
and delta updates, the client data, the interpolation of positions between server frames,
dynamic lights, temporary entities and beams, and the smoothing the FitzQuake lineage adds,
as implemented by `WinQuake/cl_main.c`, `cl_parse.c`, `cl_tent.c` and `protocol.h` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, and for CLIENT-7 by `Quake/cl_parse.c`,
`cl_main.c` and `r_alias.c` in [sezero/quakespasm](https://github.com/sezero/quakespasm) at
commit `c9c8ee9895961669c4674e8f8ca8c64cc43e38b5` (ADR-0018). To be implemented by the
preview host's client (P3-26, #240).

The preview host's client and server run in one process and the server's messages never
cross a socket (ADR-0004): the message semantics below still decide what the client knows
when, since the server writes an entity only when it is in the client's visible set and sends
only the fields that differ from the entity's baseline. `cl.time` is the client's clock,
advanced by `frametime` each frame; `cl.mtime[0]` is the time of the latest server message
and `cl.mtime[1]` the one before. A coordinate on the wire is a short over 8 and an angle a
byte over 256 of a turn (`MSG_ReadCoord`, `MSG_ReadAngle`), so positions arrive rounded to an
eighth of a unit and angles to 1.40625 degrees.

## CLIENT-1 Entities and messages

**Source:** `WinQuake/cl_main.c`, `CL_ClearState`; `WinQuake/cl_parse.c`, `CL_EntityNum`,
`CL_ParseServerInfo`, `CL_ParseBaseline`, `CL_ParseStatic`, `CL_ParseServerMessage`;
`WinQuake/protocol.h`.

### Behaviour

- **CLIENT-1.1** A new server (`svc_serverinfo`) clears the client's state: every entity
  slot, dynamic light, light style, temporary entity and beam is zeroed. The message then
  carries the protocol version (`CLIENT_PROTOCOL_VERSION`, any other is refused with
  `Server returned version <n>, not 15`), `maxclients` (1 to `CLIENT_MAX_SCOREBOARD`), the game
  type, the level name, the model names in precache order from index 1 to an empty name (more
  than `CLIENT_MAX_MODELS` is `Server sent too many model precaches`) and the sound names the
  same way (`CLIENT_MAX_SOUNDS`); every model is loaded (a missing one is `Model <name> not
  found` and the connection stops) and every sound precached (SND-1.3). The world model is
  model 1.
- **CLIENT-1.2** `CL_EntityNum (n)` is slot `n`; a slot at or past `cl.num_entities` raises
  `num_entities` to `n + 1`, and `n` at or past `MAX_EDICTS` is the error `CL_EntityNum: <n>
  is an invalid number`.
- **CLIENT-1.3** `svc_spawnbaseline` (a short entity number) and `svc_spawnstatic` read a
  baseline: a model index byte, a frame byte, a colormap byte, a skin byte, then an origin
  coordinate and an angle for each axis in turn. A static entity is the next of at most
  `CLIENT_MAX_STATIC_ENTITIES` (`Too many static entities` past that), drawn from its baseline
  at the baseline's origin and angles and never updated.
- **CLIENT-1.4** A server message is a sequence of commands read until the end of the data;
  a byte with the high bit set is an entity update whose low seven bits are the first update
  bits (CLIENT-2). The other commands: `svc_nop`; `svc_time` (a float: `mtime[1] = mtime[0]`,
  then `mtime[0]` is read); `svc_clientdata` (a short of bits, CLIENT-3); `svc_version` (a
  long that must be the protocol version); `svc_disconnect`; `svc_print` (a string to the
  console); `svc_centerprint` (HUD-3); `svc_stufftext` (a string for the command buffer);
  `svc_damage` (VIEW-4.1); `svc_serverinfo`; `svc_setangle` (three angles into
  `viewangles`); `svc_setview` (a short, the view entity); `svc_lightstyle` (a byte index
  below `MAX_LIGHTSTYLES`, else `svc_lightstyle > MAX_LIGHTSTYLES`, and the style's string,
  REND-5); `svc_sound` (CLIENT-4.1); `svc_stopsound` (a short: entity `n >> 3`, channel
  `n & 7`, SND-2.6); `svc_updatename`, `svc_updatefrags` and `svc_updatecolors` (a client
  byte below `maxclients`, else an error, then a string, a short or a byte; new colours
  rebuild the client's translation, CLIENT-1.6); `svc_particle` (PART-2.4);
  `svc_spawnbaseline`; `svc_spawnstatic`; `svc_temp_entity` (CLIENT-6); `svc_setpause` (a
  byte); `svc_signonnum` (a byte that must exceed the current signon, else `Received signon
  <n> when at <m>`); `svc_killedmonster` and `svc_foundsecret` (`STAT_MONSTERS` and
  `STAT_SECRETS` grow by one); `svc_updatestat` (a byte below `MAX_CL_STATS`, then a long);
  `svc_spawnstaticsound` (CLIENT-4.2); `svc_cdtrack` (two bytes: the track and the loop
  track, RRE-3); `svc_intermission` (intermission 1), `svc_finale` (2, with a centerprint
  string) and `svc_cutscene` (3, the same), each recording `completed_time = cl.time`;
  `svc_sellscreen`. Any other command is the error `CL_ParseServerMessage: Illegible server
  message`, and reading past the end is `Bad server message`.
- **CLIENT-1.5** The signon sequence: the client answers signon 1 with `prespawn`, 2 with its
  `name`, `color` and `spawn <parms>` commands, 3 with `begin`, and the first entity update
  after signon 3 completes it (`SIGNONS`, 4).
- **CLIENT-1.6** A client's colours byte holds the shirt in its high nibble and the trousers
  in its low one; `CL_NewTranslation` copies the colormap and replaces its `TOP_RANGE` and
  `BOTTOM_RANGE` rows (16 entries each, for each of the `VID_GRADES` shades) with the rows at
  `top = colors & 0xf0` and `bottom = (colors & 15) << 4`, reversed when the row index is 128
  or more ("the artists made some backwards ranges"). An entity whose colormap byte is `c`
  (1 to `maxclients`) is drawn through client `c`'s translation; 0 is the plain colormap, and
  more than `maxclients` is a fatal error.
- **CLIENT-1.7** After the level name is read, the client prints a divider, two line feeds,
  `\35`, 35 of `\36`, `\37` and two line feeds, then the level name after the colour byte 2
  and a line feed ("`Con_Printf ("%c%s\n", 2, str);`"), so the console shows the level's name
  in the other colour below a rule.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `CLIENT_PROTOCOL_VERSION` | 15 | `protocol.h`, `PROTOCOL_VERSION` |
| `CLIENT_MAX_SCOREBOARD` | 16 | `quakedef.h`, `MAX_SCOREBOARD` |
| `CLIENT_MAX_MODELS` | 256 | `quakedef.h`, `MAX_MODELS` |
| `CLIENT_MAX_SOUNDS` | 256 | `quakedef.h`, `MAX_SOUNDS` |
| `CLIENT_MAX_STATIC_ENTITIES` | 128 | `client.h`, `MAX_STATIC_ENTITIES` |
| `CLIENT_MAX_VISEDICTS` | 256 | `client.h`, `MAX_VISEDICTS` |
| `CLIENT_SIGNONS` | 4 | `client.h`, `SIGNONS` |
| `CLIENT_MAX_LIGHTSTYLES` | 64 | `quakedef.h`, `MAX_LIGHTSTYLES` |
| `CLIENT_MAX_STYLESTRING` | 64 | `quakedef.h`, `MAX_STYLESTRING` |

## CLIENT-2 Entity updates

**Source:** `WinQuake/cl_parse.c`, `CL_ParseUpdate`; `WinQuake/protocol.h`, the `U_` bits.

### Behaviour

- **CLIENT-2.1** The update's bits are the command's low seven; `U_MOREBITS` (1) reads a
  second byte into bits 8 to 15. The entity number is a short when `U_LONGENTITY` (bit 14)
  is set, else a byte. The slot is CLIENT-1.2.
- **CLIENT-2.2** The slot keeps the time of its last update, `msgtime`: when it is not
  `mtime[1]` (the entity was not in the previous message) the update is forced
  (`forcelink`), and `msgtime` becomes `mtime[0]`.
- **CLIENT-2.3** Each field is read when its bit is set and taken from the baseline
  otherwise: `U_MODEL` (bit 10) a model index byte (at or past `CLIENT_MAX_MODELS` is the
  error `CL_ParseModel: bad modnum`); `U_FRAME` (6) a frame byte; `U_COLORMAP` (11) a colormap
  byte (CLIENT-1.6); `U_SKIN` (12) a skin byte; `U_EFFECTS` (13) an effects byte; `U_ORIGIN1`,
  `U_ORIGIN2` and `U_ORIGIN3` (1, 2, 3) each a coordinate; `U_ANGLE1`, `U_ANGLE2` and
  `U_ANGLE3` (8, 4, 9) each an angle, in the wire order x origin, x angle, y origin, y angle,
  z origin, z angle. Before the new origin and angles are read, the previous ones move to
  `msg_origins[1]` and `msg_angles[1]`.
- **CLIENT-2.4** A model that differs from the slot's is set; a model with `synctype`
  `ST_RAND` gives the slot `syncbase = (rand () & 0x7fff) / 0x7fff` (a float from 0 to 1),
  any other model 0; no model forces the update ("hack to make null model players work").
- **CLIENT-2.5** `U_NOLERP` (5) marks the slot `forcelink`, so CLIENT-5.4 draws it at
  `msg_origins[0]` with no interpolation; nothing else is copied. An update forced by
  CLIENT-2.2 or CLIENT-2.4 also copies the new origin and angles into `msg_origins[1]`,
  `msg_angles[1]`, `origin` and `angles`, and marks the slot the same way.

## CLIENT-3 Client data

**Source:** `WinQuake/cl_parse.c`, `CL_ParseClientdata`; `WinQuake/protocol.h`, the `SU_`
bits.

### Behaviour

- **CLIENT-3.1** `svc_clientdata`'s short holds the bits. `SU_VIEWHEIGHT` (bit 0) reads a
  signed byte into `viewheight`, else `CLIENT_DEFAULT_VIEWHEIGHT`; `SU_IDEALPITCH` (1) a
  signed byte into `idealpitch`, else 0; for each axis `i`, `SU_PUNCH1 << i` (2, 3, 4) a
  signed byte into `punchangle[i]`, else 0, and `SU_VELOCITY1 << i` (5, 6, 7) a signed byte
  times 16 into `mvelocity[0][i]`, else 0, with the previous velocity moved to
  `mvelocity[1]` first.
- **CLIENT-3.2** Then a long of items: a bit newly set records `item_gettime[bit] =
  cl.time` (the HUD's flash, HUD-1.4). `SU_ONGROUND` (10) and `SU_INWATER` (11) carry no
  data. `SU_WEAPONFRAME` (12) reads a byte into `STAT_WEAPONFRAME`, else 0; `SU_ARMOR` (13) a
  byte into `STAT_ARMOR`, else 0; `SU_WEAPON` (14) a byte into `STAT_WEAPON`, else 0. Then
  always: a short `STAT_HEALTH`, a byte `STAT_AMMO`, four bytes `STAT_SHELLS` to
  `STAT_CELLS`, and a byte that is `STAT_ACTIVEWEAPON` in standard Quake and `1 << byte` when
  the game runs as Rogue or Hipnotic (`-rogue` and `-hipnotic` both clear `standard_quake`,
  `common.c`).
- **CLIENT-3.3** `onground` is cleared at the start of every server message ("unless the
  server says otherwise") and set by `SU_ONGROUND`.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `CLIENT_DEFAULT_VIEWHEIGHT` | 22 | `protocol.h`, `DEFAULT_VIEWHEIGHT` |

## CLIENT-4 Sounds from the server

**Source:** `WinQuake/cl_parse.c`, `CL_ParseStartSoundPacket`, `CL_ParseStaticSound`.

### Behaviour

- **CLIENT-4.1** `svc_sound`: a field byte, then a volume byte when `SND_VOLUME` (1) is set
  (else `SND_PACKET_VOLUME`), an attenuation byte over 64 when `SND_ATTENUATION` (2) is set
  (else `SND_PACKET_ATTENUATION`), a short holding the entity in its high bits and the channel
  in its low three, a sound index byte and the origin's three coordinates; an entity above
  `MAX_EDICTS` is the error `CL_ParseStartSoundPacket: ent = <n>`. The sound starts as SND-2
  with `fvol = volume / 255`.
- **CLIENT-4.2** `svc_spawnstaticsound`: three coordinates, a sound index byte, a volume byte
  and an attenuation byte, passed to SND-4.1 as they are (the attenuation is divided by 64
  there).

## CLIENT-5 Relinking and interpolation

**Source:** `WinQuake/cl_main.c`, `CL_ReadFromServer`, `CL_LerpPoint`, `CL_RelinkEntities`,
`CL_AllocDlight`, `CL_DecayLights`.

### Behaviour

- **CLIENT-5.1** Each client frame, `cl.oldtime = cl.time` and `cl.time += frametime`; the
  server's messages are read, then the entities are relinked and the temporary entities built
  (CLIENT-6.4).
- **CLIENT-5.2** `CL_LerpPoint`: with `f = mtime[0] - mtime[1]`, when `f` is 0, or `cl_nolerp`
  is set, or a time demo plays, or the server is local (`sv.active`), `cl.time = mtime[0]` and
  the fraction is 1. The preview host's client and server share one process, so by id's rule
  it would never interpolate; it interpolates anyway, a deliberate divergence, because its
  server ticks at a fixed rate below the display's (P3-02) while id's and QuakeSpasm's local
  server runs once per drawn frame, which is what makes their rule harmless. Otherwise `f`
  above 0.1 is treated as a dropped packet:
  `mtime[1] = mtime[0] - 0.1`, `f = 0.1`; `frac = (cl.time - mtime[1]) / f`; below 0 it is 0
  (and `cl.time = mtime[1]` when below -0.01), above 1 it is 1 (and `cl.time = mtime[0]`
  when above 1.01).
- **CLIENT-5.3** `cl.velocity` is `mvelocity[1] + frac * (mvelocity[0] - mvelocity[1])`.
  `bobjrotate = anglemod (100 * cl.time)`.
- **CLIENT-5.4** For each slot from 1 below `num_entities`: a slot with no model is skipped;
  one whose `msgtime` is not `mtime[0]` loses its model (it was not in the last message) and
  is skipped. A forced slot takes `msg_origins[0]` and `msg_angles[0]`. Otherwise `f = frac`,
  or 1 when any axis of `msg_origins[0] - msg_origins[1]` is more than 100 away ("assume a
  teleportation"); `origin = msg_origins[1] + f * delta` and each angle `msg_angles[1] + f *
  d` with `d` the difference wrapped into -180 to 180. Then `forcelink` is cleared.
- **CLIENT-5.5** A model flagged `EF_ROTATE` takes yaw `bobjrotate`. Effects:
  `EF_BRIGHTFIELD` runs PART-2.8; `EF_MUZZLEFLASH` allocates the slot's dynamic light at
  `origin + (0, 0, 16) + 18 * forward` of the angles, radius `200 + (rand () & 31)`,
  `minlight` 32, dying at `cl.time + 0.1`; `EF_BRIGHTLIGHT` one at `origin + (0, 0, 16)`,
  radius `400 + (rand () & 31)`, dying at `cl.time + 0.001`; `EF_DIMLIGHT` one at `origin`,
  radius `200 + (rand () & 31)`, the same death. Model flags, by the first set of `EF_GIB`
  (trail 2), `EF_ZOMGIB` (4), `EF_TRACER` (3), `EF_TRACER2` (5), `EF_ROCKET` (0, plus a light
  at `origin` of radius 200 dying at `cl.time + 0.01`), `EF_GRENADE` (1) and `EF_TRACER3`
  (6), draw PART-2.7 from the slot's previous origin to its new one.
- **CLIENT-5.6** The view entity is not drawn (unless the chase camera is on); every other
  relinked slot joins the visible list, up to `CLIENT_MAX_VISEDICTS`.
- **CLIENT-5.7** `CL_AllocDlight (key)`: a `key` other than 0 reuses the light with that
  key; else the first light whose `die` is before `cl.time`; else light 0. The light is
  cleared and keyed. `CL_DecayLights`, each frame: a live light with a radius loses
  `(cl.time - cl.oldtime) * decay` of radius, not below 0. There are `CLIENT_MAX_DLIGHTS`
  lights; how they light the world is REND-7.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `CLIENT_MAX_DLIGHTS` | 32 | `client.h`, `MAX_DLIGHTS` |
| `CLIENT_LERP_MAX_GAP` | 0.1 | `cl_main.c`, `CL_LerpPoint` |
| `CLIENT_TELEPORT_DISTANCE` | 100 | `cl_main.c`, `CL_RelinkEntities` |
| `CLIENT_ROTATE_SPEED` | 100 | `cl_main.c`, `CL_RelinkEntities` (degrees per second) |

## CLIENT-6 Temporary entities and beams

**Source:** `WinQuake/cl_tent.c`, `CL_InitTEnts`, `CL_ParseTEnt`, `CL_ParseBeam`,
`CL_NewTempEntity`, `CL_UpdateTEnts`; `WinQuake/protocol.h`, the `TE_` numbers.

### Behaviour

- **CLIENT-6.1** `svc_temp_entity` reads a type byte. The types with a position (three
  coordinates): `TE_WIZSPIKE` (7) runs PART-2.4 with colour 20 and count 30 and plays
  `wizard/hit.wav`; `TE_KNIGHTSPIKE` (8) colour 226, count 20, `hknight/hit.wav`; `TE_SPIKE`
  (0) colour 0, count 10, and `TE_SUPERSPIKE` (1) colour 0, count 20, each then
  `weapons/tink1.wav` four times in five (`rand () % 5` not 0) and otherwise `weapons/ric1.wav`,
  `ric2.wav` or `ric3.wav` by `rand () & 3` (1, 2, else); `TE_GUNSHOT` (2) colour 0, count 20
  and no sound; `TE_EXPLOSION` (3) PART-2.1, a dynamic light (key 0) at the position of radius
  350, dying at `cl.time + 0.5` with decay 300, and `weapons/r_exp3.wav`; `TE_TAREXPLOSION`
  (4) PART-2.3 and `weapons/r_exp3.wav`; `TE_LAVASPLASH` (10) PART-2.5; `TE_TELEPORT` (11)
  PART-2.6; `TE_EXPLOSION2` (12) reads two more bytes, a colour start and length, runs
  PART-2.2 and the explosion's light and sound. Every sound here starts on entity -1, channel
  0, volume 1, attenuation 1.
- **CLIENT-6.2** The beam types `TE_LIGHTNING1` (5), `TE_LIGHTNING2` (6), `TE_LIGHTNING3` (9)
  and `TE_BEAM` (13) read a short entity and two positions and keep a beam of the model
  `progs/bolt.mdl`, `progs/bolt2.mdl`, `progs/bolt3.mdl` or `progs/beam.mdl`: the beam of
  that entity if one exists, else the first with no model or already ended; none free prints
  `beam list overflow!`. The beam ends at `cl.time + 0.2`. There are `CLIENT_MAX_BEAMS`.
- **CLIENT-6.3** Any other type is the fatal error `CL_ParseTEnt: bad type`.
- **CLIENT-6.4** After relinking, the temporary entities are rebuilt from the live beams: a
  beam of the view entity starts at the view entity's origin. Its direction `dist = end -
  start` gives yaw 0 and pitch 90 (or 270 when `dist.z` is not above 0) when `dist.x` and
  `dist.y` are both 0, else `yaw = (int) (atan2 (dist.y, dist.x) * 180 / pi)` and `pitch =
  (int) (atan2 (dist.z, |dist.xy|) * 180 / pi)`, each raised by 360 when negative. One
  temporary entity of the beam's model is placed every 30 units from `start` while the
  remaining length is above 0, with angles `(pitch, yaw, rand () % 360)`, up to
  `CLIENT_MAX_TEMP_ENTITIES` and the visible list's limit. id's loop reuses its beam index to
  advance `org` (`for (i=0 ; i<3 ; i++)` inside `for (i=0, b=cl_beams ; i< MAX_BEAMS ; i++,
  b++)`), so after a beam that placed a segment `i` is 4 while `b` keeps advancing past
  `cl_beams` into whatever memory follows: undefined behaviour on x86 that stops once 24
  consecutive slots neither have a model nor are live. The preview host walks every beam
  with its own index, as QuakeSpasm does (`cl_tent.c`, `j`), a deliberate divergence from the
  undefined read.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `CLIENT_MAX_BEAMS` | 24 | `client.h`, `MAX_BEAMS` |
| `CLIENT_MAX_TEMP_ENTITIES` | 64 | `client.h`, `MAX_TEMP_ENTITIES` |
| `CLIENT_BEAM_TIME` | 0.2 | `cl_tent.c`, `CL_ParseBeam` |
| `CLIENT_BEAM_STEP` | 30 | `cl_tent.c`, `CL_UpdateTEnts` |

## CLIENT-7 Interpolation of animation and stepping movement

**Source:** `Quake/cl_parse.c`, `CL_ParseUpdate`; `Quake/cl_main.c`, `CL_RelinkEntities`;
`Quake/r_alias.c`, `R_SetupAliasFrame`, `R_SetupEntityTransform`
(sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`).

id's client draws each entity at the frame the server last sent and moves a `MOVETYPE_STEP`
monster in ten-per-second steps. FitzQuake interpolates both, which the preview host does by
default under ADR-0012's rule (a reference engine's fix for a visible defect is on, id's
behaviour the option, `r_lerpmodels 0` and `r_lerpmove 0`). The flags: `LERP_MOVESTEP`,
`LERP_RESETANIM`, `LERP_RESETANIM2`, `LERP_RESETMOVE`, `LERP_FINISH`.

### Behaviour

- **CLIENT-7.1** An update whose previous `msgtime` is more than 0.2 seconds before
  `mtime[0]` sets `LERP_RESETANIM` ("if we missed a think"); a model change sets it too; a
  slot dropped from the message sets `LERP_RESETMOVE | LERP_RESETANIM` for its next use, and
  so does a new slot. A muzzle flash sets `LERP_RESETANIM | LERP_RESETANIM2` on the entity
  (the weapon for the view entity), so no animation is interpolated for two frames. The
  server marks a `MOVETYPE_STEP` entity's update with `U_STEP`, bit 5, id's `U_NOLERP` renamed
  "since it's only used for MOVETYPE_STEP", which sets `LERP_MOVESTEP` and forces the update
  (CLIENT-2.5); without it the flag is cleared.
- **CLIENT-7.2** A teleport (CLIENT-5.4) sets `LERP_RESETMOVE`, and an entity with
  `LERP_MOVESTEP` under `r_lerpmove` is relinked with `f = 1` ("don't cl_lerp entities that
  will be r_lerped").
- **CLIENT-7.3** `R_SetupAliasFrame`: a frame out of the model's range is drawn as frame 0
  (`R_AliasSetupFrame: no such frame`). The pose is the frame's first pose, plus `(int)
  (cl.time / interval) % numposes` for a frame group (REND-4), whose `lerptime` is the
  group's interval; a single pose has `lerptime` 0.1. `LERP_RESETANIM` restarts: `lerpstart =
  0` and both poses are the new one. Else a pose other than the current starts a lerp:
  `lerpstart = cl.time`, previous = current, current = new, unless `LERP_RESETANIM2` defers it
  one more time (restarting as above and clearing the flag). The blend is `(cl.time -
  lerpstart) / lerptime` clamped to 0 to 1 (or over `lerpfinish - lerpstart` for a single
  pose with `LERP_FINISH`); a blend of 1 makes the previous pose the current. The vertices
  drawn are `previous + blend * (current - previous)` per pose vertex. With `r_lerpmodels` 0,
  or 1 on a model flagged `MOD_NOLERP`, the blend is 1 and both poses the new one.
- **CLIENT-7.4** `R_SetupEntityTransform`: `LERP_RESETMOVE` resets the move lerp to the
  entity's origin and angles; else an origin or angles that differ from the current start a
  move lerp at `cl.time`. With `r_lerpmove`, for an entity with `LERP_MOVESTEP` that is not
  the weapon, the blend is `(cl.time - movelerpstart) / 0.1` clamped to 0 to 1 (or over the
  `LERP_FINISH` span), the origin `previous + blend * d` and each angle `previous + blend *
  d` with `d` wrapped into -180 to 180; any other entity is drawn at its origin and angles.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `CLIENT_LERP_RESET_GAP` | 0.2 | `cl_parse.c`, `CL_ParseUpdate` |
| `CLIENT_LERP_SINGLE_POSE` | 0.1 | `r_alias.c`, `R_SetupAliasFrame` |
| `CLIENT_LERP_MOVE_TIME` | 0.1 | `r_alias.c`, `R_SetupEntityTransform` |
| `CLIENT_U_STEP` | `1 << 5` | `protocol.h`, `U_STEP` (QuakeSpasm; id's `U_NOLERP`) |

## CLIENT-8 Limits

**Source:** `WinQuake/client.h` at id-Software/Quake@`bf4ac424`; `Quake/client.h` at
sezero/quakespasm@`c9c8ee98`: "#define MAX_DLIGHTS 64 //johnfitz -- was 32", "#define MAX_BEAMS
32 //johnfitz -- was 24", "#define MAX_TEMP_ENTITIES 256 //johnfitz -- was 64", "#define
MAX_STATIC_ENTITIES 4096 //ericw -- was 512 //johnfitz -- was 128" and "#define MAX_VISEDICTS
4096 // larger, now we support BSP2".

A deliberate divergence from id's client: the 2021 rerelease's maps exceed id's limits (the
start map of Dimension of the Machine, `mg1/maps/hub.bsp`, holds more static entities than 128,
and id's client stops it with `Too many static entities`), and the engines players run them on
are of the FitzQuake lineage. id's limits are kept as a mode, as the BSP loader keeps id's
(ADR-0012).

### Behaviour

- **CLIENT-8.1** By default the client keeps QuakeSpasm's limits in place of id's: 4096
  static entities (CLIENT-1.3), 4096 visible entities (CLIENT-5.6), 64 dynamic lights
  (CLIENT-5.7), 32 beams (CLIENT-6.2) and 256 temporary entities (CLIENT-6.4); past each, it
  does what those statements say it does past id's. A new map keeps the limits.
- **CLIENT-8.2** A caller that asks for id's limits (`ClientLimits::WINQUAKE`) gets them, the
  constants of CLIENT-1 to CLIENT-6, as those statements say: a compatibility mode a caller
  chooses, as ADR-0012 keeps `BspLoader::WinQuake`.
