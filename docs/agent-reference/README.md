# Agent reference

The reference QuakeC Studio's agent reads on every run (P4-08, AGT-3): `qs-agent` builds it
into the system prompt, after the rules in `qs_agent::SYSTEM`. It is written for the agent, in
our own words, and states only what this repository's specs or id's source say; each section
names its source. Keep it short: every word of it is read on every turn.

- [`quakec.md`](quakec.md): the language, how a mod is laid out, and the idioms of id's code.
- [`workflow.md`](workflow.md): how to work in QuakeC Studio: tools, tests and scenarios.
- [`maps.md`](maps.md): the `.map` format, units and the player's limits, the entities a map
  needs, textures and leaks.

Per run, the agent is also given:

- the project's own `AGENTS.md`, when it has one, which the modder writes for their mod;
- each module's kind, dialect and target engine (`project.info`), and that engine's builtins
  (`engine.builtins`).
- the open map, when there is one: its path, its WADs and its entities' classes.
