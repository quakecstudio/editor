# Player starts

Prefix: `START` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes where id1's QuakeC puts a spawning player and when that player telefrags another, and
the checks of a coop or deathmatch map's starts that follow from it (L-88, MAP-39). The QuakeC
is id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc/client.qc`
(`SelectSpawnPoint`, `PutClientInServer`), `triggers.qc` (`spawn_tdeath`, `tdeath_touch`) and
`defs.qc` (`VEC_HULL_MIN`, `VEC_HULL_MAX`); the engine is id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d
`WinQuake/world.c` (`SV_LinkEdict`, `SV_TouchLinks`). Implemented by `qs-workbench`
(`map_starts.rs`), for `map.issues`.

## START-1 Where a player spawns

**Source:** `client.qc`, `SelectSpawnPoint` and `PutClientInServer`.

### Behaviour

- **START-1.1** In coop, `SelectSpawnPoint` takes the `info_player_coop` after the last one
  taken, `lastspawn = find(lastspawn, classname, "info_player_coop");`, and when there is none
  after it the first `info_player_start`, `if (lastspawn == world) lastspawn = find (lastspawn,
  classname, "info_player_start");`. The coop starts and the first `info_player_start` are the
  starts coop players take in turn; with no `info_player_coop` every coop player spawns at the
  first `info_player_start`.
- **START-1.2** In deathmatch it takes the `info_player_deathmatch` after the last one taken,
  wrapping to the first (`lastspawn = find(lastspawn, classname, "info_player_deathmatch"); if
  (lastspawn == world) lastspawn = find (lastspawn, classname, "info_player_deathmatch");`), so
  with fewer deathmatch starts than players two players share one. With none it falls through to
  `spot = find (world, classname, "info_player_start");`, and every player spawns there.
- **START-1.3** `PutClientInServer` puts the player at `spot.origin + '0 0 1'` with the box
  `VEC_HULL_MIN` to `VEC_HULL_MAX`, `'-16 -16 -24'` to `'16 16 32'`, and calls `spawn_tdeath
  (self.origin, self)`. A start whose box there is in solid leaves the player stuck.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `VEC_HULL_MIN` | `'-16 -16 -24'` | `defs.qc` |
| `VEC_HULL_MAX` | `'16 16 32'` | `defs.qc` |
| The spawn lift | `'0 0 1'` | `client.qc`, `PutClientInServer` |

## START-2 Telefragging at a start

**Source:** `triggers.qc`, `spawn_tdeath` and `tdeath_touch`; `world.c`, `SV_LinkEdict` and
`SV_TouchLinks`.

### Behaviour

- **START-2.1** `spawn_tdeath` makes a trigger the spawning player's box grown by 1 each way
  (`setsize (death, death_owner.mins - '1 1 1', death_owner.maxs + '1 1 1');`) at the player's
  origin, whose touch kills any other player with health (`T_Damage (other, self, self,
  50000);`).
- **START-2.2** `SV_LinkEdict` grows every linked box that is not an item by 1 more each way
  ("we must fully check even when bounding boxes don't quite touch"), and `SV_TouchLinks` runs
  a trigger's touch unless one box is wholly past the other (`ent->v.absmin[0] >
  touch->v.absmax[0] || ...`), so boxes that only touch count. A player spawning at one start
  therefore telefrags a player standing at another when the starts are at most 35 units apart
  on each of x and y and at most 59 apart on z: 16 + 16 + 1 + 1 + 1 across, and 24 + 32 + 1 + 1
  + 1 up and down.

## START-3 The checks of a map's starts

**Source:** this project's design, from START-1 and START-2.

### Behaviour

- **START-3.1** A deathmatch map, one with an `info_player_deathmatch` or played as deathmatch,
  needs a deathmatch start for each player and bot of a deathmatch chosen, else for 4; fewer is
  an issue quoting START-1.2, with no fix.
- **START-3.2** Coop chosen with more than one player and bot and no `info_player_coop` is an
  issue quoting START-1.1. Its fix places an `info_player_coop`, facing as the first
  `info_player_start` does, at the first of the eight places 64 units from it (east, west,
  north, south, then the diagonals) where a player stands and telefrags none of the coop
  starts.
- **START-3.3** A start (`info_player_start`, `info_player_start2`, `info_player_coop`,
  `info_player_deathmatch` or `testplayerstart`) whose player box (START-1.3) shares a volume
  with a solid brush is an issue. A solid brush is one of the world, `func_group` or a
  `func_detail` other than `func_detail_illusionary`, which the extended dialect merges into the
  world, or of a `func_` brush entity other than `func_illusionary`, with a face that is not a
  liquid (`*`); boxes that only touch share none. Its fix moves the start to the nearest height
  within 64 units, above first, where a player stands and telefrags none of its group's starts.
- **START-3.4** Two starts of one group that telefrag (START-2.2) are an issue on the later one:
  the deathmatch starts are one group, and the coop starts with the first `info_player_start`
  another (START-1.1). Its fix moves the later one to the first place of START-3.2's eight around
  it where a player stands and telefrags none of the group.
- **START-3.5** A player stands at a place when a solid brush shares a volume with the 16 units
  below its box's bottom.
