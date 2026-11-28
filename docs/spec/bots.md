# Bots

Prefix: `BOT` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the bots the preview host plays coop and deathmatch with (L-87, ENG-18,
[ADR-0031](../adr/0031-bots-and-local-players.md)). It is this project's design, not engine
behaviour: a bot is a client with no connection whose moves come from Rust code, so the mod's
own player QuakeC plays it and the server stays id-exact. The prior art is Quake III Arena's
bot library (navigation areas, goals weighted by item, aim and combat apart from moving); the
Reaper Bot and Frogbot are bots written in QuakeC. Implemented by `qs-sim` (the slots and the
console, BOT-1), `qs-map-gen` (the graph, BOT-2), `qs-bots` (the mind, BOT-3 to BOT-8) and
`qs-game` and `qs-workbench` (the host and the commands, BOT-9).

The QuakeC values a bot reads are id's, from
id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc/defs.qc`:
`FL_CLIENT` 8, `FL_MONSTER` 32, `FL_ONGROUND` 512, `SOLID_TRIGGER` 1, `DEAD_NO` 0, the weapon
bits `IT_SHOTGUN` 1 to `IT_LIGHTNING` 64 and `IT_AXE` 4096.

## BOT-1 The bot's slot

**Source:** this design, over the several-client server of NET-7.

### Behaviour

- **BOT-1.1** A bot the host drives is a client slot of its own kind: it has no connection, so
  it is sent nothing (NET-7.3), and each frame its move is the one the host last set, read as
  a local client's move is read off the wire (PHYS-9.6): its angles and speeds quantized, its
  buttons and impulse set on its edict. `status` lists its address as `BOT`.
- **BOT-1.2** The console's `addbot [skill] [name]` brings a bot into the first slot not active
  (NET-7.4): it connects with `SetNewParms`' parms, or those it carries from the last level,
  sends `name` and `color` as a client does, then `prespawn`, `spawn` and `begin`, so the mod's
  `ClientConnect` and `PutClientInServer` make its player. With every slot active it prints
  "`addbot: every client slot is taken (<n> max)`". `removebot [name]` drops the active bot
  named `name`, ASCII case apart, or with no name the bot in the highest slot, as
  `SV_DropClient` drops a client (NET-7.9); with none it prints "`removebot: no bot named
  <name>`" or "`removebot: no bots`". Neither touches a local client.
- **BOT-1.3** A bot named in `addbot` takes the name with its quotes and control bytes left out,
  cut to 15 bytes. An unnamed one takes the first of the list `Ash`, `Birch`, `Cedar`, `Dune`,
  `Ember`, `Flint`, `Gale`, `Heron`, `Iron`, `Jade`, `Kestrel`, `Lumen`, `Moss`, `Nettle`,
  `Onyx`, `Pike` that no active slot has, ASCII case apart, else `bot` and its slot's number
  from 1. Its shirt and pants colours are its slot's, from a list of sixteen.
- **BOT-1.4** A bot plays at a skill from 0 to 3. `addbot`'s first argument is the skill when it
  starts with a digit, `atoi` of it rounded and kept to 0 to 3, and the name is the next; any
  other first argument is the name. With no skill given, the bot plays at the `skill`
  setting, rounded and kept to 0 to 3 as the server reads it (DIFF-3.2).
- **BOT-1.5** Each bot brought in or dropped is told to the host once, in order, with its slot,
  and a bot brought in with its skill, so the host gives it a mind or takes its mind away.
- **BOT-1.6** A bot's mind reads, each frame, every edict in use, the world first: its number,
  `classname`, `origin`, `velocity`, `absmin`, `absmax`, `view_ofs`, `v_angle`, `health`,
  `armorvalue`, `items`, `weapon`, the four ammunition counts, `deadflag`, `flags`, `solid`,
  `takedamage`, `frags`, `team`, `modelindex` and `waterlevel`; and it sees along a line traced
  through the world and its entities from inside its own box, its own edict passed through.

## BOT-2 The ground as a graph

**Source:** this design, over the playability check's moves (`qs-map-gen`, L-77): walking up
a step of 18, falling, jumping, lifts, teleporters, trains, swimming and doors as id's QuakeC
opens them.

### Behaviour

- **BOT-2.1** A map's graph is every place a player reaches from its spawns, each
  `info_player_start`, `info_player_coop` and `info_player_deathmatch` the game keeps at its
  skill and deathmatch setting (EDICT-8.4), a place a cell of the playability check's search
  whatever the player has opened there; and a move from one place to another wherever a
  player makes it in some state, its length the straight distance between them. A spawn a
  search already reached adds nothing. A move is one way: a fall or a teleporter is not walked
  back.
- **BOT-2.2** The place of a point is the graph's place nearest it, a height apart counting
  twice: first among the places over the 64-unit squares around the point, else among all.
- **BOT-2.3** A route from one place to others is the shortest along the moves, by the sum of
  their lengths, one for each place sought, none for a place the moves do not lead to; it
  lists the places it passes from its start to its end. A route to the start itself is that
  place alone, of length 0. A path from one point to another is the route along the graph
  reached from the first, from its place to the place nearest the second, which is on the
  first's side of a wall when nothing leads past it.

## BOT-3 What a bot sees

**Source:** this design.

### Behaviour

- **BOT-3.1** A bot's foes are, in deathmatch, every other client edict that is alive
  (`deadflag` 0, `health` above 0) and drawn (`modelindex` not 0), and in coop every edict that
  is a monster (`flags & FL_MONSTER`), alive and can be hurt (`takedamage` not 0). Its friends,
  in coop, are the other clients that are alive.
- **BOT-3.2** A bot sees a foe within 2048 units whose middle, its box's, is within 90 degrees
  of where the bot looks, side to side, when a line from the bot's eye to that middle meets
  nothing before the foe. Of the foes it sees, it fights the nearest, and keeps fighting the
  one it fights while it still sees it.
- **BOT-3.3** A dead bot (`deadflag` not 0) moves nothing and releases fire, jump and use, then
  presses fire in the next frame, and so on in turn, as id's `PlayerDeathThink` waits for every
  button to be released and then for one to be pressed before `respawn`.

## BOT-4 Where a bot goes

**Source:** this design, after Quake III Arena's goals weighted by item.

### Behaviour

- **BOT-4.1** A bot chooses where to go when it has no route, when it has come to the end of
  its route, when it has moved less than 32 units in two seconds while it has a route, when
  it reaches the item it went for, and every four seconds; when it has not moved it also
  jumps. It
  weighs each goal by its need of it divided by one plus the route's length over 256, and takes
  the goal of the greatest weight above 0; with none it goes to a place of the graph chosen at
  random.
- **BOT-4.2** A bot's need of an item that lies where it can be picked up (`solid` is
  `SOLID_TRIGGER`), by its classname: a weapon of the weapon table it does not hold, the
  weapon's value; one it holds, 5. `item_health`, its missing health below 100. `item_armor1`, `item_armor2` and `item_armorInv`, 30, 50
  and 80, when its `armorvalue` is below 100, 150 and 200. An ammunition box, `item_shells`,
  `item_spikes`, `item_rockets` or `item_cells`, 20 when it holds a weapon of the table that
  uses it and has less than half of 100 of it (200 for nails), else 2. A powerup
  (`item_artifact_super_damage`, `item_artifact_invulnerability`, `item_artifact_invisibility`
  or `item_artifact_envirosuit`), 60. Anything else, nothing.
- **BOT-4.3** In coop, a bot that is more than 256 units from its nearest friend needs to go to
  that friend, 100, plus 1 for each unit past 256, so that it follows the player.
- **BOT-4.4** A bot walks its route a place at a time: it moves toward the next place of its
  route, and takes the one after once it is within 24 units of it across and at most 48 below it or 18,
  a step, above it.
  It jumps when the next place is more than 18 and at most 48 above it and within 64 across, or
  when the move to it from the place before is more than 40 across, a leap over a gap; it waits when the next place is more than 48 above
  it and within 24 across, as on a lift going up; it swims up when it is in water and the next
  place is above it.
- **BOT-4.5** An item a bot comes within 48 units of across, and 48 up or down, is passed over
  as a goal for ten seconds, so that one it cannot take, as a weapon it holds in coop, does
  not hold it there.

## BOT-5 Aim

**Source:** this design.

### Behaviour

- **BOT-5.1** A bot aims at the middle of the foe it fights, with an error, side to side and up
  and down, drawn afresh every half second from its own random numbers, uniformly within plus
  or minus 12, 7, 4 and 2 degrees at skills 0, 1, 2 and 3.
- **BOT-5.2** A bot turns its view at most 180, 360, 540 and 900 degrees a second at skills 0
  to 3, toward its aim when it fights, else toward where it moves.
- **BOT-5.3** A bot fires only once it has seen the foe it fights for its reaction time, 0.6,
  0.4, 0.25 and 0.15 seconds at skills 0 to 3, and while its view is within 15 degrees of its
  aim; it holds fire otherwise.

## BOT-6 Weapons

**Source:** id's `W_ChangeWeapon` (`weapons.qc`): impulse 1 selects `IT_AXE`, 2 `IT_SHOTGUN`,
3 `IT_SUPER_SHOTGUN`, 4 `IT_NAILGUN`, 5 `IT_SUPER_NAILGUN`, 6 `IT_GRENADE_LAUNCHER`,
7 `IT_ROCKET_LAUNCHER`, 8 `IT_LIGHTNING`, refused without the weapon or "not enough ammo"
(fewer than 1 shell, 2 for the super shotgun, 1 nail, 2 for the super nailgun, 1 rocket or 1
cell).

### Behaviour

- **BOT-6.1** The weapon table lists, best first, each weapon's `items` bit, its impulse, the
  ammunition it uses and the least it fires with, its pickup's classname and its value as a
  goal. id's table, the default: the rocket launcher (100), the lightning gun (90), the super
  nailgun (70), the grenade launcher (60), the super shotgun (50), the nailgun (40), the
  shotgun and the axe, with id's impulses and least ammunition. A mod may give its own.
- **BOT-6.2** A bot holds the first weapon of the table it has (`items`) with the least
  ammunition it fires with; when that is not its `weapon`, it sends the weapon's impulse, at
  most once every half second.

## BOT-7 How a bot moves

**Source:** this design.

### Behaviour

- **BOT-7.1** A bot moves at 400 units a second toward where it goes, as forward and side
  speeds from the angle between where it moves and where it looks, which `sv_maxspeed` then
  limits; it does not move while it waits.
- **BOT-7.2** A bot with no graph stands where it is and still fights and respawns.

## BOT-8 Random numbers

**Source:** this design.

### Behaviour

- **BOT-8.1** Each bot draws its random numbers from a source of its own, which starts from the
  host's seed and its slot, so the same game with the same moves gives every bot the same
  numbers on every run (SCN-2.5), and two bots never share a draw.

## BOT-9 The host

**Source:** this design.

### Behaviour

- **BOT-9.1** Before each server frame the host gives each bot brought in a mind at its skill,
  takes the mind of each bot dropped, or whose slot is no longer a bot's the host drives, as
  after `kick`, and sets each bot's move from its mind.
- **BOT-9.2** A scenario played on a game plays its bots. Its engine words choose the slots as
  id's `Host_FindMaxClients` (`WinQuake/host.c`) reads `-listen`: "`if (i != (com_argc - 1))
  svs.maxclients = Q_atoi (com_argv[i+1]); else svs.maxclients = 8;`", then "`if
  (svs.maxclients < 1) svs.maxclients = 8; else if (svs.maxclients > MAX_SCOREBOARD)
  svs.maxclients = MAX_SCOREBOARD;`", and with more than one slot `deathmatch` is 1 (NET-7.1).
  Each `+<name> <value>` pair after that sets a setting before the map loads, as the command
  line's `+` commands run once the host has started, such as `+deathmatch 0 +coop 1`.
- **BOT-9.3** The commands `game.add_bot`, with an optional skill from 0 to 3 and name, and
  `game.remove_bot`, with an optional name, type `addbot` and `removebot` at the running
  game's console and answer what it printed.
