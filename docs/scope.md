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
- One Rust codebase, signed native builds for macOS (Apple silicon), Windows and Linux, and no crashes
  in shipped code.

Some goals are reached only after the MVP: GMQCC, CSQC and MenuQC are in the post-MVP backlog,
and the map editor is Phases 5 to 7.

## Non-goals for v1

From the design document's "Non-goals for v1" (`GOAL-7`). Nothing in the plan builds these for
v1; where a later issue exists it is linked.

| Non-goal | Status | Link |
| --- | --- | --- |
| Quake 2 and Quake 3 map formats, and writing Half-Life's | Not planned. The map editor reads and writes Quake .map (standard and Valve 220) and writes BSP29, BSP2 and .lit only. Reading Half-Life's BSP version 30 and WAD3 is planned for Phase 5, after the MVP (MAP-24) | [Design doc: Non-goals for v1](design/design-doc.md#summary-and-goals), [Map editor](design/design-doc.md#map-editor) |
| GMQCC-only syntax | Planned after the MVP | [L-10](https://github.com/quakecstudio/editor/issues/194) spec, [L-02](https://github.com/quakecstudio/editor/issues/186) implementation |
| A web/WASM build | Planned after the MVP; the UI framework, egui, supports it ([ADR-0003](adr/0003-ui-framework.md)) | [L-07](https://github.com/quakecstudio/editor/issues/191) |
| Japanese and Chinese input through a real input method, recorded | After the MVP; the editor takes IME events and is tested with synthetic ones (UI-4) | [P2-151](https://github.com/quakecstudio/editor/issues/644) |
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
| Preview host with reload-on-save, breakpoints and an entity inspector | [Phase 3](https://github.com/quakecstudio/editor/milestone/4) |
| Agent panel with diff review, checkpoints and the agent tools; MCP server | [Phase 4](https://github.com/quakecstudio/editor/milestone/5) |
| Signed builds for macOS, Windows and Linux | [Phase 4](https://github.com/quakecstudio/editor/milestone/5) |
| Export a mod as a zip players can install: progs.dat, maps and the project's own assets, optionally in a PAK, with a README and license notices, never id's game data | [Phase 4](https://github.com/quakecstudio/editor/milestone/5) |

### Out

Everything in the design document's "Out (later)" list, each with its issue in the next section:

- GMQCC dialect, CSQC and MenuQC modules.
- Compiler optimizations beyond constant folding as FTEQCC does it, always on, with no flag.
- Embedded game view, web build, and the map editor (Phases 5 to 7).

## Post-MVP

Every issue in the [Post-MVP backlog](https://github.com/quakecstudio/editor/milestone/9)
milestone (`MVP-3`). All of them wait for the MVP gate.

| Item | Design doc | Issues |
| --- | --- | --- |
| GMQCC dialect | Non-goals for v1; MVP scope, Out | [L-10](https://github.com/quakecstudio/editor/issues/194) Spec: GMQCC dialect; [L-02](https://github.com/quakecstudio/editor/issues/186) GMQCC dialect |
| CSQC and MenuQC modules | MVP scope, Out | [L-03](https://github.com/quakecstudio/editor/issues/187) Spec: CSQC and MenuQC modules; [L-09](https://github.com/quakecstudio/editor/issues/193) Implement CSQC and MenuQC modules |
| The rest of FTEQCC, beyond the features the rerelease uses | Compiler, Dialects in order | [L-01](https://github.com/quakecstudio/editor/issues/185) Spec: FTEQCC code generation beyond the rerelease subset; [L-08](https://github.com/quakecstudio/editor/issues/192) Generate code for full FTEQCC |
| Compiler optimizations beyond constant folding as FTEQCC does it, always on, with no flag | MVP scope, Out; Compiler | [L-04](https://github.com/quakecstudio/editor/issues/188) Constant folding, dead-code removal and string and global deduplication |
| Embedded game view | MVP scope, Out; Engine bridge, Docking; in the IDE's process by [ADR-0028](adr/0028-embedded-game-view.md), Justin, 2026-11-16 | [L-06](https://github.com/quakecstudio/editor/issues/190) Run the preview host's game loop from a library both apps use; [L-26](https://github.com/quakecstudio/editor/issues/1001) Embedded game view |
| Web build | Non-goals for v1; MVP scope, Out | [L-07](https://github.com/quakecstudio/editor/issues/191) Web build |
| ericw-tools class advanced lighting | Map editor, Our own map compiler | [L-12](https://github.com/quakecstudio/editor/issues/195) Advanced lighting (ericw-tools class) |
| Later engines' improvements in the preview host | Engine bridge; [ADR-0004](adr/0004-preview-host-base.md) | [L-13](https://github.com/quakecstudio/editor/issues/449) Adopt later engines' improvements in the preview host |
| The compiler differential on a world with movement and collision | Compiler, Testing; split from [P3-33](https://github.com/quakecstudio/editor/issues/689) | [L-15](https://github.com/quakecstudio/editor/issues/737) Run the compiler differential on qs-sim's world |
| The nightly Differential's per-function jobs within the runners' six hours | Testing; moved out of Phase 3 by Justin on 2026-11-10, after the gate review | [P3-43](https://github.com/quakecstudio/editor/issues/784) Make the nightly Differential finish within the runners' six hours |
| Next-edit suggestions | Editor and UI; moved out of the MVP by [ADR-0007](adr/0007-mvp-scope.md) | [P4-11](https://github.com/quakecstudio/editor/issues/122) Next-edit suggestions |
| External agents in the panel over ACP | Editor and UI, Product principles; moved out of the MVP by [ADR-0007](adr/0007-mvp-scope.md) | [P4-14](https://github.com/quakecstudio/editor/issues/125) Host external agents over ACP |
| Windows and Linux desktop checks | MVP scope; moved out of P4-24 and P4-23 by Justin on 2026-11-11, who has no Windows or Linux desktop, and the P4 gate's Windows and Linux demo on 2026-11-16 | [L-16](https://github.com/quakecstudio/editor/issues/851) Windows and Linux desktop checks |
| A new user's usability test of the guide | MVP scope (QA-8); moved out of Phase 4 by Justin on 2026-11-16: the guide is written, and no one is recording a test now | [P4-20](https://github.com/quakecstudio/editor/issues/131) User documentation |
| Rust 1.100 and its new APIs | Conventions (CONV-1); asked for by Justin on 2026-11-14 | [L-17](https://github.com/quakecstudio/editor/issues/956) Upgrade the workspace to Rust 1.100 |
| A performance audit of every crate | Performance (PRIN-5, TEST-8); asked for by Justin on 2026-11-15 | [L-18](https://github.com/quakecstudio/editor/issues/966) Audit each crate's performance, one crate at a time |
| The VoiceOver pass | Accessibility (QA-9); moved out of P7-12 by Justin on 2026-11-15 | [L-19](https://github.com/quakecstudio/editor/issues/967) VoiceOver pass over the app |
| A shortcut for every command and an AutoCAD-style command line | Editor and UI (UI-23); asked for by Justin on 2026-11-15 | [L-20](https://github.com/quakecstudio/editor/issues/971) A keyboard shortcut for every command, and a command line to drive them |
| The later engines' client-side improvements | Preview (ENG-2); adopted by [ADR-0026](adr/0026-preview-host-later-engine-improvements.md), Justin, 2026-11-15 | [L-21](https://github.com/quakecstudio/editor/issues/978) Port the later engines' client-side improvements ADR-0026 adopts |
| ericw-tools' sunlight, sky dome, surface lights, minimum light and phong | Map editor (MAP-21); scheduled by [ADR-0027](adr/0027-advanced-lighting.md), Justin, 2026-11-15 | [L-22](https://github.com/quakecstudio/editor/issues/979) Port ericw-tools' sunlight, sky dome, surface lights, minimum light and phong |
| ericw-tools' dirt and bounce | Map editor (MAP-21); scheduled by [ADR-0027](adr/0027-advanced-lighting.md), Justin, 2026-11-15 | [L-23](https://github.com/quakecstudio/editor/issues/980) Port ericw-tools' dirt and bounce lighting |
| Side panels with icons rather than text | Editor and UI (UI-2); asked for by Justin on 2026-11-15 | [L-24](https://github.com/quakecstudio/editor/issues/981) Make the side panels less text heavy |
| A build that reads `#pragma target` as the check does | Compiler (COMP-2); found by #187's work on 2026-11-16 | [L-25](https://github.com/quakecstudio/editor/issues/993) A build ignores #pragma target, which the check reads |
| A Welcome page on first launch and the app's own icon | Editor and UI (PLAT-8, UI-14); asked for by Justin on 2026-11-20 | [L-27](https://github.com/quakecstudio/editor/issues/1053) Show a Welcome page on first launch and the app's own icon |
| One New Project entry that asks which game in plain words | Editor and UI (UI-14, PLAT-8); asked for by Justin on 2026-11-20 | [L-28](https://github.com/quakecstudio/editor/issues/1055) Ask which game a new project is for once, in plain words |
| Demos recorded and played in the preview host, in the server's protocol | Preview (ENG-2); split from L-21 by Justin on 2026-11-20 | [L-29](https://github.com/quakecstudio/editor/issues/1059) Record and play demos in the preview host |
| The later engines' options as console variables | Preview (ENG-2); chosen by Justin on 2026-11-20 | [L-30](https://github.com/quakecstudio/editor/issues/1061) Set the later engines' options from the console, by their upstream names |
| The startup configuration before the first map, and `sv_protocol` as QuakeSpasm's command | Preview (ENG-2); chosen by Justin on 2026-11-20 | [L-31](https://github.com/quakecstudio/editor/issues/1068) Run the startup configuration before the first map, and sv_protocol as QuakeSpasm's command |
| Map tools an agent can build and review a map with | Map editor (MAP-16), PRIN-6; asked for by Justin on 2026-11-20 | [L-40](https://github.com/quakecstudio/editor/issues/1066) An agent builds and reviews a map without guessing |
| What a map review cannot see in one frame: darkness, sleeping monsters, the brushes a tool made | Map editor (MAP-16); found by L-40's trial on 2026-11-20 | [L-41](https://github.com/quakecstudio/editor/issues/1067) Tell an agent what a map review cannot see in one frame |
| Command names an agent's tool search finds | Agent harness (PRIN-3, PRIN-6); asked for by Justin on 2026-11-20 | [L-42](https://github.com/quakecstudio/editor/issues/1069) Name the commands an agent finds by name for what they do |
| An agent's reply drawn as Markdown, and an outside agent's map edits applied at once | Agent harness (UI-11, PRIN-6); found by a trial and decided by Justin on 2026-11-20 | [L-43](https://github.com/quakecstudio/editor/issues/1074) Show the agent's reply as Markdown and apply an outside agent's map edits at once |
| Status bar counts that open Problems, and a command line that lists its commands | Interface (UI-23, UI-14); asked for by Justin on 2026-11-20 | [L-44](https://github.com/quakecstudio/editor/issues/1076) Make the status bar's counts open Problems and the command line list its commands |
| What an outside agent needs to write and debug QuakeC | Agent harness (PRIN-6, AGT-9); asked for by Justin on 2026-11-20 | [L-45](https://github.com/quakecstudio/editor/issues/1077) Give an outside agent what it needs to write and debug QuakeC |
| The rest of the agent's QuakeC reference: the language by dialect, weapons, items and triggers | Agent harness (PRIN-6, AGT-9); what L-45 left of Justin's ask of 2026-11-20 | [L-46](https://github.com/quakecstudio/editor/issues/1080) Write the rest of the agent's QuakeC reference |
| Models imported, written and changed with commands | Agent harness (PRIN-6), MAP-4; asked for by Justin on 2026-11-20 | [L-47](https://github.com/quakecstudio/editor/issues/1081) Import, write and change models with commands |
| A random playable map from one button | Map editor (MAP-3, MAP-16), PRIN-6; asked for by Justin on 2026-11-20 | [L-48](https://github.com/quakecstudio/editor/issues/1082) Generate a random playable map from one button |
| The room shapes of id's maps for an agent to build with | Map editor (MAP-16), PRIN-6; asked for by Justin on 2026-11-20 | [L-49](https://github.com/quakecstudio/editor/issues/1083) Give an agent the room shapes of id's maps to build with |
| Generated levels on several floors, with loops, puzzles and no trap | Map editor (MAP-3, MAP-16), PRIN-6; asked for by Justin on 2026-11-23 | [L-74](https://github.com/quakecstudio/editor/issues/1127) Generate levels on several floors with loops, puzzles and no trap the player cannot leave |
| An entity fired in the editor, with a trace of what the QuakeC did | Map editor (MAP-35, MAP-16), PRIN-1; asked for by Justin on 2026-11-25 | [L-76](https://github.com/quakecstudio/editor/issues/1135) Fire a map's triggers in the editor and trace what the QuakeC does |
| A hand-built map checked for play: the exit, items and secrets reached, and no trap | Map editor (MAP-36, MAP-7); asked for by Justin on 2026-11-25 | [L-77](https://github.com/quakecstudio/editor/issues/1136) Check a hand-built map can be finished, with nothing out of reach and no trap |
| A balance sheet of a map on each skill | Map editor (MAP-37, MAP-4); asked for by Justin on 2026-11-25 | [L-78](https://github.com/quakecstudio/editor/issues/1137) Show a map's monsters, ammo, health and armour on each skill |
| How much of the target engine's limits a map uses | Map editor (MAP-38, MAP-6), PRIN-8; asked for by Justin on 2026-11-25 | [L-79](https://github.com/quakecstudio/editor/issues/1138) Show how much of the target engine's limits a map uses |
| Every control self-evident, as Don't Make Me Think asks | Editor and UI (UI-24, UI-14), Map editor (MAP-27); asked for by Justin on 2026-11-25 | [L-80](https://github.com/quakecstudio/editor/issues/1142) Make every panel self-evident, so that no control makes the modder think |
| The semantic model overflowing its stack on some input | Language (LANG-4), PRIN-7; found by CI on PR 1146 on 2026-11-27 | [L-81](https://github.com/quakecstudio/editor/issues/1147) The semantic model overflows its stack on some random input |
| The project's own tooling and its documentation kept accurate | QA-1, CONV-5; asked for by Justin on 2026-11-27 | [L-82](https://github.com/quakecstudio/editor/issues/1148) Keep the project's own tooling and its documentation accurate |
| Known values offered beyond the map Properties panel: the debugger, the running game, map keys and settings | Editor and UI (UI-24), Map editor (MAP-27); asked for by Justin on 2026-11-27 | [L-83](https://github.com/quakecstudio/editor/issues/1150) Offer known values in the debugger, the running game, map keys and settings |
| Binary fbx models, and fbx facing as obj does | Agent harness (PRIN-6), MAP-4; asked for by Justin on 2026-11-21 | [L-50](https://github.com/quakecstudio/editor/issues/1089) Import binary fbx models, facing the way obj models do |
| Rigged fbx models from Blender posed as Blender poses them | Agent harness (PRIN-6), MAP-4; found by comparing with Blender on 2026-11-21 | [L-51](https://github.com/quakecstudio/editor/issues/1091) A rigged fbx from Blender imports with its animation's poses wrong |
| Play plays the map open in the Map tab | Map editor (MAP-13), UI-14; asked for by Justin on 2026-11-21 | [L-52](https://github.com/quakecstudio/editor/issues/1092) Play plays the map open in the Map tab |
| Properties shows the game's values while paused | UI-13; found by the showcase on 2026-11-21 | [L-53](https://github.com/quakecstudio/editor/issues/1096) Properties shows an entity's spawn-time fields while the game is paused |
| The first shotgun shot always takes health | ENG-2; found by the showcase on 2026-11-21 | [L-54](https://github.com/quakecstudio/editor/issues/1097) The first shotgun shot sometimes draws blood without taking health |
| A local struct given a brace initialiser compiles | COMP-3; found writing the agent reference on 2026-11-21 | [L-55](https://github.com/quakecstudio/editor/issues/1101) A local struct with a brace initialiser stops the compiler |
| The debugging tools hold up over MCP | ENG-2; reported by the agent runs of L-46 on 2026-11-21 | [L-56](https://github.com/quakecstudio/editor/issues/1102) Debugging a played game over MCP closes the host or reads stale values |
| `return = e` and arrays of more than one dimension compile | COMP-3; found reviewing the agent reference on 2026-11-21 | [L-57](https://github.com/quakecstudio/editor/issues/1104) FTEQCC: return = e returns nothing, and a multi-dimensional array's element does not parse |
| A build that stops on the project graph's errors | COMP-2; found playtesting for v2026.11.2 on 2026-11-22 | [L-58](https://github.com/quakecstudio/editor/issues/1108) `build` writes a progs.dat when progs.src names a missing file |
| A manifest module whose source is missing says so | GOAL-4; found playtesting for v2026.11.2 on 2026-11-22 | [L-59](https://github.com/quakecstudio/editor/issues/1109) A quakec.toml module with a missing src reports that there is no quakec.toml |
| A reversed range is told apart from one past the end | PRIN-3; found playtesting for v2026.11.2 on 2026-11-22 | [L-60](https://github.com/quakecstudio/editor/issues/1110) file.replace_range reports a reversed range as past the end of the file |
| A project built from inside its directory is named after it | PLAT-6; found playtesting for v2026.11.2 on 2026-11-22 | [L-61](https://github.com/quakecstudio/editor/issues/1111) quakec-ide build in a project directory writes build/mod, not build/<project> |
| One error for a statement nested past the limit | GOAL-1; found playtesting for v2026.11.2 on 2026-11-22 | [L-62](https://github.com/quakecstudio/editor/issues/1112) Statements nested past the limit report nesting too deep thousands of times |
| A program past a progs.dat limit says which and where | GOAL-2; found playtesting for v2026.11.2 on 2026-11-22 | [L-63](https://github.com/quakecstudio/editor/issues/1113) A program past a progs.dat table limit stops the compiler with an internal error |
| A file a list-form progs.src includes is compiled | GOAL-1; found playtesting for v2026.11.2 on 2026-11-22 | [L-64](https://github.com/quakecstudio/editor/issues/1114) A file included from a list-form progs.src is never compiled |
| A read-only MCP server says it is read only | AGT-7; found playtesting for v2026.11.2 on 2026-11-22 | [L-65](https://github.com/quakecstudio/editor/issues/1115) A read-only MCP server says the project is outside its roots |
| A level that fails to load says why and where | ENG-4; found playtesting for v2026.11.2 on 2026-11-22 | [L-66](https://github.com/quakecstudio/editor/issues/1116) A QuakeC error while a level loads reaches the caller only as exit status 1 |
| A run error's message in preview.poll | ENG-4; found playtesting for v2026.11.2 on 2026-11-22 | [L-67](https://github.com/quakecstudio/editor/issues/1117) preview.poll leaves out a run error's message and stack |
| An empty classname is refused and reported as the game makes it | MAP-7; found playtesting for v2026.11.2 on 2026-11-22 | [L-68](https://github.com/quakecstudio/editor/issues/1118) An entity with an empty classname passes map.issues and stops the level |
| A preview host that stops answering is shown and stopped at once | ENG-1; found playtesting for v2026.11.2 on 2026-11-22 | [L-69](https://github.com/quakecstudio/editor/issues/1119) A preview host that stops answering is still shown as playing |
| A breakpoint the game refuses is not kept | ENG-4; found playtesting for v2026.11.2 on 2026-11-22 | [L-70](https://github.com/quakecstudio/editor/issues/1120) A breakpoint the preview host refuses stays in the list and the next toggle removes it |
| A run error's stack in preview.poll | ENG-4; found in the follow-up playtest of v2026.11.2 on 2026-11-22 | [L-71](https://github.com/quakecstudio/editor/issues/1122) A game stopped at a run error answers no stack |
| The toolbar says a host that does not answer is not answering | ENG-1, PRIN-8; found in the follow-up playtest of v2026.11.2 on 2026-11-22 | [L-72](https://github.com/quakecstudio/editor/issues/1123) The toolbar shows a host that does not answer as playing |
| The game popped out of the Game tab to full screen | Preview (ENG-10, ENG-9); asked for by Justin on 2026-11-22 | [L-73](https://github.com/quakecstudio/editor/issues/1124) Pop the game out of the Game tab into a full-screen window |
| The Windows MSI signed, not only the executables in it | Quality, testing and release (QA-6); asked for by Justin on 2026-11-24 | [L-75](https://github.com/quakecstudio/editor/issues/1132) Sign the Windows MSI as well as the executables in it |
| In-app update check | Quality, testing and release; moved out of the MVP by [ADR-0007](adr/0007-mvp-scope.md) | [P4-19](https://github.com/quakecstudio/editor/issues/130) Update check that names the install's own way to update |
| Editor integrations for VS Code, Zed and Neovim | Language server; moved out of Phase 2 by Justin on 2026-10-29, as it serves other editors and not the IDE or the engine | [P2-16](https://github.com/quakecstudio/editor/issues/72) Package editor integrations for VS Code, Zed and Neovim |

### The map editor, Phases 5 to 7

The map editor is post-MVP but planned as phases of its own rather than backlog items (design
document, "Phasing after the MVP"):

| Phase | Work | Gate |
| --- | --- | --- |
| [Phase 5: map entity editing](https://github.com/quakecstudio/editor/milestone/6) | Entity editing in existing maps, with QuakeC-derived definitions and live entity updates | [#150](https://github.com/quakecstudio/editor/issues/150) |
| [Phase 6: brushes and map compiler](https://github.com/quakecstudio/editor/milestone/7) | Brush editing and the map compiler (bsp, fast light, vis) | [#171](https://github.com/quakecstudio/editor/issues/171) |
| [Phase 7: lighting, region compile and performance](https://github.com/quakecstudio/editor/milestone/8) | Full lighting, region compile and compile performance | [#181](https://github.com/quakecstudio/editor/issues/181) |
