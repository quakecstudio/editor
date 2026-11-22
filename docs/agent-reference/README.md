# Agent reference

The reference QuakeC Studio's agent reads on every run (P4-08, AGT-3): `qs-agent` builds it
into the system prompt, after the rules in `qs_agent::SYSTEM`. It is written for the agent, in
our own words, and states only what this repository's specs or id's source say; each section
names its source. Keep it short: every word of it is read on every turn.

- [`quakec.md`](quakec.md): the language, how a mod is laid out, and the idioms of id's code.
- [`workflow.md`](workflow.md): how to work in QuakeC Studio: tools, tests and scenarios.
- [`maps.md`](maps.md): the `.map` format, units and the player's limits, the entities a map
  needs, textures and leaks.

More are read when the task needs them, through `project.reference`, which serves every
topic here to any agent, the built-in one and one that reaches the IDE over MCP (L-45, L-46):

- [`language-vanilla.md`](language-vanilla.md), [`language-rerelease.md`](language-rerelease.md)
  and [`language-fteqcc.md`](language-fteqcc.md): each dialect the compiler takes, in full,
  from its spec, with the mistakes a model makes in it (topics `vanilla`, `rerelease` and
  `fteqcc`). `the_dialect_topics_hold_when_run` (`qs-workbench`) runs what the first two
  say of the operators, conditions and calls, and the `fteqcc` topic's constructs that
  compile for id's VM.

- [`entities.md`](entities.md): what the engine calls, how an entity moves and collides, the
  builtins by what they are for, what the language does that C does not, and debugging.
- [`monsters.md`](monsters.md): how a monster is written, from id's soldier, and every place
  a new one must be added.
- [`weapons.md`](weapons.md), [`items.md`](items.md) and [`triggers.md`](triggers.md): how
  each is written, from id's QuakeC 1.01, and where a new one must be added.
- [`models.md`](models.md): what a model is, a mod's own models, and how to recolour, scale
  and reframe one (L-47).

Per run, the agent is also given:

- the project's own `AGENTS.md`, when it has one, which the modder writes for their mod;
- each module's kind, dialect and target engine (`project.info`), and that engine's builtins
  (`engine.builtins`).
- the open map, when there is one: its path, its WADs and its entities' classes.
