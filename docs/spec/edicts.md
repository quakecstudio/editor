# Edicts, system globals and fields

Prefix: `EDICT` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the server's entities (edicts) as the engine lays them out, allocates, frees and
spawns them, and the system globals and fields that the engine and QuakeC share: where each
one is, who writes it and when. Source: [id-Software/Quake](https://github.com/id-Software/Quake)
at commit `bf4ac424ce754894ac8f1dae6a3981954bc9852d`, the WinQuake tree built without
`QUAKE2`, `GLQUAKE` or `IDGODS` (the shipped `progdefs.h` is `progdefs.q1`). Files are
`WinQuake/pr_edict.c`, `progs.h`, `progdefs.q1`, `server.h`, `sv_main.c`, `sv_phys.c`,
`sv_user.c`, `world.c`, `host.c`, `host_cmd.c` and `pr_cmds.c` unless a statement names
another.

Implemented by `qc-edict` (P1-09). The physics that moves entities (`sv_phys.c`, `sv_move.c`,
`world.c`) belongs to the preview host's simulation (`qs-sim`, P3-24) and is described here only
as far as it writes system fields and calls QuakeC. Opcode semantics, including `OP_STATE` and
the store through `OP_ADDRESS`, are in [`vm.md`](vm.md); builtins are in `builtins.md` (P1-04);
the file layout of globals and fields is in [`progs.md`](progs.md); limits are in
[`limits.md`](limits.md). Saved games (`ED_Write`, `ED_ParseGlobals`) are not covered. Every
statement in this file is in [`coverage-baseline.tsv`](coverage-baseline.tsv) until `qc-edict`
cites it.

Times are seconds. `sv.time` is the server clock, a C `double`; the `time` global and every
float field are `f32`, so storing the clock in them rounds it to the nearest `f32`.

## EDICT-1 Entity memory and numbering

**Source:** `WinQuake/progs.h`, `edict_t`, `EDICT_TO_PROG`, `PROG_TO_EDICT`, `NEXT_EDICT`;
`WinQuake/pr_edict.c`, `PR_LoadProgs`, `EDICT_NUM`, `NUM_FOR_EDICT`; `WinQuake/sv_main.c`,
`SV_SpawnServer`; `WinQuake/zone.c`, `Hunk_AllocName`.

The engine keeps every edict in one array. Each edict is a fixed engine header followed by
the progs' field slots, and QuakeC refers to an edict by its byte offset in that array.

### Behaviour

- **EDICT-1.1** An edict is `EDICT_HEADER_SIZE` (96) bytes of engine state followed by
  `entityfields` 4-byte field slots (PROGS-13.1). The edict size is
  `96 + 4 * entityfields` bytes; for id 1.01's 105 field slots it is 516.
- **EDICT-1.2** The header holds, in order: the free flag, the area links, the BSP leaf count
  and up to 16 leaf numbers (`LIMIT_MAX_ENT_LEAFS`), the network baseline, and `freetime`, the
  server time the edict was last freed (an `f32`). QuakeC cannot address the header: field
  slot 0 is the first byte after it.
- **EDICT-1.3** id computes the header size as `sizeof(edict_t) - sizeof(entvars_t)`, which
  depends on the build. This spec fixes 96, the size on the 32-bit x86 build id shipped; a
  64-bit build of the same code has 108. The header size is visible to QuakeC only through
  entity and pointer values (EDICT-1.5, EDICT-1.6), so a comparison with an oracle built for
  64 bits compares those values as edict numbers and field slots; the trace header records
  each VM's edict size and field offset for this (TRACE-2.1).
- **EDICT-1.4** The array has `LIMIT_MAX_EDICTS` (600) edicts, numbered from 0. Edict 0 is
  the world (EDICT-4) and edicts 1 to `maxclients` are the client slots (EDICT-5).
  `num_edicts` starts at `maxclients + 1` (EDICT-1.8) and grows by one each time allocation
  takes a new edict (EDICT-6.2); it never shrinks, and edicts at or above it have not been
  used on this map.
- **EDICT-1.5** An entity value is the edict's number times the edict size
  (PROGS-17.4): with 105 field slots, edict 3 is 1548. Edict 0 is 0, so the world is the
  "null" entity.
- **EDICT-1.6** A pointer value made by `OP_ADDRESS` for field slot `f` of edict `n` is
  `n * edict size + 96 + 4 * f` (PROGS-17.7).
- **EDICT-1.7** Turning a number into an edict fails with `EDICT_NUM: bad number <n>` when the
  number is negative or not less than 600. Turning an edict back into a number fails with
  `NUM_FOR_EDICT: bad pointer` when the number is negative or not less than `num_edicts`.
  An entity value is not checked when QuakeC uses it (`PROG_TO_EDICT` only adds); what one
  that names no edict (EDICT-1.9) does is in VM-11.4, VM-12.5 and BUILTIN-2.7.
- **EDICT-1.8** Every map gets a new array: `SV_SpawnServer` reloads the progs, which resets
  every global to its value in the file (PROGS-12.1), and allocates the edicts zero-filled,
  so every field of every edict is 0 and no edict is free until the map's entities are
  spawned (EDICT-8). `num_edicts` starts at `maxclients + 1`.
- **EDICT-1.9** An entity value **names an edict** when it is `n` times the edict size for a
  whole number `n` from 0 to `LIMIT_MAX_EDICTS - 1` (599); it names edict `n`. It does so
  whether or not `n` is below `num_edicts` and whether or not edict `n` is free: the array
  holds all 600 edicts (EDICT-1.4), so id's code reads and writes such an edict as any other.
  Any other value, negative, not a multiple of the edict size, or 600 edicts or more, names
  none: id's `PROG_TO_EDICT` (`(edict_t *)((byte *)sv.edicts + e)`) then gives an address that
  is not the start of an edict in the array, so what it reaches depends on the header size
  (EDICT-1.3) or lies outside the array. The edict size is the VM's own (TRACE-2.1), so the
  same edict number is a different value in a 32-bit and a 64-bit build.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `EDICT_HEADER_SIZE` | 96 | `WinQuake/pr_edict.c`, `PR_LoadProgs`: `pr_edict_size = progs->entityfields * 4 + sizeof (edict_t) - sizeof(entvars_t)`, for `edict_t` in `WinQuake/progs.h` on 32-bit x86 |

`LIMIT_MAX_EDICTS` and `LIMIT_MAX_ENT_LEAFS` are in [`limits.md`](limits.md).

## EDICT-2 System globals and `end_sys_globals`

**Source:** `WinQuake/progdefs.q1`, `globalvars_t`, `PROGHEADER_CRC`;
`WinQuake/pr_edict.c`, `PR_LoadProgs`; `WinQuake/progs.h`, `pr_global_struct`.

The engine reads and writes the system globals as members of a C struct laid over the
globals, so each one is at a fixed slot. It never looks them up by name.

### Behaviour

- **EDICT-2.1** The engine's system globals are the 64 slots from 28 (`PROGS_RESERVED_OFS`) to
  91, at these slots, with these types:

  | Slot | Global | Type | Slot | Global | Type |
  | --- | --- | --- | --- | --- | --- |
  | 28 | `self` | entity | 59 | `v_forward` | vector |
  | 29 | `other` | entity | 62 | `v_up` | vector |
  | 30 | `world` | entity | 65 | `v_right` | vector |
  | 31 | `time` | float | 68 | `trace_allsolid` | float |
  | 32 | `frametime` | float | 69 | `trace_startsolid` | float |
  | 33 | `force_retouch` | float | 70 | `trace_fraction` | float |
  | 34 | `mapname` | string | 71 | `trace_endpos` | vector |
  | 35 | `deathmatch` | float | 74 | `trace_plane_normal` | vector |
  | 36 | `coop` | float | 77 | `trace_plane_dist` | float |
  | 37 | `teamplay` | float | 78 | `trace_ent` | entity |
  | 38 | `serverflags` | float | 79 | `trace_inopen` | float |
  | 39 | `total_secrets` | float | 80 | `trace_inwater` | float |
  | 40 | `total_monsters` | float | 81 | `msg_entity` | entity |
  | 41 | `found_secrets` | float | 82 | `main` | function |
  | 42 | `killed_monsters` | float | 83 | `StartFrame` | function |
  | 43 to 58 | `parm1` to `parm16` | float | 84 | `PlayerPreThink` | function |
  | | | | 85 | `PlayerPostThink` | function |
  | | | | 86 | `ClientKill` | function |
  | | | | 87 | `ClientConnect` | function |
  | | | | 88 | `PutClientInServer` | function |
  | | | | 89 | `ClientDisconnect` | function |
  | | | | 90 | `SetNewParms` | function |
  | | | | 91 | `SetChangeParms` | function |

- **EDICT-2.2** The only check that a progs puts these globals at these slots is the header
  CRC (PROGS-3.2, PROGS-5): qcc's `progdefs.h` lists, in order, every global declared before
  the one named `end_sys_globals`, so a progs whose CRC is 5927 declared exactly these names
  and types in this order. `end_sys_globals` itself is not a system global, and the engine
  does not use its slot.
- **EDICT-2.3** When the CRC check is skipped (PROGS-15.8), the system globals are still the
  slots of EDICT-2.1, whatever the progs' definitions say is there, as id's engine would read
  them; they are never resolved by name.
- **EDICT-2.4** A progs with fewer than 92 global slots cannot hold the system globals. id's
  engine would read and write past the end of the globals, which is undefined; this spec
  fixes the answer: such a progs is refused with a typed error when the edict store is set
  up. A progs id's qcc wrote with CRC 5927 always has at least 92.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `EDICT_SYS_GLOBALS_END` | 92 | `WinQuake/progdefs.q1`, `globalvars_t`: `pad[28]` plus 64 slots |
| `EDICT_NUM_SPAWN_PARMS` | 16 | `WinQuake/server.h`, `NUM_SPAWN_PARMS` (`parm1` to `parm16`) |

## EDICT-3 System fields and `end_sys_fields`

**Source:** `WinQuake/progdefs.q1`, `entvars_t`; `WinQuake/progs.h`, `edict_t`;
`WinQuake/pr_edict.c`, `ED_FindField`, `GetEdictFieldValue`.

### Behaviour

- **EDICT-3.1** The engine's system fields are the 105 field slots from 0 to 104, at these
  slots, with these types; a mod's own fields follow from slot 105:

  | Slot | Field | Type | Slot | Field | Type | Slot | Field | Type |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | 0 | `modelindex` | float | 48 | `health` | float | 77 | `colormap` | float |
  | 1 | `absmin` | vector | 49 | `frags` | float | 78 | `team` | float |
  | 4 | `absmax` | vector | 50 | `weapon` | float | 79 | `max_health` | float |
  | 7 | `ltime` | float | 51 | `weaponmodel` | string | 80 | `teleport_time` | float |
  | 8 | `movetype` | float | 52 | `weaponframe` | float | 81 | `armortype` | float |
  | 9 | `solid` | float | 53 | `currentammo` | float | 82 | `armorvalue` | float |
  | 10 | `origin` | vector | 54 | `ammo_shells` | float | 83 | `waterlevel` | float |
  | 13 | `oldorigin` | vector | 55 | `ammo_nails` | float | 84 | `watertype` | float |
  | 16 | `velocity` | vector | 56 | `ammo_rockets` | float | 85 | `ideal_yaw` | float |
  | 19 | `angles` | vector | 57 | `ammo_cells` | float | 86 | `yaw_speed` | float |
  | 22 | `avelocity` | vector | 58 | `items` | float | 87 | `aiment` | entity |
  | 25 | `punchangle` | vector | 59 | `takedamage` | float | 88 | `goalentity` | entity |
  | 28 | `classname` | string | 60 | `chain` | entity | 89 | `spawnflags` | float |
  | 29 | `model` | string | 61 | `deadflag` | float | 90 | `target` | string |
  | 30 | `frame` | float | 62 | `view_ofs` | vector | 91 | `targetname` | string |
  | 31 | `skin` | float | 65 | `button0` | float | 92 | `dmg_take` | float |
  | 32 | `effects` | float | 66 | `button1` | float | 93 | `dmg_save` | float |
  | 33 | `mins` | vector | 67 | `button2` | float | 94 | `dmg_inflictor` | entity |
  | 36 | `maxs` | vector | 68 | `impulse` | float | 95 | `owner` | entity |
  | 39 | `size` | vector | 69 | `fixangle` | float | 96 | `movedir` | vector |
  | 42 | `touch` | function | 70 | `v_angle` | vector | 99 | `message` | string |
  | 43 | `use` | function | 73 | `idealpitch` | float | 100 | `sounds` | float |
  | 44 | `think` | function | 74 | `netname` | string | 101 | `noise` | string |
  | 45 | `blocked` | function | 75 | `enemy` | entity | 102 | `noise1` | string |
  | 46 | `nextthink` | float | 76 | `flags` | float | 103 | `noise2` | string |
  | 47 | `groundentity` | entity | | | | 104 | `noise3` | string |

- **EDICT-3.2** As for globals, only the header CRC ties a progs' field definitions to these
  slots: `progdefs.h` lists every field declared before the one named `end_sys_fields`. The
  engine never looks a system field up by name, and with the CRC check skipped (PROGS-15.8)
  the system fields are still the slots of EDICT-3.1. `end_sys_fields` takes a field slot of
  its own (QC-21.3) that the engine does not use.
- **EDICT-3.3** A progs whose `entityfields` is less than 105 cannot hold the system fields.
  id's engine would write them past the edict's fields into the next edict, which is
  undefined; this spec fixes the answer: such a progs is refused with a typed error when the
  edict store is set up. A progs id's qcc wrote with CRC 5927 always has at least 105.
- **EDICT-3.4** The engine reads these non-system fields by name, and only if the progs defines
  them: `gravity` (scales gravity, `SV_AddGravity`), `items2` (`SV_WriteClientdataToMessage`),
  and the mission-pack ammo fields set by the `give` cheat (`Host_Give_f`). Lookup is by exact
  name over the field definitions, first match (`ED_FindField`); a missing field is skipped, not
  an error.
- **EDICT-3.5** With more than 894760 field slots, `LIMIT_MAX_EDICTS` edicts of EDICT-1.1's
  size take more than 2147483647 bytes, so an entity value (EDICT-1.5), a 32-bit integer, cannot
  name every edict, and id's `PR_LoadProgs` computes `entityfields * 4` as an `int`, which
  overflows for a large enough count, which is undefined. This spec fixes the answer: a progs
  whose `entityfields` is greater than 894760 is refused with a typed error when the edict store
  is set up, whether or not the header CRC was checked. No progs a compiler writes comes near
  it. 894760 is `(2147483647 / 600 - 96) / 4`, each division rounded down.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `EDICT_SYS_FIELDS_END` | 105 | `WinQuake/progdefs.q1`, `entvars_t`: 105 slots |

## EDICT-4 The world entity

**Source:** `WinQuake/sv_main.c`, `SV_SpawnServer`; `WinQuake/pr_edict.c`, `ED_LoadFromFile`,
`ED_ParseEdict`, `ED_Free`; `WinQuake/world.c`, `SV_LinkEdict`; `WinQuake/pr_exec.c`,
`PR_ExecuteProgram` (`OP_ADDRESS`).

### Behaviour

- **EDICT-4.1** Before the map's entities are parsed, the engine zeroes edict 0's fields,
  clears its free flag, and sets `model` to the world model's name (`maps/<map>.bsp`),
  `modelindex` to 1, `solid` to `SOLID_BSP` (4) and `movetype` to `MOVETYPE_PUSH` (7).
- **EDICT-4.2** The map's first entity is parsed into edict 0 without clearing it first, so
  the four fields of EDICT-4.1 keep their values unless the entity's keys set them. Every
  later entity is cleared first (EDICT-8.2).
- **EDICT-4.3** QuakeC may store into the world's fields only while the map is loading; once
  the server is active, `OP_ADDRESS` on edict 0 fails with `assignment to world entity`
  (VM-12.2; a pointer made while loading still writes later, VM-12.6). Engine code writes
  the world's fields at any time.
- **EDICT-4.4** The world is never handed out by allocation (EDICT-6.1), is never linked into
  the area tree, and gets physics like any other edict: `MOVETYPE_PUSH` (EDICT-10.4).
- **EDICT-4.5** The global `world` (slot 30) is never written by the engine; it is entity 0
  because qcc writes an uninitialised global as 0.
- **EDICT-4.6** `remove(world)` is not refused: edict 0 is freed by EDICT-7 like any other,
  and from then on the physics loop skips it. id's engine has no check, and this spec keeps
  its behaviour.

## EDICT-5 Client edicts

**Source:** `WinQuake/sv_main.c`, `SV_SpawnServer`, `SV_ConnectClient`;
`WinQuake/host_cmd.c`, `Host_Spawn_f`; `WinQuake/host.c`, `Host_FindMaxClients`,
`SV_DropClient`.

### Behaviour

- **EDICT-5.1** Edicts 1 to `maxclients` belong to the client slots in order: client slot `i`
  (from 0) is edict `i + 1`, whether or not a player is connected. `maxclients` is 1 to 16.
- **EDICT-5.2** When a player enters a map that was not loaded from a saved game, the engine
  zeroes the client edict's fields and sets `colormap` to the edict's number, `team` to the
  player's bottom colour plus 1 (`(colors & 15) + 1`) and `netname` to the player's name. It
  does not change the edict's free flag.
- **EDICT-5.3** A client edict is never handed out by allocation (EDICT-6.1), even when it is
  free or no player uses it. When a player disconnects the edict keeps its fields ("the body
  stays around").
- **EDICT-5.4** The physics loop runs a client edict only while its slot has a connected
  player, and skips it when it is free. A client edict freed by `remove` stays free until the
  next map, since EDICT-5.2 does not clear the flag.

## EDICT-6 Allocation (`spawn`)

**Source:** `WinQuake/pr_edict.c`, `ED_Alloc`, `ED_ClearEdict`; `WinQuake/pr_cmds.c`,
`PF_Spawn`.

`ED_Alloc` serves the `spawn` builtin and every map entity after the first. Its comment gives
the reason for the delay: "Try to avoid reusing an entity that was recently freed, because it
can cause the client to think the entity morphed into something else".

### Behaviour

- **EDICT-6.1** Allocation scans edicts `maxclients + 1` to `num_edicts - 1` in increasing
  order and takes the first one that is free and whose `freetime` is less than 2 or more than
  0.5 seconds before now (`freetime < 2 || sv.time - freetime > 0.5`).
- **EDICT-6.2** If no edict qualifies, it takes edict `num_edicts` and adds one to
  `num_edicts`. If `num_edicts` is already 600, it fails with `ED_Alloc: no free edicts`,
  even when free edicts exist that are too recently freed to qualify.
- **EDICT-6.3** The comparison is made in double precision between the server clock and the
  `f32` `freetime`, and is strict: an edict freed at time t is reused at t + 0.6 but not at
  t + 0.5. Because `freetime` is rounded to `f32`, the delay measured on the clock can differ
  slightly from 0.5: an edict freed at 100000.3 has `freetime` 100000.296875 and is reused at
  100000.8.
- **EDICT-6.4** An edict freed while `freetime` is below 2 (in the first second of a map,
  whose clock starts at 1, EDICT-8.1) can be reused at once.
- **EDICT-6.5** The edict handed out has every field slot set to 0 and its free flag cleared.
  Its header is otherwise left as it was, including `freetime`, the leaf links and the
  baseline.
- **EDICT-6.6** `spawn` returns the new edict's entity value. An edict allocated while the
  physics loop is running gets its turn in the same frame if its number is above the edict
  being run (EDICT-10.2), and waits for the next frame otherwise.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `EDICT_REUSE_DELAY` | 0.5 | `WinQuake/pr_edict.c`, `ED_Alloc`: `sv.time - e->freetime > 0.5` |
| `EDICT_REUSE_GRACE` | 2 | `WinQuake/pr_edict.c`, `ED_Alloc`: `e->freetime < 2`, "the first couple seconds of server time can involve a lot of freeing and allocating, so relax the replacement policy" |

## EDICT-7 Freeing (`remove`)

**Source:** `WinQuake/pr_edict.c`, `ED_Free`; `WinQuake/pr_cmds.c`, `PF_Remove`,
`PF_makestatic`, `PF_objerror`; `WinQuake/world.c`, `SV_UnlinkEdict`.

### Behaviour

- **EDICT-7.1** Freeing an edict unlinks it from the area tree, sets its free flag, sets
  `freetime` to the server clock rounded to `f32`, and sets these fields: `model`,
  `takedamage`, `modelindex`, `colormap`, `skin`, `frame`, `origin`, `angles` and `solid` to
  0, and `nextthink` to -1.
- **EDICT-7.2** Every other field keeps its value: `classname`, `think`, `touch`, `owner`,
  `health` and the rest can still be read through an entity value that refers to a freed
  edict.
- **EDICT-7.3** Nothing else changes: entity values elsewhere that refer to the freed edict
  are not cleared (id's comment: "FIXME: walk all entities and NULL out references to this
  entity"), and `num_edicts` does not shrink.
- **EDICT-7.4** Freeing an edict that is already free does it all again, so `freetime` moves
  to the later time and the reuse delay starts over.
- **EDICT-7.5** `remove` frees its argument by EDICT-7.1. `makestatic` frees its argument
  after sending it to clients as a static entity. `objerror` frees `self` before stopping the
  map with its error.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `EDICT_FREED_NEXTTHINK` | -1 | `WinQuake/pr_edict.c`, `ED_Free`: `ed->v.nextthink = -1` |

## EDICT-8 Spawning a map's entities

**Source:** `WinQuake/sv_main.c`, `SV_SpawnServer`; `WinQuake/pr_edict.c`,
`ED_LoadFromFile`, `ED_ParseEdict`, `ED_ParseEpair`, `ED_FindField`, `ED_FindFunction`;
`WinQuake/common.c`, `COM_Parse`; `WinQuake/server.h`, `SPAWNFLAG_NOT_*`.

### Behaviour

- **EDICT-8.1** A map starts with the server clock at 1. The engine sets the globals of
  EDICT-11.4, sets the `time` global to 1, and spawns the entities of the BSP's entity lump
  in file order. `time` is not set again between entities.
- **EDICT-8.2** The first entity goes into edict 0 (EDICT-4.2); each later one into an edict
  from allocation (EDICT-6), which clears it. Each `"key" "value"` pair is then applied in
  order, a later duplicate key overwriting an earlier one:
  - the key `angle` is renamed `angles` and its value `v` becomes `0 v 0`, and the key
    `light` is renamed `light_lev`;
  - trailing spaces are removed from the key;
  - a key starting with `_` is ignored;
  - a key that names no field is ignored, with the message `'<key>' is not a field`;
  - otherwise the value is converted by the field's type: a string becomes a new engine
    string with the escapes of EXAMPLE-1, a float is C `atof`, a vector is three `atof`
    values separated by single spaces, an entity is entity number `atoi(value)`, a function is
    the function of that name, and a field is the value of the global whose slot is the
    named field's offset (id reads `G_INT(def->ofs)`, the field's offset in the entity used
    as a global slot). An unknown function or field name is the error
    `ED_ParseEdict: parse error`.
- **EDICT-8.3** A vector value with fewer than three space-separated components reads past the
  end of the value in id's code, which is undefined; this spec fixes the answer: the missing
  components are 0.
- **EDICT-8.4** After its keys are applied, an entity is freed (EDICT-7) instead of spawned,
  and counted as inhibited, when deathmatch is on and its `spawnflags` has
  `SPAWNFLAG_NOT_DEATHMATCH` (2048); or when deathmatch is off and the skill is 0 with
  `SPAWNFLAG_NOT_EASY` (256), 1 with `SPAWNFLAG_NOT_MEDIUM` (512), or 2 or more with
  `SPAWNFLAG_NOT_HARD` (1024). The skill is the `skill` setting rounded (`(int)(skill + 0.5)`)
  and clamped to 0 to 3 when the map starts. `spawnflags` is truncated to an integer for the
  test.
- **EDICT-8.5** An entity with no `classname`, or whose `classname` names no function, is
  freed with the message `No classname for:` or `No spawn function for:` followed by the
  entity's fields. Function names are matched exactly and the first match wins.
- **EDICT-8.6** Otherwise the engine sets `self` to the entity and calls the function named by
  its `classname`, then moves on to the next entity. Nothing else (no `other`, no
  `frametime`) is set.
- **EDICT-8.7** Because the clock is 1 while entities spawn, an inhibited or classless entity's
  edict is reused by the very next allocation (EDICT-6.4).
- **EDICT-8.8** Text that is not `{` where an entity should start is the error
  `ED_LoadFromFile: found <token> when expecting {`; running out of text inside an entity is
  `ED_ParseEntity: EOF without closing brace`; a `}` where a value should be is
  `ED_ParseEntity: closing brace without data`.
- **EDICT-8.9** After the last entity the server becomes active and runs two physics frames of
  0.1 seconds each (EDICT-10), so play starts with the clock at 1.2. The `frametime` global is
  not set for these two frames: it keeps the progs file's value, or whatever the spawn
  functions stored.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `EDICT_MAP_START_TIME` | 1.0 | `WinQuake/sv_main.c`, `SV_SpawnServer`: `sv.time = 1.0` |
| `EDICT_SETTLE_FRAMES` | 2 | `WinQuake/sv_main.c`, `SV_SpawnServer`: "run two frames to allow everything to settle" |
| `EDICT_SETTLE_FRAMETIME` | 0.1 | `WinQuake/sv_main.c`, `SV_SpawnServer`: `host_frametime = 0.1` |
| `EDICT_SPAWNFLAG_NOT_EASY` | 256 | `WinQuake/server.h`, `SPAWNFLAG_NOT_EASY` |
| `EDICT_SPAWNFLAG_NOT_MEDIUM` | 512 | `WinQuake/server.h`, `SPAWNFLAG_NOT_MEDIUM` |
| `EDICT_SPAWNFLAG_NOT_HARD` | 1024 | `WinQuake/server.h`, `SPAWNFLAG_NOT_HARD` |
| `EDICT_SPAWNFLAG_NOT_DEATHMATCH` | 2048 | `WinQuake/server.h`, `SPAWNFLAG_NOT_DEATHMATCH` |

## EDICT-9 QuakeC entry points

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`; `WinQuake/sv_phys.c`, `SV_Physics`,
`SV_RunThink`, `SV_Physics_Pusher`, `SV_Physics_Client`, `SV_Impact`, `SV_PushMove`;
`WinQuake/world.c`, `SV_TouchLinks`; `WinQuake/host_cmd.c`, `Host_Spawn_f`, `Host_Kill_f`,
`Host_Changelevel_f`; `WinQuake/sv_main.c`, `SV_ConnectClient`, `SV_SaveSpawnparms`;
`WinQuake/host.c`, `SV_DropClient`.

Every call from the engine into QuakeC, and what it sets first. "Not set" means the global
keeps whatever value it had.

### Behaviour

- **EDICT-9.1** The engine calls QuakeC only at these points:

  | Function called | When | `self` | `other` | `time` |
  | --- | --- | --- | --- | --- |
  | The entity's spawn function | Each map entity (EDICT-8.6) | the entity | not set | 1 |
  | `StartFrame` | Start of every physics frame | world | world | clock |
  | `PlayerPreThink` | A connected client's physics turn, first | the client | not set | clock |
  | `think` | An entity's physics turn, when due (EDICT-10.3, EDICT-10.4) | the entity | world | EDICT-10.3 |
  | `PlayerPostThink` | A connected client's physics turn, last | the client | not set | clock |
  | `touch` of a trigger | An entity's box is relinked over a trigger's (`SV_TouchLinks`) | the trigger | the entity | clock |
  | `touch` of both | A move collides (`SV_Impact`) | each in turn | the other | clock |
  | `blocked` | A pusher's move is blocked | the pusher | the blocker | not set |
  | `SetNewParms` | A new player connects | not set | not set | not set |
  | `ClientConnect`, then `PutClientInServer` | A player enters the map (EDICT-5.2) | the client, for the first | not set | clock, for the first |
  | `ClientKill` | The `kill` command, unless the player's `health` is 0 or less | the client | not set | clock |
  | `SetChangeParms` | `changelevel`, for each connected client | the client | not set | not set |
  | `ClientDisconnect` | A spawned player disconnects normally | the client | not set | not set |

  "Clock" is the server clock rounded to `f32`. `PutClientInServer` runs straight after
  `ClientConnect` with whatever `self` and `time` that left.
- **EDICT-9.2** `touch` from a trigger (`SV_TouchLinks`) and from a collision (`SV_Impact`)
  save `self` and `other` before the call and restore them afterwards, as does
  `ClientDisconnect` for `self`. No other entry point restores anything.
- **EDICT-9.3** A trigger's `touch` runs only if the trigger is `SOLID_TRIGGER` (1) and
  `touch` is not 0. A collision runs each side's `touch` only if it is not 0 and the side is
  not `SOLID_NOT` (0), the moving entity first.
- **EDICT-9.4** The engine calls the function in the global or field even when it is 0 or out
  of range, which fails with `PR_ExecuteProgram: NULL function` (VM-2.1): a progs with no
  `StartFrame` fails on the first frame, and an entity with `nextthink` due and `think` 0
  fails when it would think. The function global `main` is never called.

## EDICT-10 The physics frame and think timing

**Source:** `WinQuake/host.c`, `Host_ServerFrame`; `WinQuake/sv_phys.c`,
`SV_Physics`, `SV_RunThink`, `SV_Physics_Pusher`, `SV_Physics_None`, `SV_Physics_Noclip`,
`SV_Physics_Toss`, `SV_Physics_Step`, `SV_Physics_Client`; `WinQuake/sv_user.c`,
`SV_RunClients`; `WinQuake/sv_main.c`, `SV_SendClientMessages`, `SV_CleanupEnts`.

### Behaviour

- **EDICT-10.1** A server frame does, in order: set the `frametime` global to the frame's
  length; read each client's input and apply its movement (`SV_RunClients`, which calls no
  QuakeC); run the physics frame unless paused; send the updates to clients. In single
  player the physics frame does not run while the console or a menu has focus.
- **EDICT-10.2** A physics frame calls `StartFrame` (EDICT-9.1), then visits edicts 0 to
  `num_edicts - 1` in increasing order, reading `num_edicts` again after each one, and skips
  free edicts. Each visited edict gets its turn: client edicts 1 to `maxclients` by
  `SV_Physics_Client`, and every other edict by its `movetype`. On an edict that is not a
  client, a `movetype` other than `NONE` (0), `STEP` (4), `FLY` (5), `TOSS` (6), `PUSH` (7),
  `NOCLIP` (8), `FLYMISSILE` (9) or `BOUNCE` (10) is the error
  `SV_Physics: bad movetype <n>`; on a client edict, one other than `NONE`, `WALK` (3),
  `FLY`, `TOSS`, `NOCLIP` or `BOUNCE` is the error `SV_Physics_client: bad movetype <n>`.
  After the last edict, the server clock advances by the frame's length.
- **EDICT-10.3** An entity thinks when its `nextthink` is greater than 0 and not more than the
  clock plus the frame's length (so up to one frame early). The engine sets `nextthink` to 0,
  sets `time` to `nextthink`, or to the clock if `nextthink` is earlier, sets `self` and
  `other`, and calls `think`. A `nextthink` of 0 or less never thinks, which is why a freed edict
  (`nextthink` -1) does not.
- **EDICT-10.4** A `MOVETYPE_PUSH` entity thinks by its own clock `ltime` instead: when
  `nextthink` is less than `ltime` plus the frame's length, it moves only up to `nextthink` (not
  at all if `nextthink` is behind `ltime`); after the move, if `nextthink` is above the old
  `ltime` and not above the new one, it sets `nextthink` to 0, `time` to the server clock (not
  `nextthink`), `self` and `other`, and calls `think`. A pusher that is blocked does not advance
  `ltime`, so it does not think.
- **EDICT-10.5** Where in its turn an entity thinks depends on `movetype`: `MOVETYPE_NONE`,
  `NOCLIP`, `TOSS`, `BOUNCE`, `FLY` and `FLYMISSILE` think before moving and do not move if
  the think freed them; `MOVETYPE_STEP` thinks after moving. A client edict thinks after
  `PlayerPreThink` and before its move; with `MOVETYPE_TOSS` or `BOUNCE` it moves by the toss
  physics, which also thinks first.
- **EDICT-10.6** If `force_retouch` is non-zero when the loop reaches an edict, the edict is
  relinked with trigger touching before its turn. After the loop, a non-zero `force_retouch`
  is decreased by 1. A negative value therefore relinks every frame and keeps decreasing.
- **EDICT-10.7** After the updates are sent each frame, the engine clears the muzzle-flash bit
  (`EF_MUZZLEFLASH`, 2) of `effects` on edicts 1 to `num_edicts - 1`, free or not.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `EDICT_EF_MUZZLEFLASH` | 2 | `WinQuake/server.h`, `EF_MUZZLEFLASH`, cleared by `SV_CleanupEnts` |

## EDICT-11 Who writes each system global

**Source:** as EDICT-9 and EDICT-10, and `WinQuake/sv_main.c`, `SV_SpawnServer`;
`WinQuake/pr_cmds.c`, `PF_makevectors`, `PF_traceline`, `PF_setspawnparms`, `PF_aim`;
`WinQuake/host_cmd.c`, `Host_Map_f`, `Host_Spawn_f`.

Every system global starts each map with its value in the progs file (EDICT-1.8). QuakeC may
write any of them at any time; the engine's writes are listed here.

### Behaviour

- **EDICT-11.1** `self`, `other`: written by the engine before each call into QuakeC as
  EDICT-9.1 says. The `walkmove` builtin saves `self` before its move, which can run touch
  functions, and restores it after. The engine reads `self` in builtins that act on it.
- **EDICT-11.2** `world`: never written by the engine (EDICT-4.5).
- **EDICT-11.3** `time`: written by the engine before each call into QuakeC as EDICT-9.1
  says. The engine reads it only in `OP_STATE` (VM-17.1).
- **EDICT-11.4** `deathmatch`, `coop`, `mapname`, `serverflags`: written once per map, before
  the entities spawn. If the `coop` setting is non-zero the engine writes `coop` from it and
  leaves `deathmatch` alone (the `deathmatch` setting is forced to 0); otherwise it writes
  `deathmatch` from its setting and leaves `coop` alone. `mapname` is the map's name without
  `maps/` or `.bsp`, and `serverflags` the value saved from the previous map (EDICT-11.6).
- **EDICT-11.5** `frametime`: written by the engine at the start of each server frame
  (EDICT-10.1), not during the spawn frames (EDICT-8.9).
- **EDICT-11.6** `serverflags`: QuakeC writes it; the engine reads it at `changelevel` to carry
  it to the next map, and sends its low 4 bits to clients as the sigils of the status bar
  (bits 28 to 31 of the items) unless the progs defines `items2`. The `map` command starts a
  new game with it 0.
- **EDICT-11.7** `force_retouch`: QuakeC writes it; the engine reads it and decreases it by 1
  each physics frame while it is non-zero (EDICT-10.6).
- **EDICT-11.8** `teamplay`, `total_secrets`, `total_monsters`, `found_secrets`,
  `killed_monsters`: never written by the engine. The four counters are read when a player
  enters the map, to send them to that player's status bar; `teamplay` is not read (the
  engine uses its `teamplay` setting).
- **EDICT-11.9** `parm1` to `parm16`: written by the engine from the client's saved parms just
  before `ClientConnect` (EDICT-9.1) and by the `setspawnparms` builtin. Read by the engine
  into the client's saved parms just after `SetNewParms` and after each `SetChangeParms`.
- **EDICT-11.10** `v_forward`, `v_up`, `v_right`: written only by the `makevectors` builtin;
  `v_forward` is read by the `aim` builtin.
- **EDICT-11.11** `trace_allsolid`, `trace_startsolid`, `trace_fraction`, `trace_endpos`,
  `trace_plane_normal`, `trace_plane_dist`, `trace_ent`, `trace_inopen`, `trace_inwater`:
  written only by the `traceline` builtin, all together; `trace_ent` is the world when the
  trace hit nothing. Never read by the engine. Movement that collides does not write them.
- **EDICT-11.12** `msg_entity`: never written by the engine; read by the `Write*` builtins when
  the destination is one client.
- **EDICT-11.13** `main`, `StartFrame`, `PlayerPreThink`, `PlayerPostThink`, `ClientKill`,
  `ClientConnect`, `PutClientInServer`, `ClientDisconnect`, `SetNewParms`, `SetChangeParms`:
  never written by the engine; each holds the function the engine calls by EDICT-9.1, and
  `main` is never read.

## EDICT-12 Who writes each system field

**Source:** as EDICT-9 to EDICT-11, and `WinQuake/sv_phys.c`, `sv_move.c`, `sv_user.c`,
`world.c`, `pr_cmds.c` (`PF_setorigin`, `SetMinMaxSize`, `PF_setmodel`, `PF_droptofloor`,
`PF_changeyaw`, `PF_findradius`, `PF_walkmove`, `PF_movetogoal`), `sv_main.c`
(`SV_WriteClientdataToMessage`), `host_cmd.c` (`Host_Spawn_f`, `Host_Name_f`, `Host_Color_f`
and the cheats `god`, `notarget`, `noclip`, `fly`, `give`).

"Physics" means the entity's turn in the physics frame (EDICT-10.2) and the movement of the
`walkmove` and `movetogoal` builtins; "client input" means `SV_RunClients` (EDICT-10.1);
"client update" means building the update for a connected player, after the physics frame.
Every field is written by QuakeC too, and every field is set by map keys (EDICT-8.2) and
zeroed by allocation (EDICT-6.5).

### Behaviour

- **EDICT-12.1** `modelindex`, `model`: written by the `setmodel` builtin, by freeing (to 0),
  and for the world when the map starts (EDICT-4.1).
- **EDICT-12.2** `absmin`, `absmax`: written by the engine whenever it links the entity into
  the area tree (`setorigin`, `setsize`, `setmodel` and physics): `origin + mins` and
  `origin + maxs`, widened by 15 on x and y for an entity whose `flags` has `FL_ITEM` (256)
  and by 1 on every axis otherwise. Not written for the world or a free edict.
- **EDICT-12.3** `ltime`: written by physics for `MOVETYPE_PUSH` entities, advanced by each
  move that is not blocked (EDICT-10.4).
- **EDICT-12.4** `movetype`, `solid`: `movetype` is written for the world (EDICT-4.1) and by
  the `noclip` and `fly` cheats; `solid` for the world, by freeing (to 0), and by physics,
  which sets a `MOVETYPE_PUSH` entity's `solid` to `SOLID_NOT` while it pushes each entity
  and then to `SOLID_BSP` (4), whatever it was before.
- **EDICT-12.5** `origin`: written by the `setorigin` and `droptofloor` builtins, by physics
  and client input, and by freeing (to 0). `oldorigin`: written by physics for client
  entities (`SV_CheckStuck`) to the last position that was not stuck.
- **EDICT-12.6** `velocity`: written by physics (gravity, clipping, `sv_maxvelocity`, a NaN
  component becomes 0) and client input. `avelocity`: written by physics, set to 0 with
  `velocity` when a `TOSS`, `BOUNCE`, `FLY` or `FLYMISSILE` entity lands on a floor, unless it
  bounces (`BOUNCE` with an upward speed of 60 or more).
- **EDICT-12.7** `angles`: written by the `changeyaw` builtin (and `movetogoal` through it),
  by physics from `avelocity`, by client input (pitch and yaw from `v_angle` unless
  `fixangle` is set, roll from movement), and by freeing (to 0).
- **EDICT-12.8** `punchangle`: decayed toward 0 by client input. `v_angle`, `button0`,
  `button2`, `impulse`: written by client input from the player's last move; `impulse` only
  when the move carries a non-zero impulse, so QuakeC must clear it. `button1` is never
  written by the engine.
- **EDICT-12.9** `classname`: never written by the engine except by map keys. `frame`,
  `skin`: written by freeing (to 0); `frame` also by `OP_STATE` (VM-17.2). `effects`: its
  `EF_MUZZLEFLASH` bit is cleared after every frame's updates (EDICT-10.7).
- **EDICT-12.10** `mins`, `maxs`, `size`: written by the `setsize` and `setmodel` builtins
  (`size` is `maxs - mins`). When a pusher is blocked by a `SOLID_NOT` or `SOLID_TRIGGER`
  entity, physics sets that entity's `mins` x and y to 0 and `maxs` to `mins`, without
  changing `size`.
- **EDICT-12.11** `touch`, `use`, `blocked`, `target`, `targetname`, `message`, `sounds`,
  `noise`, `noise1` to `noise3`, `deadflag`, `max_health`, `armortype`, `aiment`,
  `spawnflags`, `owner`, `movedir`, `enemy`, `goalentity`, `yaw_speed`, `view_ofs`,
  `weaponmodel`, `weaponframe`, `currentammo`, `frags`, `armorvalue`, `dmg_inflictor`,
  `takedamage` (except freeing, to 0): never written by the engine. Of these the engine reads
  `touch` and `blocked` (EDICT-9.1), `message` and `sounds` of the world (sent to clients as
  the level name and CD track), `spawnflags` (EDICT-8.4), `owner` (collision filtering),
  `enemy`, `goalentity` and `yaw_speed` (the movement builtins), `view_ofs` (water level, the
  `checkclient` builtin and the client update), `movedir` (water jumps), `takedamage` (the
  `aim` builtin), `frags` and the status-bar fields (client update). `use` is never read.
- **EDICT-12.12** `think`, `nextthink`: `think` is written only by `OP_STATE` (VM-17.3);
  `nextthink` by `OP_STATE` (VM-17.1), set to 0 just before each think (EDICT-10.3, EDICT-10.4),
  and set to -1 by freeing.
- **EDICT-12.13** `groundentity`: written by physics and by the `droptofloor`, `walkmove` and
  `movetogoal` builtins when the entity lands on something.
- **EDICT-12.14** `health`, `weapon`, `items`, `ammo_shells`, `ammo_nails`, `ammo_rockets`,
  `ammo_cells`: written by the engine only through the `give` cheat; read for the client
  update, and `health` also by `kill` (EDICT-9.1) and client input (a player with 0 or less
  does not move).
- **EDICT-12.15** `chain`: written by the `findradius` builtin, which links its results
  through it.
- **EDICT-12.16** `fixangle`, `dmg_take`, `dmg_save`: cleared to 0 by the client update once it
  has sent them (the view angles if `fixangle` is set; the damage indicator if either damage
  field is non-zero). `idealpitch`: written during each client update, on the edict of the
  last client whose input was read (id's `SV_SetIdealPitch` uses `sv_player`, not the client
  being updated; with one player they are the same).
- **EDICT-12.17** `netname`, `colormap`, `team`: written when a player enters the map
  (EDICT-5.2); `netname` also by the `name` command and `team` by the `color` command;
  `colormap` set to 0 by freeing.
- **EDICT-12.18** `flags`: the engine sets and clears `FL_ONGROUND` (512),
  `FL_PARTIALGROUND` (1024) and `FL_WATERJUMP` (2048) in physics, client input and the
  `droptofloor`, `walkmove` and `movetogoal` builtins; the `god` and `notarget` cheats toggle
  `FL_GODMODE` (64) and `FL_NOTARGET` (128). It does so by converting `flags` to an integer
  and back, so a fractional part is lost.
- **EDICT-12.19** `teleport_time`: cleared to 0 by client input when a water jump ends.
  `waterlevel`, `watertype`: written by physics from the contents at the entity's position.
- **EDICT-12.20** `ideal_yaw`: written by the `movetogoal` builtin as it picks directions.

## EDICT-13 Engine-made values

**Source:** `WinQuake/sv_main.c`, `SV_SpawnServer`; `WinQuake/host_cmd.c`, `Host_Spawn_f`,
`Host_Name_f`; `WinQuake/pr_edict.c`, `ED_NewString`, `ED_ParseEpair`; `WinQuake/pr_exec.c`,
`OP_NOT_S`.

### Behaviour

- **EDICT-13.1** A string value the engine makes (`mapname`, `netname`, the world's `model`, and
  every string value set from a map key) is the address of engine-owned text minus the address
  of the string table (PROGS-11.6). It is not 0 and does not lie inside the string table; its
  number depends on memory layout and is not part of this spec. The strings builtins return are
  in `builtins.md`.
- **EDICT-13.2** An engine-made string value is true in a condition unless its text is empty
  (`OP_NOT_S` tests both the value and the first byte).
- **EDICT-13.3** QuakeC Studio represents engine-made strings in a range of values that cannot
  be a string-table offset, so that EDICT-13.1 and EDICT-13.2 hold; the differential
  comparison (`differential.md`, P1-27) compares their text, not their numbers.
- **EDICT-13.4** An entity value the engine makes is always EDICT-1.5's value for that edict;
  the engine never makes a pointer value.

## Test vectors

### Layout

The slots of EDICT-2.1 and EDICT-3.1 and the header size of EDICT-1.3 were checked on
2026-10-16 by a local harness that included `WinQuake/progdefs.q1` unchanged and the `edict_t`,
`entity_state_t` and `link_t` definitions copied verbatim from the pinned commit, printing
`offsetof(...) / 4` of every member of `globalvars_t` and `entvars_t` and
`sizeof(edict_t) - sizeof(entvars_t)`. It printed the slots in the two tables, 92 for
`sizeof(globalvars_t) / 4` and 105 for `sizeof(entvars_t) / 4`. Compiled with Apple clang 21
for `i386-unknown-linux-gnu` the header is 96 bytes, and for `x86_64-unknown-linux-gnu` and
the 64-bit macOS host it is 108.

| Input | Output | Statements |
| --- | --- | --- |
| Header size, 32-bit x86 | 96 | EDICT-1.1, EDICT-1.3 |
| Edict size with 105 field slots | 516 | EDICT-1.1 |
| Entity value of edict 3, 105 field slots | 1548 | EDICT-1.5 |
| Pointer to `nextthink` (slot 46) of edict 2, 105 field slots | 1032 + 96 + 184 = 1312 | EDICT-1.6 |
| `self`, `time`, `parm1`, `trace_ent`, `SetChangeParms` | slots 28, 31, 43, 78, 91 | EDICT-2.1 |
| `origin`, `think`, `nextthink`, `flags`, `noise3` | field slots 10, 44, 46, 76, 104 | EDICT-3.1 |

### Allocation and freeing

Checked on 2026-10-16 by running id's `ED_ClearEdict`, `ED_Alloc`, `ED_Free`, `EDICT_NUM` and
`NUM_FOR_EDICT`, copied unchanged from `WinQuake/pr_edict.c` at the pinned commit into a local
harness with `sv`, `svs` and `progs` reduced to the members those functions read,
`SV_UnlinkEdict` stubbed out and `Sys_Error` printing its message. The edicts had 105 field
slots. Each row starts from a new map: `num_edicts` is `maxclients + 1` and nothing is free.

Since P1-32 every row is a vector in [`vectors/vm/edicts.txt`](vectors/vm/edicts.txt), which
`cargo xtask oracles` runs through the VM oracle on Linux, macOS and Windows in CI: `spawn`,
`remove` and `nextent` are id's `PF_` functions, the host sets the clock between runs, and the
`EDICT_NUM` rows call id's function from the host. On 2026-10-16 all 17 passed on macOS arm64.
The trace shows fields and globals, not the edict header, so the vectors show the header through
what reads it: the free flag through `nextent`, which skips free edicts, and `freetime` through
the time at which `spawn` reuses the edict. Edict 2 of the `freetime` 3.25 row starts in use with
every field slot 7 rather than being spawned and stored to; "`freetime` still 3.25" is not
visible to QuakeC, since only allocation reads `freetime` and only of a free edict.

| `maxclients` | Steps (time: action) | Result | Statements |
| --- | --- | --- | --- |
| 1 | 1.0: spawn, spawn | edicts 2, 3; `num_edicts` 4 | EDICT-6.2, EDICT-1.4 |
| 1 | 1.0: spawn, spawn, remove 2, spawn | the last spawn is 2 | EDICT-6.4, EDICT-8.7 |
| 1 | 1.0: spawn ×2; 1.9: remove 3; 1.95: spawn | 3 | EDICT-6.4 |
| 1 | 5.0: spawn (2), remove 2, spawn; 5.5: spawn; 5.6: spawn | 3, then 4, then 2 | EDICT-6.1, EDICT-6.3 |
| 1 | 2.0: spawn (2), remove 2, spawn | 3 | EDICT-6.1, EDICT-6.4 |
| 4 | 10.0: spawn ×4 (5 to 8), remove 8, remove 6; 11.0: spawn | 6 | EDICT-6.1 |
| 1 | 100000.3: spawn (2), remove 2; 100000.8: spawn | `freetime` 100000.296875; spawn is 2 | EDICT-6.3, EDICT-7.1 |
| 1 | 10.0: spawn ×598 | `num_edicts` 600; the next spawn fails with `ED_Alloc: no free edicts` | EDICT-6.2 |
| 1 | then 10.0: remove 300; 10.2: spawn; 10.6: spawn | `ED_Alloc: no free edicts`, then 300 | EDICT-6.2 |
| 1 | 3.0: spawn (2), every field slot set to 7; 3.25: remove 2 | free, `freetime` 3.25; slots 0, 9 to 12, 19 to 21, 29, 30, 31, 59, 77 are 0, slot 46 is -1, every other slot is 7 | EDICT-7.1, EDICT-7.2 |
| 1 | then 4.0: spawn | 2, not free, every field slot 0, `freetime` still 3.25 | EDICT-6.5 |
| 1 | then 4.0: remove 2; 4.4: remove 2; 4.6: spawn; 5.0: spawn | 3, then 2 | EDICT-7.4 |
| 1 | `EDICT_NUM(600)`, `EDICT_NUM(-1)`, `EDICT_NUM(599)` | `EDICT_NUM: bad number 600`, `EDICT_NUM: bad number -1`, edict 599 | EDICT-1.7 |
| 1 | `NUM_FOR_EDICT` of edict `num_edicts` | `NUM_FOR_EDICT: bad pointer` | EDICT-1.7 |
| 8 | 1.0: remove 3, spawn | 9 | EDICT-5.3, EDICT-6.1 |

### Entity values

EDICT-1.9 is checked through the builtins that read entity values, by the vectors
`entity_unused_edict`, `entity_past_the_array`, `entity_not_a_multiple` and `entity_negative`
in [`vectors/vm/builtins.txt`](vectors/vm/builtins.txt), which the VM oracle passed on
2026-10-16 (`builtins.md`, Test vectors): with three edicts in use, edict 10 names an edict and
its fields can be written and read; edict 600, the value 4 and -1 name none.

The rest of this spec (EDICT-4, EDICT-5.2, EDICT-8 to EDICT-13) is behaviour of the running
server. It was written from the source at the pinned commit and is checked against id's engine
by the server simulation differential suite once the engine oracle exists (P3-25); until then
its vectors are the statements themselves.
