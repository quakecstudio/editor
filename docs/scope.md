# Scope

What QuakeC Studio v1 is, what it is not, where the MVP line falls and where everything past it
is tracked. It restates the [design document](design/design-doc.md) (Summary and goals, MVP
scope, Map editor) against the plan; if the two disagree, the design document wins and this page
is fixed. Requirement IDs (`GOAL-n`, `MVP-n`) are in
[`plan/requirements.yaml`](../plan/requirements.yaml).

`python3 plan/check.py` fails if an issue in the Post-MVP backlog (the plan's `LATER` milestone)
is not linked from this page, so moving an item out of the MVP means updating this page in the
same PR.

## Goals

From the design document's Summary and goals (`GOAL-1` to `GOAL-6`):

- A dialect-aware semantic engine (vanilla, FTEQCC, GMQCC; SSQC, CSQC, MenuQC) with fault-tolerant
  parsing, go-to-definition, references, rename and diagnostics, and our own compiler built on the
  same model, with no outside toolchain.
- Live preview: save, compile and see the change in a running engine, with breakpoints and entity
  inspection.
- A built-in agent harness whose tools are the same commands the UI uses, also exposed over MCP
  and a CLI.
- A built-in map editor and map compiler that share the QuakeC project, its entity definitions
  and the play loop.
- One Rust codebase, signed native builds for macOS (universal), Windows and Linux, and no crashes
  in shipped code.

Some goals are reached only after the MVP: GMQCC, CSQC and MenuQC are in the post-MVP backlog,
and the map editor is Phases 5 to 7.

## Non-goals for v1

From the design document's "Non-goals for v1" (`GOAL-7`). Nothing in the plan builds these for
v1; where a later issue exists it is linked.

| Non-goal | Status | Link |
| --- | --- | --- |
| Quake 2, Quake 3 and Half-Life map formats | Not planned. The map editor reads and writes Quake .map (standard and Valve 220) and writes BSP29, BSP2 and .lit only | [Design doc: Non-goals for v1](design/design-doc.md#summary-and-goals), [Map editor](design/design-doc.md#map-editor) |
| GMQCC-only syntax | Planned after the MVP | [L-10](https://github.com/quakecstudio/editor/issues/194) spec, [L-02](https://github.com/quakecstudio/editor/issues/186) implementation |
| A web/WASM build | Revisited after the UI framework decision | [L-07](https://github.com/quakecstudio/editor/issues/191), decided by the spike in [#10](https://github.com/quakecstudio/editor/issues/10) |
| Quake 2 or Quake 3 game code | Not planned. QuakeC Studio targets QuakeC (progs.dat, csprogs.dat, menu.dat) only | [Design doc: Non-goals for v1](design/design-doc.md#summary-and-goals) |

## MVP

The MVP is done when a modder can open the rerelease QuakeC, ask the agent for a new weapon,
review the diff and see it fire in a live game on macOS, Windows and Linux (`MVP-1`). It closes
with the Phase 4 gate, [#136](https://github.com/quakecstudio/editor/issues/136). The In list
below is confirmed, item by item, in [#21](https://github.com/quakecstudio/editor/issues/21);
anything that issue answers no to moves to the post-MVP backlog and onto this page.

### In (`MVP-4`)

| Item | Phase |
| --- | --- |
| Semantic core for the vanilla and FTEQCC dialects, SSQC module only | [Phase 2](https://github.com/quakecstudio/editor/milestone/3) |
| Editor with navigation, diagnostics, completion and hover docs | [Phase 2](https://github.com/quakecstudio/editor/milestone/3) |
| Our own compiler for vanilla plus the FTEQCC features the rerelease uses; no outside toolchain | [Phase 2](https://github.com/quakecstudio/editor/milestone/3) |
| Preview host with reload-on-save, breakpoints and an entity inspector; FTEQW adapter on Windows and Linux | [Phase 3](https://github.com/quakecstudio/editor/milestone/4) |
| Agent panel with diff review, checkpoints and the agent tools; MCP server | [Phase 4](https://github.com/quakecstudio/editor/milestone/5) |
| Signed builds for macOS, Windows and Linux | [Phase 4](https://github.com/quakecstudio/editor/milestone/5) |
| Export a mod as a zip players can install: progs.dat, maps and the project's own assets, optionally in a PAK, with a README and license notices, never id's game data | [Phase 4](https://github.com/quakecstudio/editor/milestone/5) |

### Out

Everything in the design document's "Out (later)" list, each with its issue in the next section:

- GMQCC dialect, CSQC and MenuQC modules.
- Compiler optimizations beyond constant folding.
- DarkPlaces adapter, embedded game view, web build, and the map editor (Phases 5 to 7).

## Post-MVP

Every issue in the [Post-MVP backlog](https://github.com/quakecstudio/editor/milestone/9)
milestone (`MVP-3`). All of them wait for the MVP gate.

| Item | Design doc | Issues |
| --- | --- | --- |
| GMQCC dialect | Non-goals for v1; MVP scope, Out | [L-10](https://github.com/quakecstudio/editor/issues/194) Spec: GMQCC dialect; [L-02](https://github.com/quakecstudio/editor/issues/186) GMQCC dialect |
| CSQC and MenuQC modules | MVP scope, Out | [L-03](https://github.com/quakecstudio/editor/issues/187) Spec: CSQC and MenuQC modules; [L-09](https://github.com/quakecstudio/editor/issues/193) Implement CSQC and MenuQC modules |
| The rest of FTEQCC, beyond the features the rerelease uses | Compiler, Dialects in order | [L-01](https://github.com/quakecstudio/editor/issues/185) Spec: FTEQCC code generation beyond the rerelease subset; [L-08](https://github.com/quakecstudio/editor/issues/192) Generate code for full FTEQCC |
| Compiler optimizations beyond constant folding | MVP scope, Out; Compiler | [L-04](https://github.com/quakecstudio/editor/issues/188) Dead-code removal and string and global deduplication |
| DarkPlaces adapter | MVP scope, Out | [L-05](https://github.com/quakecstudio/editor/issues/189) DarkPlaces adapter |
| Embedded game view | MVP scope, Out; Engine bridge, Docking | [L-06](https://github.com/quakecstudio/editor/issues/190) Embedded game view |
| Web build | Non-goals for v1; MVP scope, Out | [L-07](https://github.com/quakecstudio/editor/issues/191) Web build |
| ericw-tools class advanced lighting | Map editor, Our own map compiler | [L-12](https://github.com/quakecstudio/editor/issues/195) Advanced lighting (ericw-tools class) |
| Next-edit suggestions | Editor and UI; moved out of the MVP by [ADR-0007](adr/0007-mvp-scope.md) | [P4-11](https://github.com/quakecstudio/editor/issues/122) Next-edit suggestions |
| External agents in the panel over ACP | Editor and UI, Product principles; moved out of the MVP by [ADR-0007](adr/0007-mvp-scope.md) | [P4-14](https://github.com/quakecstudio/editor/issues/125) Host external agents over ACP |
| In-app update check | Quality, testing and release; moved out of the MVP by [ADR-0007](adr/0007-mvp-scope.md) | [P4-19](https://github.com/quakecstudio/editor/issues/130) Update mechanism |

### The map editor, Phases 5 to 7

The map editor is post-MVP but planned as phases of its own rather than backlog items (design
document, "Phasing after the MVP"):

| Phase | Work | Gate |
| --- | --- | --- |
| [Phase 5: map entity editing](https://github.com/quakecstudio/editor/milestone/6) | Entity editing in existing maps, with QuakeC-derived definitions and live entity updates | [#150](https://github.com/quakecstudio/editor/issues/150) |
| [Phase 6: brushes and map compiler](https://github.com/quakecstudio/editor/milestone/7) | Brush editing and the map compiler (bsp, fast light, vis) | [#171](https://github.com/quakecstudio/editor/issues/171) |
| [Phase 7: lighting, region compile and performance](https://github.com/quakecstudio/editor/milestone/8) | Full lighting, region compile and compile performance | [#181](https://github.com/quakecstudio/editor/issues/181) |
