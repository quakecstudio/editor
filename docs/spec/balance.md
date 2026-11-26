# A map's balance

Prefix: `BALANCE` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the balance sheet of the open map (L-78, MAP-37): on each skill and in each kind of
game, the monsters and their health, the weapons, the ammunition and the damage it can do, the
health, armour and secrets, as the game's own code spawns them. It is this project's design,
not engine behaviour: which entities a skill keeps is the engine's rule (EDICT-8.4), and what
an entity is and gives is the program's own code, never a table of classnames, so a mod's
monsters and items are counted as id's are. Implemented by `qs-sim` (`spawn::load_from_file`,
the counters around each spawn function) and `qs-workbench` (`map_balance`, the command
`map.balance`).

Prior art: none in a Quake map editor. A mapper balances a level by playing it on each skill
and counting by hand, or by reading the counts the status bar shows at the start (the
`total_monsters` and `total_secrets` a player is sent, EDICT-11.8). The model followed is that
count, made for each skill at once and broken down by class, layer and group.

## BALANCE-1 What the load records

**Source:** this design; the counters are the program's globals `total_monsters` and
`total_secrets`, which the engine never writes and reads only to send them to a player who
enters (EDICT-11.8). id's `walkmonster_start` (`monsters.qc`) and `trigger_secret`
(`triggers.qc`) add one to them in the spawn function.

### Behaviour

- **BALANCE-1.1** When a map loads, each entity whose spawn function runs (EDICT-8.6) records
  what the function added to `total_monsters` and to `total_secrets`: each global read before
  the function ran and after it returned. An entity inhibited (EDICT-8.4), with no class
  (EDICT-8.5), or put in the map after the load added nothing. Recording changes nothing the
  game does.
- **BALANCE-1.2** `map.balance` is a command of the registry that reads and changes nothing. It
  needs an open project whose server program builds and an open map, and refuses a parameter
  it does not know.

## BALANCE-2 The sheets

**Source:** this design.

### Behaviour

- **BALANCE-2.1** A sheet is made by spawning the open map as `map.entities_removed_by_skill`
  spawns it (DIFF-3.5), and reading what the spawn left before any frame runs: a monster is an
  entity that added to `total_monsters` (BALANCE-1.1), counted as many times as it added, with
  the `health` it has after its spawn function, which may read the skill; a secret is one that
  added to `total_secrets`.
- **BALANCE-2.2** There is a sheet for each skill 0 to 3 in each of single player, coop and
  deathmatch, twelve in all, skill by skill. An entity is on a sheet exactly when the game keeps
  it at that skill in that kind of game: the engine's spawnflags test (EDICT-8.4), so nightmare's
  entities are hard's and deathmatch tests no skill flag, and whatever the program removes
  itself.
- **BALANCE-2.3** A tally of some of the map's entities gives the monsters by classname with
  their count and health; the pickups that give a weapon by classname; the ammunition by the
  field it is kept in, with its units and its damage (BALANCE-4.2); the health, the armour and
  the secrets; the damage of the ammunition; and that damage over the monsters' health, none
  when they have none. Each line names its entities by place in the map. A sheet has a tally of
  the whole map, and of the entities selected when `selection` names any.
- **BALANCE-2.4** A sheet has a tally of each layer and group of the map, in map order, with its
  name: the entities in it, directly or in a group in it however deep, as the map's own
  containers place them (MAP-9.2).
- **BALANCE-2.5** A sheet's monster count is the program's `total_monsters` after the map
  spawned, and its secret count the program's `total_secrets`, both of which it gives.
- **BALANCE-2.6** The sheets of one map are the same on every run: the spawn and each touch
  start from the same state (MOVE-6.1).

## BALANCE-3 What a pickup gives

**Source:** this design; id's `armor_touch` (`items.qc`) gives armour by its `classname`, and
`weapon_touch` the ammunition a weapon comes with, so what a pickup gives is in its `touch`,
not in a field its spawn function sets.

### Behaviour

- **BALANCE-3.1** After the spawn the player is brought into the game (DIFF-16.1), and what the
  player then has is the sheet's start: health, armour (`armorvalue`), each field named `ammo_`
  and the weapons of `items`. Each entity of the map still in use that is not a monster is then
  touched, its `touch` run with the player as `other` (FIRE-1.4) and no frame after, each in a
  copy of the game, so one touch changes nothing another sees.
- **BALANCE-3.2** Before each touch the player is emptied: `health` 1, `armorvalue`,
  `armortype` and `items` 0, and every field named `ammo_` 0. What the entity gives is the
  player's health above 1, its `armorvalue`, each `ammo_` field above 0 and the weapons of
  `items` (`WinQuake/quakedef.h`, `IT_SHOTGUN` 1 to `IT_SUPER_LIGHTNING` 128) after the touch;
  only a gain counts, so a trigger that hurts gives nothing. An entity with no `touch`, or
  whose `touch` fails, gives nothing.
- **BALANCE-3.3** The player's fields are read as the program declares them, the four the
  engine defines (`ammo_shells`, `ammo_nails`, `ammo_rockets`, `ammo_cells`) and a mod's own
  alike.
- **BALANCE-3.4** When the program cannot put a player in the map, as id's
  `PutClientInServer` cannot on a level with no `info_player_start`, the sheet says why, its
  monsters and secrets are counted, and its pickups give nothing.

## BALANCE-4 The damage of ammunition

**Source:** `weapons.qc` of id 1.01 (`id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73`,
`qcc/v101qc`) and of the rerelease
(`id-Software/quake-rerelease-qc@634eefab09a77eb7b5f5ca7078ba3d8784a91142`, `quakec`), quoted
line by line in [`../agent-reference/weapons.md`](../agent-reference/weapons.md), "What a unit of
ammo can do".

### Behaviour

- **BALANCE-4.1** The damage of id's weapons is a table of each weapon that takes ammunition:
  the field it takes from, the units a shot takes and the most a shot does to one target with
  every pellet or nail hitting, a random part at its least. The shotgun takes 1 shell for 24,
  the super shotgun 2 for 56, the nailgun 1 nail for 9, the super nailgun 2 for 18, the grenade
  launcher 1 rocket for 120, the rocket launcher 1 for 100, the thunderbolt 1 cell for 30. Both
  sources give each weapon the same numbers. `map.balance` answers the table with its sheets.
- **BALANCE-4.2** A unit of one of the engine's four kinds of ammunition does the most any
  weapon of the table does with one unit: a shell 28, a nail 9, a rocket 120, a cell 30. A mod's
  own kind is counted in units with no damage, and its pickups add nothing to a tally's damage.
  The start's ammunition counts in the sheet's own ratio, the damage of the start and the map
  over the map's monsters' health; a tally's ratio counts only its own entities.
