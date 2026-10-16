# Roadmap

How QuakeC Studio gets built, from an empty repo to the MVP and then the map editor. The [design document](design/design-doc.md) says what we are building and why; the [plan](../plan/README.md) holds every requirement and issue as data; this page is the map between them. Read it before picking up an issue.

## What we are building

A native Rust IDE for QuakeC on macOS, Windows and Linux. A modder edits code, presses Play and sees the change in the running game in about a second; pauses the game, clicks an entity and inspects or edits its fields; asks the built-in agent for a change and reviews it as a diff before anything touches disk. The IDE compiles QuakeC itself, with no outside toolchain, and later edits and compiles maps too.

**The MVP** is done when a modder can open id's 2021 rerelease QuakeC, ask the agent for a new weapon, review the diff, and see it fire in a live game on all three operating systems, from signed builds.

## Decisions so far

| Decision | Where it lives |
| --- | --- |
| Rust, one codebase, signed native builds for macOS (universal), Windows and Linux | Design doc: Summary; GOAL-6 |
| Feel like Zed's editor with Roblox Studio's play loop, not like Cursor: the running game is the product, code serves it | Design doc: Product principles; UI-14 |
| Our own QuakeC compiler, no outside toolchain at runtime | Design doc: Compiler; GOAL-2 |
| The whole project is GPL-3.0-or-later | [ADR-0002](adr/0002-license-model.md); LIC-1 |
| Foundations ported from id's GPL Quake source, which is also the spec; ports keep id's notice and are documented in `docs/spec/`; id's original VM and qcc run as test oracles to prove the ports match | Design doc: Source and licensing; LIC-2, LIC-3; [#9](https://github.com/quakecstudio/editor/issues/9) |
| Live preview through our own preview host, a Rust engine ported from id's source on wgpu and our VM crates, proved against id's engine as an oracle, and the only engine the IDE runs; v1 docks the engine window beside the IDE, embedding comes later | [ADR-0004](adr/0004-preview-host-base.md), [ADR-0005](adr/0005-default-preview-target.md); Design doc: Engine bridge; ENG-1 to ENG-16 |
| The agent is a client of the same command registry as the UI and the CLI; exposed over MCP; external agents run in the panel over ACP | Design doc: Agent harness; PRIN-3, PRIN-6 |
| A map editor and our own map compiler, after the MVP (Phases 5 to 7) | Design doc: Map editor; MAP-18 |
| Rust conventions follow vouch | [`conventions.md`](conventions.md) |
| A layer's dependants wait until its test suite passes the readiness bar | [`testing.md`](testing.md) |
| The visual design and mockups | [`design/`](design/README.md); approved on [#199](https://github.com/quakecstudio/editor/issues/199) |

Phase 0's decisions are made: the UI framework is egui ([ADR-0003](adr/0003-ui-framework.md)), the preview host is our own Rust engine and the only one the IDE runs ([ADR-0004](adr/0004-preview-host-base.md), [ADR-0005](adr/0005-default-preview-target.md)), the name stays QuakeC Studio ([ADR-0006](adr/0006-product-name.md)), and the MVP scope and game directories are set ([ADR-0007](adr/0007-mvp-scope.md), [ADR-0008](adr/0008-game-directories.md)).

## How the code is layered

Each layer may depend only on layers below it; `cargo xtask layers` enforces this ([`layers.md`](layers.md)).

| Layer | What it holds |
| --- | --- |
| L0 formats | progs.dat, .lno, PAK and WAD2, .map, BSP29/BSP2, MDL |
| L1 VM | qc-vm, qc-edict, qc-builtins: what the engine executes |
| L2 syntax | Preprocessor, lossless CST, progs.src project graph |
| L3 semantics | Resolution, types, dialects, engine-target database, LSP, map entity definitions |
| L4 bridges | Our QuakeC and map compilers; the engine bridge |
| L5 commands | Command engine, session, undo: one registry for UI, CLI, agent and MCP |
| L6 agent | Agent loop, model providers, checkpoints, MCP and ACP |
| L7 UI | The app on the chosen framework |
| apps | Desktop app, CLI, LSP server |

The only C programs are the oracles in `oracles/` (id's VM trace harness, id's qcc, a pinned FTEQCC, id's engine as the engine oracle, later id's map tools and ericw-tools; built by `cargo xtask oracles`, CI only). The preview host is the Rust binary `apps/preview-host`, which runs as its own process beside the IDE ([ADR-0004](adr/0004-preview-host-base.md)).

## The phases

Phases run in order. Each ends with a gate review issue, and every issue in a phase depends on the previous gate. Inside a phase, an issue that builds on a lower layer also waits for that layer's readiness review.

```mermaid
flowchart LR
  P0["P0 decisions and spikes"] --> P1["P1 foundations"] --> P2["P2 language core and editor"] --> P3["P3 live preview"] --> P4["P4 agent and MVP"]
  P4 --> P5["P5 map entities"] --> P6["P6 brushes and map compiler"] --> P7["P7 lighting and performance"]
  P4 --> LATER["Post-MVP backlog"]
```

| Phase | What gets built | Gate |
| --- | --- | --- |
| **0. Decisions and spikes** | The repo, `cargo xtask ci`, CI on three OSes, the ADR and porting processes, the readiness tooling, the UI framework decision, the visual design (the license is decided) | UI framework, license model and visual design chosen; repo, CI and planning in place |
| **1. Foundations** | Specs and Rust ports of id's formats and VM: qc-types, qc-progs, qc-edict, qc-vm and qc-builtins; the oracles and a differential runner that compares our VM to id's statement by statement; PAK/WAD2, BSP, MDL, SPR, WAV and MD5 readers, and finding the player's game files (including on a Mac); the lexer, preprocessor and lossless parser, fuzzed | qc-vm matches id's VM on the id 1.01 and rerelease corpus; the parser round-trips it losslessly |
| **2. Language core and editor** | The incremental semantic model, dialects and engine-target database; our compiler (vanilla, then the rerelease's FTEQCC features) with differential and conformance suites; the command engine; CLI, LSP and tree-sitter grammar; the app shell, code editor, panels and command palette to the approved mockups | The rerelease compiles with our compiler and behaves identically in qc-vm; full navigation and clean diagnostics in the editor |
| **3. Live preview** | The bridge protocol; the Rust preview host (engine spec, server simulation, the wgpu renderer moved up from Phase 5, sound) proved against id's engine as an oracle, with hot reload, debugging and headless mode; the play toolbar, Entities, Properties, Debugger and Console; docking the engine window; deterministic scenarios | Save-to-game in about a second on all three OSes, with breakpoints and entity inspection |
| **4. Agent and MVP** | Agent tools as commands, staged edits and hunk review, checkpoints, the agent loop and providers, the agent panel, inline edit, MCP, evals; privacy audit, signing and packaging, mod export, docs, license compliance review | The MVP demo on three OSes; signed builds published |
| **5. Map entity editing** | Entity definitions derived from QuakeC, the map view with entity editing, live entity updates, agent map tools | Entities in existing maps are edited with QuakeC-derived definitions and appear live in the game |
| **6. Brushes and map compiler** | Specs and ports of id's tools as our qmap-bsp, qmap-vis and fast qmap-light, checked against id's tools and ericw-tools; brush tools, 2D views, texture browser and PNG texture import, leak diagnostics, play from here | A new map built from brushes compiles with our tools and plays, matching id's tools on the map corpus |
| **7. Lighting and performance** | Full coloured lighting, background full compile, region compile, compile performance | Lighting, region compile and background compile meet the performance budgets |
| **Post-MVP** | GMQCC, CSQC and MenuQC, full FTEQCC code generation, optimizations, the embedded game view, a web build, advanced lighting | Each item is scheduled into a phase by an ADR first |

The [project board](https://github.com/orgs/quakecstudio/projects/1) has the live state; its Roadmap view groups the issues by phase.

## Where we are

Phase 0. The plan, the issues, the board, the design document and the approved mockups exist; there is no code yet.

Two tracks run in parallel:

1. **Repo foundation:** [#1](https://github.com/quakecstudio/editor/issues/1) workspace scaffold, then [#2](https://github.com/quakecstudio/editor/issues/2) Makefile and xtask, [#3](https://github.com/quakecstudio/editor/issues/3) CI on three OSes, [#24](https://github.com/quakecstudio/editor/issues/24) the readiness tooling, [#7](https://github.com/quakecstudio/editor/issues/7) the ADR process.
2. **Decisions:** the license is decided ([ADR-0002](adr/0002-license-model.md)); next are [#15](https://github.com/quakecstudio/editor/issues/15) name, [#12](https://github.com/quakecstudio/editor/issues/12) preview host base, [#21](https://github.com/quakecstudio/editor/issues/21) MVP scope, [#14](https://github.com/quakecstudio/editor/issues/14) default preview target.

The largest risk in Phase 0 is the two-week UI framework spike ([#10](https://github.com/quakecstudio/editor/issues/10)).

## Working an issue

Agents use the `/work-issue` skill, and `/run-phase` under `/goal` to work a whole phase; see [`agents/README.md`](agents/README.md). By hand, the steps are the same:

1. Pick an open issue in the current phase whose dependencies are all closed.
2. Read its deliverable, acceptance criteria and verification, the requirements it cites in [`plan/requirements.yaml`](../plan/requirements.yaml), and the matching section of the design document.
3. When porting from id's source, keep id's copyright line, cite the file and function, and document the behaviour in `docs/spec/` (`docs/porting.md`, from #9).
4. Work on a branch, meet the definition of done in the issue, and open a PR with `Closes #<n>`. `main` merges only through the merge queue; an agent may add its own PR once CI passes, except for `needs:human` issues.
5. If the work shows the plan is wrong or incomplete, change `plan/` in a PR and file follow-up issues; do not leave it in comments.
