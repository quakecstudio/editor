# QuakeC Studio

A native, cross-platform IDE for QuakeC, the scripting language of id Software's original *Quake*, written in Rust. It has Zed's editor feel and Roblox Studio's play loop: edit code, press Play, and see the change in the running game in about a second. It ships its own QuakeC compiler (no outside toolchain), a built-in agent that edits, compiles and tests through the same commands as the UI (also exposed over MCP), and a map editor with its own map compiler.

![The v1 play loop: the editor beside the game, docked in its own window](docs/design/mockups/01-play-loop.png)

*Concept mockup of the approved design, not a screenshot.*

## Status

Phase 0 of 7: decisions and spikes. There is no code yet. Work is tracked as GitHub issues generated from [`plan/`](plan/README.md), on the [project board](https://github.com/orgs/quakecstudio/projects/1).

## Read first

| Document | What it covers |
| --- | --- |
| [`docs/roadmap.md`](docs/roadmap.md) | The overall plan: decisions so far, layers, the phases and their gates, where we are, how to work an issue |
| [`docs/design/design-doc.md`](docs/design/design-doc.md) | What we are building and why |
| [`docs/design/README.md`](docs/design/README.md) | The approved mockups and the design language |
| [`plan/README.md`](plan/README.md) | How the design becomes requirements, issues, phases and gates |
| [`docs/conventions.md`](docs/conventions.md) | Rust conventions (based on vouch) |
| [`docs/testing.md`](docs/testing.md) | The test-suite readiness bar a layer must pass before work above it starts |

## License

To be decided in [#8](https://github.com/quakecstudio/editor/issues/8): MIT OR Apache-2.0 for the IDE core with GPL tools kept out of process, or GPL for everything. Quake's game data is not included and never will be; you need your own copy of Quake.

QuakeC Studio is an independent project and is not affiliated with or endorsed by id Software, ZeniMax Media or Bethesda.
