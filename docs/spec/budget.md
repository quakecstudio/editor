# A map's budget

Prefix: `BUDGET` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how much of the target engine's limits the open map uses (L-79, MAP-38): each limit
of [`engine-targets.md`](engine-targets.md) ENGT-5, what the map uses of it and the share, for
the project's target engine and for id's, and the issues it gives `map.issues`. It is this
project's design: the limits are the engines' (ENGT-5), and what the map uses is measured in
the BSP a compile writes and in the map spawned by the game's own code, never estimated from
the `.map`. Implemented by `qs-workbench` (`map_budget`, the command `map.budget`).

Prior art: Hammer's Map, Show Information, which lists the counts of a compiled map against
the compiler's limits; the counts qbsp, vis and light print at the end of a run; and the
QuakeSpasm lineage's developer warnings when a lump is past id's limit (ENGT-5.8). The model
followed is Hammer's table, made for the engine the project targets and kept beside the
compile.

## BUDGET-1 The command

**Source:** this design.

### Behaviour

- **BUDGET-1.1** `map.budget` is a command of the registry that reads and changes nothing. It
  needs an open map, takes `answer` and refuses any other parameter. With `answer` `now`, the
  default, it answers the budget; with `later` it answers null at once and makes the spawns
  on their own thread, and `map.budget_found` answers the budget once it is made, then the
  one kept (BUDGET-3.3) while it is current, and null while one is being made or none is
  kept. A budget made later is the one made at once.
- **BUDGET-1.2** It answers for the project's target engine (ENGT-3.5, ENGT-3.6), and then for
  id's, Vanilla, when the target is another; with no project open, for Vanilla alone. For each
  engine it gives whether it reads BSP2 (ENGT-5.6) and a line for each limit of ENGT-5.4: the
  limit, what the map uses, the engine's bound for the map's format (ENGT-5.7) with what the
  engine does past it and where the bound is stated, the share, and its standing.
- **BUDGET-1.3** The share is what the map uses over the bound. The standing is past when it
  uses more than the bound, near when the share is at least `BUDGET_NEAR_SHARE` (0.9), within
  otherwise or when the engine sets no bound (ENGT-5.7), and unknown when the bound is unknown
  or the use is not measured.
- **BUDGET-1.4** The lines are in order of standing, past first, then near, within and
  unknown; past, near and within lines with a share by the share, largest first; lines of one
  standing and share in the order of ENGT-5.4.

## BUDGET-2 What the map uses

**Source:** this design; the counts are ENGT-5.4's.

### Behaviour

- **BUDGET-2.1** The lumps, `visleafs` and the face extent are those of the last compile's
  report (`map.compile`, `map.find_leaks`) while the map has not been edited since; of the BSP
  itself for a map opened from one, but for the entities and their text, which are those the
  editor holds, as the spawn reads them; and not measured before a compile. A compile neither
  lights nor vises the map, so its lighting and visibility are not measured. The face extent
  is the one id's rule gives for Vanilla and the FitzQuake lineage's for the other engines,
  read as an unsigned 16-bit value, so an extent that wraps the engine's `short` counts as
  what it is; a BSP opened is measured by its loader's rule for both.
- **BUDGET-2.2** The format is the compile's: BSP2 when a lump was too wide for BSP29
  (QBSP-14.4), BSP29 otherwise, and the BSP's own for a map opened from one; BSP29 before a
  compile.
- **BUDGET-2.3** The edicts, precached models, precached sounds and light styles are measured
  by spawning the map with the project's server program as `map.entities_removed_by_skill`
  spawns it, on each skill 0 to 3 in single player, and running the two frames that settle a
  started game: the edicts are `num_edicts` (EDICT-1.4), the world and the client slots
  among them; the precaches each list's names after its empty entry 0 (LIMIT-6.2), the world
  and its submodels among the models; the light styles the styles the program set
  (BUILTIN-12.6), or, for an engine whose bound one more than the highest style set passes,
  that, since the bound is on a style's number. Each is the most of the four skills, with
  the first skill that uses that most.
- **BUDGET-2.4** The spawn has room past the bounds of the engines answered for (EDICT-1.10,
  LIMIT-6.3, LIMIT-6.4), id's among them: one more edict and one more light style than the
  largest bound, and each precache list one more name than the largest bound besides its
  empty entry 0. So a map past id's limit and inside the target's is counted exactly. A
  skill's spawn that runs out of that room, ED_Alloc's `no free edicts`, a full precache list
  or a light style past the array, counts that limit as at least one more than the room held,
  past every bound answered for, and counts nothing else on that skill; a limit no skill
  counted is not measured, the reason naming the limit the spawns ran out of.
- **BUDGET-2.5** With no project, or a server program that does not build, or a spawn that
  fails on some skill other than by running out of room, the edicts, precaches and light
  styles are not measured, with the reason.

## BUDGET-3 Issues

**Source:** this design.

### Behaviour

- **BUDGET-3.1** Each line of the target engine that is past its bound is an issue of
  `map.issues`, for the whole map: `N <limit> is past <Engine>'s limit of M`, `N` being `at
  least N` for a spawn that ran out of room (BUDGET-2.4), and what the
  engine does (ENGT-5.5): `<Engine> refuses it`, `<Engine> loads it with a warning`, `id's
  compiler cannot write it`, or `<Engine> writes past its array, which is undefined`. Each
  near line is one too: `N <limit> is P% of <Engine>'s limit of M`, `P` the share rounded
  down to a whole percent.
- **BUDGET-3.2** A map the compile wrote as BSP2 is an issue naming the lump BSP29 could not
  hold: with a target that reads BSP2, `the <lump> are too many for BSP29, so the map is
  compiled as BSP2, which <Engine> reads`; with one that does not, `the <lump> are too many
  for BSP29, so the map is compiled as BSP2, which <Engine> does not read: it will not load`;
  with KEX, `..., and whether KEX reads BSP2 is not known`.
- **BUDGET-3.3** The issues and the budget are those of the last `map.budget`, kept while the
  map's text, the project's target engine and its sources are those it was made of; a change
  to any of them leaves no issue and no budget kept, as a change to the map or the sources
  does to `map.logic_issues`' (FIRE-5.3).
