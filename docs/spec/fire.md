# Firing an entity

Prefix: `FIRE` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes firing an entity of the open map in the editor: its `use` or `touch` run in `qs-sim`
with the project's own program, the trace of what the program did about it, and the issues only
running shows (L-76, MAP-35). It is this project's design, not engine behaviour: what a frame
does is `qs-sim`'s, as `docs/spec/engine/` says, and what firing means is the program's own
code, never a table of classnames, so a mod's triggers are traced as id's are. Implemented by
`qs-sim` (`world::fire`, the firing and its trace) and `qs-workbench` (`map_logic`, the commands
`map.fire` and `map.logic_issues`).

Prior art: Hammer's entity I/O, whose `ent_fire` console command fires one entity's input in a
running game, and its Check for Problems; the model followed is the first, one entity fired and
what happened listed in order.

## FIRE-1 The firing

**Source:** this design; the engine calls an entity's `touch` with `self` the entity and `other`
what touched it and `time` the server's time (PHYS-3), and id's `SUB_UseTargets` calls a `use`
with `self` the target and `other` the entity firing it ([`../agent-reference/triggers.md`](../agent-reference/triggers.md)).

### Behaviour

- **FIRE-1.1** An entity is fired in a world of `qs-sim`: an edict in use, by its number. An edict
  not in use is refused.
- **FIRE-1.2** It is fired by its `use`, by its `touch`, or by either: its `use` when the field
  holds a function, else its `touch`.
- **FIRE-1.3** An entity with no function of the kind asked for is refused, the refusal naming
  the kind.
- **FIRE-1.4** The function runs as a callback does: the `time` global is the server's time,
  `self` the entity and `other` the player when a client is in the game, else the world. The
  program's own global `activator`, when it has one, is set to the same entity as `other`, as
  id's code sets it before it fires targets.

## FIRE-2 The trace

**Source:** this design.

### Behaviour

- **FIRE-2.1** A callback is recorded when a function is entered that is, at that moment, the
  `use`, `touch`, `think` or `blocked` of the entity `self` names, the fields checked in that
  order: its kind, the `time` global, the entity, its classname and the function's name. A
  function entered that is none of the four is not recorded, whoever calls it.
- **FIRE-2.2** Only what the firing reached is recorded. The run of the fired function is; after
  it, a run the engine starts is when `self` or `other` is an entity reached, the world never
  one. The entities reached are the fired one and each entity a recorded run wrote a field of or
  spawned; a freed edict is no longer reached. So the thinks of the map's other entities run and
  are not in the trace.
- **FIRE-2.3** A `find` (BUILTIN-8) whose field is `targetname` is recorded as a lookup: `self`
  then, the name looked for, and the entity found, or none when it found the world.
- **FIRE-2.4** An entity found by a lookup has answered nothing when, before the next lookup or
  the end of the run, the program read its `use`, which held no function, and read or took the
  address of no other field of it and entered no function with it as `self`. It is recorded
  with `self` at the lookup. This is what id's `SUB_UseTargets` does with a target that has no
  `use` (`if (self.use != SUB_Null) { if (self.use) self.use (); }`); a destination a
  teleporter looks up and reads, or an entity a `killtarget` looks up and removes, is not one.
- **FIRE-2.5** An entity a recorded run removes or spawns is recorded after the run's other
  events, in the order it happened, with the `time` global at the run's start. A removed
  entity's classname is the one it last had; a spawned one's is read at the end of the frame.
- **FIRE-2.6** A `centerprint` in a recorded run is recorded with its text, its strings joined,
  and a `sound` with its entity and its sample.
- **FIRE-2.7** After the firing's run, and after each frame, each entity reached and still in
  use is recorded with the fields written since the last such record, whoever wrote them, each
  once, with its value then and the server's time.

## FIRE-3 The end

**Source:** this design.

### Behaviour

- **FIRE-3.1** After the fired function returns the server's frames run, each of the frame time
  given, a scenario's when none is (SCN-1.3), until the length given has run: 10 seconds when
  none is, and never more than 300.
- **FIRE-3.2** Before each frame the trace ends as quiet when no entity reached and in use has a
  `nextthink` above 0.
- **FIRE-3.3** The trace ends when the program leaves the host a command, such as
  `changelevel`: the command is the end, and it is not run.
- **FIRE-3.4** The trace ends when a function fails, in the firing's run or a frame, with the
  error's text; the events before it stay.
- **FIRE-3.5** A trace reads no clock: the same world fired with the same options gives the same
  trace.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `FIRE_SECONDS` | 10 | This design: long enough for id's doors and plats to finish their move and wait |
| `FIRE_MAX_SECONDS` | 300 | This design |

## FIRE-4 The issues only running shows

**Source:** this design.

### Behaviour

- **FIRE-4.1** `map.logic_issues` spawns the open map with the project's program at a skill
  and in a kind of game, as `map.entities_removed_by_skill` does, brings the player into the
  game, and fires, each in a copy of that world, every entity of the map the game kept that has
  a key naming entities to fire (MAP-8.8), by either function (FIRE-1.2). An entity with
  neither function is not fired.
- **FIRE-4.2** An entity that answered nothing in a trace (FIRE-2.4) is reported once for the
  map, with the entity fired and the entity whose targets they were: firing it does nothing.
- **FIRE-4.3** An entity removed in a trace whose `targetname` a later lookup of the same trace
  looks for and does not find is reported: the chain removes what it then fires.
- **FIRE-4.4** An entity whose `use` ran more than `FIRE_LOOP_USES` times in one trace is
  reported: the chain comes back to it. A trace that ended in an error (FIRE-3.4) is reported
  with the entity fired and the error.
- **FIRE-4.5** An entity with a key naming entities to fire, that other entities of the map
  fire, is fired by its `use` as many times as there are entities that fire it, and then until
  a firing first looks up one of its own keys' names, `FIRE_COUNT_USES` times at most. When
  that firing is a later one than the entities that fire it give, it is reported with both
  counts: it fires its targets only at a firing the map cannot reach. This is id's
  `trigger_counter` with a `count` above the entities that target it, found without reading
  `count`.
- **FIRE-4.6** Each issue names entities of the map by their place in it; an entity the game
  spawned itself is not one, and an issue about one alone is not reported.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `FIRE_LOOP_USES` | 8 | This design |
| `FIRE_COUNT_USES` | 16 | This design |
| `FIRE_ISSUE_SECONDS` | 5 | This design: the length of each trace `map.logic_issues` takes |

## FIRE-5 The commands

**Source:** this design.

### Behaviour

- **FIRE-5.1** `map.fire` takes an entity of the open map by its place, a skill, a kind of game,
  which function fires it and a length, spawns the map as FIRE-4.1 says, fires the entity
  (FIRE-1) and answers the trace (FIRE-2, FIRE-3): each event with the place in the map of each
  entity it names, when it is one of the map's. An entity the game did not keep at that skill is
  refused.
- **FIRE-5.2** `map.fire` and `map.logic_issues` change nothing: not the map, not a game that is
  playing it. They need a project whose server program builds.
- **FIRE-5.3** The issues of `map.logic_issues` join `map.issues` once it has run, each with the
  entity it is about, until the map or the program changes.
