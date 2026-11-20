# Server messages

Prefix: `NET` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes what the server sends a client and when: the writers and buffers, the connection
and signon sequence with the entity baselines, each frame's datagram with its time, client
data and entity updates, and the reliable messages, as implemented by `WinQuake/sv_main.c`,
`host_cmd.c`, `sv_user.c`, `host.c` and `common.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. Implemented by `qs-sim`, whose messages the
preview host's client reads as [`client.md`](client.md) says (P3-26, #240); the bytes the
builtins write are [`../builtins.md`](../builtins.md) BUILTIN-11 to BUILTIN-13.

The preview host's server and client share one process and the messages never cross a
socket (ADR-0004); they are still built byte for byte as id builds them, so that the client
knows exactly what id's client knows, and so that a recorded message stream from id's engine
compares with ours. `sv.time` is a `double`; every edict field is a `float`.

## NET-1 Writers, buffers and the frame

**Source:** `WinQuake/common.c`, `MSG_WriteChar` to `MSG_WriteAngle`, `SZ_GetSpace`,
`SZ_Write`; `WinQuake/server.h`; `WinQuake/host.c`, `Host_ServerFrame`;
`WinQuake/sv_main.c`, `SV_SendClientMessages`, `SV_SendClientDatagram`,
`SV_UpdateToReliableMessages`, `SV_SendNop`, `SV_CleanupEnts`.

### Behaviour

- **NET-1.1** A message is written as BUILTIN-13.2 says: a char or byte is the low 8 bits of
  the int, a short the low 16 little-endian, a long all 32, a coord the short `(int) (f * 8)`,
  an angle the byte `((int) f * 256 / 360) & 255`, a string its bytes and a NUL. A float is
  its four bytes little-endian (`MSG_WriteFloat`) and a null string pointer is one NUL. A
  float field written as a byte, short or char is converted to an int first, truncating.
- **NET-1.2** The server keeps the unreliable datagram `sv.datagram` and the reliable
  datagram `sv.reliable_datagram`, `NET_MAX_DATAGRAM` bytes each, the signon buffer
  `sv.signon`, `NET_SIGNON_SIZE` bytes, and for each client slot its reliable message,
  `NET_MAX_MSGLEN` bytes, which may overflow (BUILTIN-13.3, BUILTIN-13.4). A server frame
  (`Host_ServerFrame`) sets the `frametime` global, clears `sv.datagram`, admits new
  clients, runs each client's move (PHYS-9), runs the physics (PHYS-1) unless the server is
  paused, and then sends the clients' messages (NET-1.3).
- **NET-1.3** `SV_SendClientMessages` first updates the reliable messages (NET-1.4), then for
  each active client: a spawned one is sent a datagram (NET-1.5); one not yet spawned is sent
  its message only when a signon stage asked for it (`sendsignon`), and otherwise a datagram
  of one `svc_nop` once `NET_NOP_SECONDS` have passed since its last message; an overflowed
  message drops the client and clears `overflowed` (the preview host keeps its one client:
  the message is cleared and the flag reset); a message with any bytes is then sent and
  cleared, and
  `sendsignon` cleared. Last, `SV_CleanupEnts` clears `EF_MUZZLEFLASH` (2) from every edict's
  `effects`, so a muzzle flash is sent in one datagram only.
- **NET-1.4** `SV_UpdateToReliableMessages`: for each client slot `i` whose edict's `frags`
  differs from the slot's `old_frags`, every active client's message gets `svc_updatefrags`
  (14), the byte `i` and the frags as a short, and `old_frags` takes the value. Then
  `sv.reliable_datagram` is appended to every active client's message and cleared.
- **NET-1.5** `SV_SendClientDatagram` builds a datagram of `NET_MAX_DATAGRAM` bytes:
  `svc_time` (7) and `sv.time` as a float, the client data (NET-4), the entities (NET-3), and
  then `sv.datagram` only when `cursize + sv.datagram.cursize < maxsize` (otherwise none of
  it: the frame's sounds, particles and broadcast writes are lost). It is sent unreliably; a
  send that fails drops the client.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `NET_MAX_DATAGRAM` | 1024 | `quakedef.h`, `MAX_DATAGRAM` |
| `NET_MAX_MSGLEN` | 8000 | `quakedef.h`, `MAX_MSGLEN` |
| `NET_SIGNON_SIZE` | 8192 | `server.h`, `signon_buf[8192]` |
| `NET_NOP_SECONDS` | 5 | `sv_main.c`, `SV_SendClientMessages` |

## NET-2 Connecting and the signon sequence

**Source:** `WinQuake/sv_main.c`, `SV_SpawnServer`, `SV_CreateBaseline`, `SV_ModelIndex`,
`SV_SendServerinfo`, `SV_ConnectClient`; `WinQuake/host_cmd.c`, `Host_PreSpawn_f`,
`Host_Spawn_f`, `Host_Begin_f`; `WinQuake/quakedef.h`, the `STAT_` numbers.

### Behaviour

- **NET-2.1** `SV_CreateBaseline`, after the settle frames (EDICT-8.9): every edict from 0
  that is in use and is either a client slot (1 to `maxclients`) or has a `modelindex` other
  than 0 gets a baseline: its `origin`, `angles`, `frame` and `skin`; a client's `colormap`
  is its number and its `modelindex` that of `progs/player.mdl`, any other's `colormap` is 0
  and its `modelindex` that of its `model` text (`SV_ModelIndex`: 0 for an empty text, and
  the fatal error `SV_ModelIndex: model <name> not precached` for a text not in the model
  precache list). Each is appended to the signon buffer: `svc_spawnbaseline` (22), the
  number as a short, the modelindex, frame, colormap and skin as bytes, then for x, y and z
  in turn the origin's coordinate and the angles' angle.
- **NET-2.2** `SV_SendServerinfo`, when a client connects, appends to its message:
  `svc_print` (8) with the text `\x02\nVERSION 1.09 SERVER (<crc> CRC)` (`%4.2f` of
  `VERSION` and the progs CRC as `%i`); `svc_serverinfo` (11), the long
  `NET_PROTOCOL_VERSION`, `maxclients` as a byte, the byte `NET_GAME_DEATHMATCH` when
  `deathmatch` is set and `coop` is not, else `NET_GAME_COOP`, the world's `message` text
  (id passes it through `sprintf` as a format string, so a `%` in it is undefined; the
  preview host writes the text as it is), each model precache name from entry 1 with its NUL
  then a 0 byte, each sound precache name from entry 1 the same way; `svc_cdtrack` (32) with
  the world's `sounds` as a byte twice (the track and the loop track); `svc_setview` (5) with
  the client's edict number as a short; and `svc_signonnum` (25) with 1. The slot is marked
  `sendsignon` and not spawned.
- **NET-2.3** The `prespawn` command appends the signon buffer to the client's message, then
  `svc_signonnum` 2, and marks `sendsignon`.
- **NET-2.4** The `spawn` command brings the client in (PHYS-10.1, unless a game is being
  loaded), then clears its message (`SZ_Clear`, so what `ClientConnect` and
  `PutClientInServer` wrote to the client is dropped) and appends: `svc_time` (7) and `sv.time` as a float; for each
  client slot `i` from 0, `svc_updatename` (13), the byte `i` and the slot's name,
  `svc_updatefrags` (14), `i` and its `old_frags` as a short, and `svc_updatecolors` (17),
  `i` and its colours byte; for each of the `MAX_LIGHTSTYLES` (64) styles `svc_lightstyle`
  (12), the index as a byte and the style's string (one NUL for a style never set);
  `svc_updatestat` (3) four times, each a stat byte and a long: `STAT_TOTALSECRETS` with the
  `total_secrets` global, `STAT_TOTALMONSTERS` with `total_monsters`, `STAT_SECRETS` with
  `found_secrets` and `STAT_MONSTERS` with `killed_monsters`, each converted to an int;
  `svc_setangle` (10) with the client edict's `angles[0]` and `angles[1]` as angles and a
  third angle of 0; the client data (NET-4); and `svc_signonnum` 3, marking `sendsignon`.
- **NET-2.5** The `begin` command marks the client spawned, so from the next frame on it is
  sent datagrams (NET-1.3). The preview host's client is local and answers each signon at
  once, so when the player enters (`World::connect`) the serverinfo, the prespawn, spawn and
  begin messages are written in that order in one reliable message.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `NET_PROTOCOL_VERSION` | 15 | `protocol.h`, `PROTOCOL_VERSION` |
| `NET_VERSION` | `1.09` | `quakedef.h`, `VERSION` |
| `NET_GAME_COOP` | 0 | `quakedef.h`, `GAME_COOP` |
| `NET_GAME_DEATHMATCH` | 1 | `quakedef.h`, `GAME_DEATHMATCH` |
| `NET_STAT_TOTALSECRETS` | 11 | `quakedef.h`, `STAT_TOTALSECRETS` |
| `NET_STAT_TOTALMONSTERS` | 12 | `quakedef.h`, `STAT_TOTALMONSTERS` |
| `NET_STAT_SECRETS` | 13 | `quakedef.h`, `STAT_SECRETS` |
| `NET_STAT_MONSTERS` | 14 | `quakedef.h`, `STAT_MONSTERS` |

## NET-3 Entity updates

**Source:** `WinQuake/sv_main.c`, `SV_FatPVS`, `SV_AddToFatPVS`, `SV_WriteEntitiesToClient`;
`WinQuake/protocol.h`, the `U_` bits.

### Behaviour

- **NET-3.1** `SV_FatPVS (org)`, with `org` the client's `origin + view_ofs`, is the OR of the
  PVS (BSP-6) of every leaf other than a solid one that the walk reaches: from the world's
  root node, with `d` the point's distance from the node's plane, only the front child is
  visited when `d > NET_FAT_PVS_DISTANCE`, only the back child when `d < -8`, and both
  otherwise. The result has `(numleafs + 31) >> 3` bytes, bit `i` for leaf `i + 1`.
- **NET-3.2** `SV_WriteEntitiesToClient` walks every edict from 1 below `num_edicts`. The
  client's own edict is always sent; any other is skipped when its `modelindex` is 0 or its
  `model` text is empty, or when none of the leafs it is linked in (WORLD-6, so none for an
  edict not linked) has its bit in the fat PVS. Before each update, fewer than
  `NET_PACKET_SLACK` bytes left in the datagram prints the console message `packet overflow`
  and ends the entities.
- **NET-3.3** The update's bits: `U_ORIGIN1 << i` (bits 1 to 3) when `origin[i] -
  baseline.origin[i]` is below -0.1 or above 0.1; `U_ANGLE1` (8), `U_ANGLE2` (4) and
  `U_ANGLE3` (9) when the angle differs from the baseline's exactly; `U_NOLERP` (5) when
  `movetype` is `MOVETYPE_STEP`; `U_COLORMAP` (11), `U_SKIN` (12), `U_FRAME` (6),
  `U_EFFECTS` (13) and `U_MODEL` (10) when the field differs from the baseline's int value
  (compared as floats); `U_LONGENTITY` (14) when the number is 256 or more; `U_MOREBITS`
  (0) when the bits reach 256. Then the message: the byte `bits | U_SIGNAL` (128), the byte
  `bits >> 8` under `U_MOREBITS`, the number as a short under `U_LONGENTITY` or a byte
  otherwise, and under their bits `modelindex`, `frame`, `colormap`, `skin` and `effects` as
  bytes, then for x, y and z in turn the origin's coordinate and the angles' angle under
  their bits.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `NET_FAT_PVS_DISTANCE` | 8 | `sv_main.c`, `SV_AddToFatPVS` |
| `NET_PACKET_SLACK` | 16 | `sv_main.c`, `SV_WriteEntitiesToClient` |
| `NET_ORIGIN_EPSILON` | 0.1 | `sv_main.c`, `SV_WriteEntitiesToClient` |

## NET-4 Client data

**Source:** `WinQuake/sv_main.c`, `SV_WriteClientdataToMessage`; `WinQuake/sv_user.c`,
`SV_SetIdealPitch`; `WinQuake/protocol.h`, the `SU_` bits; `WinQuake/common.c`,
`standard_quake`.

### Behaviour

- **NET-4.1** When the client edict's `dmg_take` or `dmg_save` is not 0: `svc_damage` (19),
  `dmg_save` and `dmg_take` as bytes, and three coordinates of the inflictor's (`dmg_inflictor`
  through `PROG_TO_EDICT`) `origin + 0.5 * (mins + maxs)`; both fields are then 0.
- **NET-4.2** `SV_SetIdealPitch` then runs: nothing while the player is not on the ground
  (`FL_ONGROUND`). Otherwise, with `yaw` the player's `angles[YAW]` in radians, for `i` from
  0 to `NET_MAX_FORWARD - 1`: `top = origin + (cos yaw, sin yaw) * ((i + 3) * 12)` on x and y
  and `origin.z + view_ofs.z`, `bottom` is `top` less 160 on z, and a point trace from `top`
  to `bottom` ignoring monsters (WORLD-7, `MOVE_NOMONSTERS`, passing the player) that is
  all solid ("looking at a wall") or has fraction 1 ("near a dropoff") leaves `idealpitch` as
  it is; else `z[i] = top.z + fraction * (bottom.z - top.z)`. Then, with `dir` and `steps`
  0, for `j` from 1: `step = z[j] - z[j - 1]` as an int; 0 is skipped (`step` within
  `ON_EPSILON` of 0 as an int); a `step` other than `dir` when `dir` is set leaves
  `idealpitch` ("mixed changes"); else `steps` grows and `dir = step`. A `dir` of 0 sets
  `idealpitch` 0; fewer than 2 steps leave it; otherwise `idealpitch = -dir *
  NET_IDEALPITCH_SCALE`.
- **NET-4.3** While the edict's `fixangle` is set: `svc_setangle` (10) with its three
  `angles` as angles, and `fixangle` becomes 0 (PHYS-10.2).
- **NET-4.4** The bits: `SU_VIEWHEIGHT` (0) when `view_ofs[2]` is not
  `CLIENT_DEFAULT_VIEWHEIGHT`; `SU_IDEALPITCH` (1) when `idealpitch` is not 0; `SU_ITEMS`
  (9) always; `SU_ONGROUND` (10) when `flags` has `FL_ONGROUND`; `SU_INWATER` (11) when
  `waterlevel` is 2 or more; `SU_PUNCH1 << i` (2 to 4) when `punchangle[i]` is not 0 and
  `SU_VELOCITY1 << i` (5 to 7) when `velocity[i]` is not 0; `SU_WEAPONFRAME` (12) when
  `weaponframe` is not 0; `SU_ARMOR` (13) when `armorvalue` is not 0; `SU_WEAPON` (14)
  always. Then `svc_clientdata` (15), the bits as a short, and under their bits:
  `view_ofs[2]` as a char, `idealpitch` as a char, for each axis `punchangle[i]` as a char
  and `velocity[i] / 16` as a char; always the items as a long (NET-4.5); under their bits
  `weaponframe` and `armorvalue` as bytes and the model precache index of `weaponmodel`
  (`SV_ModelIndex`, NET-2.1) as a byte; then always `health` as a short, `currentammo`,
  `ammo_shells`, `ammo_nails`, `ammo_rockets` and `ammo_cells` as bytes, and `weapon` as a
  byte in standard Quake, or, under `-rogue` or `-hipnotic`, the index of the lowest set bit
  of `(int) weapon` as a byte, and nothing when none is set.
- **NET-4.5** The items long is `(int) items | ((int) items2 << 23)` when the progs defines
  the field `items2` (EDICT-3.4), else `(int) items | ((int) serverflags << 28)`.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `NET_MAX_FORWARD` | 6 | `sv_user.c`, `MAX_FORWARD` |
| `NET_IDEALPITCH_SCALE` | 0.8 | `sv_user.c`, `sv_idealpitchscale` default |
| `NET_ON_EPSILON` | 0.1 | `quakedef.h`, `ON_EPSILON` |

## NET-5 The builtins' bytes

**Source:** `WinQuake/pr_cmds.c`, `PF_bprint`, `PF_sprint`, `PF_centerprint`, `PF_stuffcmd`,
`PF_sound`, `PF_ambientsound`, `PF_particle`, `PF_lightstyle`, `PF_makestatic`, `PF_WriteByte`
to `PF_WriteEntity`; `WinQuake/sv_main.c`, `SV_StartSound`, `SV_StartParticle`,
`SV_BroadcastPrintf`.

### Behaviour

- **NET-5.1** The bytes each builtin writes, and to which buffer, are BUILTIN-11.1 to
  BUILTIN-11.4, BUILTIN-12.3 to BUILTIN-12.5, BUILTIN-12.7, BUILTIN-12.8 and BUILTIN-13.1 to
  BUILTIN-13.2. `qs-sim` writes them into the buffers of NET-1.2 beside the mock engine's
  byte counts (DIFF-6.5), and the bytes it holds equal the counts the mock keeps.

## NET-6 FitzQuake's 666 and RMQ's 999

**Source:** QuakeSpasm (`sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`):
`Quake/protocol.h`; `Quake/common.c`, `MSG_WriteCoord`, `MSG_WriteAngle`, `MSG_WriteAngle16`
and their readers; `Quake/sv_main.c`, `SV_Protocol_f`, `SV_StartSound`, `SV_LocalSound`,
`SV_SendServerinfo`, `SV_WriteEntitiesToClient`, `SV_WriteClientdataToMessage`,
`SV_CreateBaseline`, `SV_SpawnServer`; `Quake/pr_cmds.c`, `PF_ambientsound`, `PF_makestatic`;
`Quake/pr_edict.c`, `ED_Free`, `ED_ParseEdict`, `PR_LoadProgs`. Adopted under
[ADR-0026](../../adr/0026-preview-host-later-engine-improvements.md): the server writes its
messages in FitzQuake's protocol by default, as QuakeSpasm does, with id's 15 one option away.

The protocol is how the server's messages are written, not what the server simulates: every
edict and global stays as NET-1 to NET-5 and the PHYS statements say, whichever protocol is
written, so `cargo xtask sim-diff` holds `qs-sim` to id's server frame by frame in any of the
three (DIFF-16).

### Behaviour

- **NET-6.1** The server speaks one protocol, QuakeSpasm's `sv_protocol`: "`int sv_protocol =
  PROTOCOL_FITZQUAKE;`", 666, by default, or `PROTOCOL_NETQUAKE` 15 or `PROTOCOL_RMQ` 999.
  `SV_SpawnServer` gives 999 the flags "`sv.protocolflags = PRFL_INT32COORD |
  PRFL_SHORTANGLE;`" and the others none. `SV_SendServerinfo` writes the protocol's number in
  place of NET-2.2's 15, and for 999 its flags as a long after it; under 15 it sends the model
  and sound names "`if (sv.protocol != PROTOCOL_NETQUAKE || i < 256)`", so at most the first
  255 of each. The preview host chooses the protocol with `--protocol` (15, 666 or 999) before
  the map loads, so its baselines (NET-2.1) are written in it. QuakeSpasm prints "`FITZQUAKE
  %1.2f SERVER (%i CRC)`" where id prints NET-2.2's version line; the host keeps id's line,
  since it is not FitzQuake.
- **NET-6.2** Under 15 a coordinate and an angle are NET-1.1's. Under 666 and 999 they are
  QuakeSpasm's `MSG_WriteCoord (sb, f, flags)`: a float with `PRFL_FLOATCOORD`, "`MSG_WriteLong
  (sb, Q_rint (f * 16))`" with `PRFL_INT32COORD`, `MSG_WriteCoord24`'s "`MSG_WriteShort (sb,
  f); MSG_WriteByte (sb, (int)(f*255)%255);`" with `PRFL_24BITCOORD`, else `MSG_WriteCoord16`'s
  "`MSG_WriteShort (sb, Q_rint(f*8));`"; and `MSG_WriteAngle (sb, f, flags)`: a float with
  `PRFL_FLOATANGLE`, "`MSG_WriteShort (sb, Q_rint(f * 65536.0 / 360.0) & 65535)`" with
  `PRFL_SHORTANGLE`, else "`MSG_WriteByte (sb, Q_rint(f * 256.0 / 360.0) & 255)`". `Q_rint (x)`
  is "`((x) > 0 ? (int)((x) + 0.5) : (int)((x) - 0.5))`", the products with `65536.0` and
  `256.0` in double. `MSG_WriteAngle16` is a float with `PRFL_FLOATANGLE`, else the short. Every
  buffer of NET-1.2, and each message built from them, writes in the server's protocol, the
  builtins' `WriteCoord` and `WriteAngle` (BUILTIN-13.2) included. A deliberate divergence:
  QuakeSpasm's client sends its move's angles with `MSG_WriteAngle16` under 666 and its
  `SV_ReadClientMove` reads them so; the host's moves arrive as PHYS-9.6 says in every
  protocol, since they are what the simulation reads and ENG-15 holds the simulation to id's.
- **NET-6.3** Under 666 and 999 `SV_WriteEntitiesToClient` changes NET-3.2 and NET-3.3: the
  updates stop when fewer than 40 bytes are left ("For float coords and angles the limit is
  40") where 15 stops at NET-3.2's 16; once the bits of NET-3.3 are made, the entity's alpha and
  scale are read (NET-6.8), an entity whose alpha is `ENTALPHA_ZERO` with `effects` 0 is not
  sent ("don't send invisible entities unless they have effects"), and the bits gain
  `U_ALPHA` (1<<16) when the alpha differs from the baseline's, `U_SCALE` (1<<20) when the scale
  does, `U_FRAME2` (1<<17) with `U_FRAME` and a frame past a byte (`& 0xFF00`), `U_MODEL2`
  (1<<18) with `U_MODEL` and a model past a byte, then `U_EXTEND1` (1<<15) for "`bits >=
  65536`" and `U_EXTEND2` (1<<23) for "`bits >= 16777216`". The third and fourth bytes of bits
  follow the second, under their extend bits, and after the angles come the alpha byte, the
  scale byte, the frame's and the model's high bytes (`>> 8`), each under its bit. Under 15
  an entity whose model is past a byte is not sent ("don't send model>255 entities if
  protocol is 15"), and none of these bits is set. QuakeSpasm masks `effects` with
  `pr_effects_mask` for the rerelease's lights; the host sends `effects` as NET-3.3 does.
- **NET-6.4** Under 666 and 999 `SV_CreateBaseline` keeps a baseline alpha, the edict's (NET-6.8)
  or `ENTALPHA_DEFAULT` for a client slot, and a baseline scale, `ENTSCALE_DEFAULT` or under 999
  the edict's `scale` field's, and decides "which extra data needs to be sent": `B_LARGEMODEL`
  (1<<0) and `B_LARGEFRAME` (1<<1) for a model or frame past a byte, `B_ALPHA` (1<<2) and
  `B_SCALE` (1<<3) for a value that is not the default. With no bits the baseline is NET-2.1's
  `svc_spawnbaseline`; with any it is `svc_spawnbaseline2` (42), the edict's number as a short,
  the bits as a byte, the model and the frame each a short under its large bit and else a
  byte, the colormap, the skin, the origins and angles as NET-2.1, then the alpha and the
  scale bytes under their bits. Under 15 a model or frame past a byte is kept as 0, the alpha
  and scale as the defaults ("still want to send baseline in PROTOCOL_NETQUAKE, so reset these
  values"), and the baseline is NET-2.1's.
- **NET-6.5** `PF_makestatic` under 666 and 999 writes nothing for an edict whose alpha is
  `ENTALPHA_ZERO` ("don't send invisible static entities") and otherwise takes NET-6.4's bits,
  `B_SCALE` only under 999: with none it is BUILTIN-12.8's `svc_spawnstatic`, with any
  `svc_spawnstatic2` (43) and the bits, the model and frame sized by them, then the alpha and
  scale bytes under their bits. Under 15 a static whose model or frame is past a byte writes
  nothing ("can't display the correct model & frame, so don't show it at all"). The edict is
  freed either way. `PF_ambientsound` with a sound past 255 writes `svc_spawnstaticsound2`
  (44) with the sound as a short under 666 and 999, and nothing under 15.
- **NET-6.6** Under 666 and 999 `SV_WriteClientdataToMessage` adds to NET-4.4's bits:
  `SU_WEAPON2` (1<<16) for a weapon model past a byte, `SU_ARMOR2`, `SU_AMMO2`, `SU_SHELLS2`,
  `SU_NAILS2`, `SU_ROCKETS2` and `SU_CELLS2` (1<<17 to 1<<22) for `armorvalue`,
  `currentammo` and the four ammo counts past a byte, `SU_WEAPONFRAME2` (1<<24) with
  `SU_WEAPONFRAME` and a frame past a byte, `SU_WEAPONALPHA` (1<<25) when the client's alpha is
  not the default ("for now, weaponalpha = client entity alpha"), then `SU_EXTEND1` (1<<15)
  and `SU_EXTEND2` (1<<23) as NET-6.3's. The third and fourth bytes of bits follow the short;
  after the weapon come the high bytes in that order and the alpha byte, each under its bit.
- **NET-6.7** `SV_StartSound` under 666 and 999 sets `SND_LARGEENTITY` (1<<3) for an entity of
  8192 or more and `SND_LARGESOUND` (1<<4) for a sound past 255 or a channel of 8 or more, and
  under 15 writes nothing for either ("don't send any info protocol can't support"); with
  `SND_LARGEENTITY` the entity is a short and the channel a byte where BUILTIN-12.3 packs
  them in one short, and with `SND_LARGESOUND` the sound is a short. `SV_LocalSound`
  (RRE-2.1) sends a sound past 255 as `SND_LARGESOUND` and a short under 666 and 999.
- **NET-6.8** An edict's alpha is QuakeSpasm's `ent->alpha`, a byte beside its fields:
  `ENTALPHA_ENCODE(a)`, "`(((a)==0)?ENTALPHA_DEFAULT:Q_rint(CLAMP(1.0f,(a)*254.0f+1,255.0f)))`",
  `ENTALPHA_DEFAULT` 0 and `ENTALPHA_ZERO` 1. `ED_ParseEdict` sets it from an `alpha` key, "hack
  to support .alpha even when progs.dat doesn't know about it", with `Q_atof`; when the progs
  has an `alpha` field (`pr_alpha_supported`), `SV_WriteEntitiesToClient` sets it from the field
  each time it reaches the edict; `ED_Free` resets it ("reset alpha for next entity"); and
  `ED_Write` saves it when the progs has no `alpha` field, "`fprintf (f, "\"alpha\" \"%f\"\n",
  ENTALPHA_TOSAVE(ed->alpha))`", `ENTALPHA_TOSAVE` 0 for the default, -1 for `ENTALPHA_ZERO` and
  `(a - 1) / 254` else, which the load reads back by the same key. The
  scale is `ENTSCALE_ENCODE` of the progs' `scale` field, "`((a) ? ((a) * ENTSCALE_DEFAULT) :
  ENTSCALE_DEFAULT)`" with `ENTSCALE_DEFAULT` 16, kept in an `unsigned char`: the host keeps
  the low byte of the product converted to an int, as x86 does, which C leaves undefined past
  255; `ENTSCALE_DEFAULT` when the progs has no `scale` field. The host's `Q_atof` is id's
  (BUILTIN-11.7), which skips no leading space where QuakeSpasm's does.
- **NET-6.9** QuakeSpasm's "Fix viewmodel interpolation with >10Hz animations" (`Quakespasm.txt`;
  commit `e132da234ba8e9a77e1dcb7f518ace39bec6521d`): `SV_RunThink` keeps "`ent->oldthinktime =
  thinktime; ent->oldframe = ent->v.frame;`", the think's time after "don't let things stay
  in the past", and `SV_Physics`, after each entity's physics, sets `sendinterval` when "`!ent->free
  && ent->v.nextthink > sv.time && (ent->v.movetype == MOVETYPE_STEP || ent->v.movetype ==
  MOVETYPE_WALK || ent->v.frame != ent->oldframe)`" and `j = Q_rint((ent->v.nextthink -
  ent->oldthinktime)*255)` is from 0 to 255 and neither 25 nor 26 ("25 and 26 are close enough
  to 0.1 to not send"), `sv.time` the frame's start. Under 666 and 999 an entity with
  `sendinterval` has `U_LERPFINISH` (1<<19) in its update, and its byte, after the model's high
  byte, is "`(byte)(Q_rint((ent->v.nextthink-sv.time)*255))`", `sv.time` then the frame's end.
  Under 15 the bit is never sent. None of it changes an edict (the marks are kept beside the
  fields), so the simulation is NET-1 to NET-5's.
