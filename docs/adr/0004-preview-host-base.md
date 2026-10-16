# ADR-0004: Build the preview host in Rust on wgpu

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** Justin Plock, 2026-10-15, [PR #221 comment](https://github.com/quakecstudio/editor/pull/221#issuecomment-6205708332)
- **Issue:** [#12](https://github.com/quakecstudio/editor/issues/12)
- **Requirements:** ENG-2, PLAT-3

## Question

What engine does the preview host run: a fork of a QuakeSpasm-lineage C engine (QuakeSpasm,
Ironwail or vkQuake), a new host ported from id's source to Rust, or one now and the other later?

## Context

The design document's Engine bridge section makes the preview host "a small GPL fork of a
QuakeSpasm-lineage engine with one added module, the IDE bridge" (ENG-2). The host is what makes
live preview the same on macOS, Windows and Linux (PLAT-3), because FTEQW's macOS support is
unverified ([#13](https://github.com/quakecstudio/editor/issues/13), risk R-6 in
[`docs/risks.md`](../risks.md)). The choice blocks the default preview target
([#14](https://github.com/quakecstudio/editor/issues/14)), the bridge protocol spec
([#89](https://github.com/quakecstudio/editor/issues/89), P3-01) and, through them, the host itself
([#90](https://github.com/quakecstudio/editor/issues/90), P3-02), hot reload (P3-03), debugging
(P3-04) and headless mode (P3-05). P3-02 asks for a fork whose changes outside `bridge/` are few
enough to list one by one in `PATCHES.md`.

On #13 Justin asked why the project should depend on FTEQW at all, and whether the engine should
also be ported to a native Rust engine on native GPU APIs such as Metal. This ADR answers the
second question as option D. On the first: the IDE does not depend on FTEQW under any option here.
The preview host is the engine that guarantees live preview on all three operating systems; FTEQW
is one optional adapter (P3-07), useful on Windows and Linux because modders already use it and
its CSQC and extensions go beyond QuakeSpasm's. #13 only decides whether that adapter is also
offered on macOS. Whether the FTEQW adapter stays on the MVP path is a question for #21.

### What the bridge needs

The bridge needs the same things from every QuakeSpasm-lineage candidate, because they share
QuakeSpasm's code: a socket polled once per `Host_Frame`, and hooks in `PR_LoadProgs` (progs
loaded), `ED_Alloc` and `ED_Free` (edict spawned and removed), the field writes in the interpreter
(edict changed), `PR_RunError` (QC error) and the statement loop of `PR_ExecuteProgram`
(breakpoints and stepping). The candidates differ in what surrounds those functions.

### The C candidates, as of 2026-10-15

Sources were read at each project's default branch on that date (tarballs from GitHub; activity
from the GitHub API). Line counts are `wc -l` over `Quake/*.c` and `Quake/*.h`, which include a
few bundled helper libraries, so compare them as relative sizes only.

| Criterion | QuakeSpasm | Ironwail | vkQuake |
| --- | --- | --- | --- |
| License | GPL-2.0-or-later (`Quake/host.c` header: "or (at your option) any later version"), so usable under GPL-3.0-or-later (ADR-0002) | GPL-2.0-or-later, same header | GPL-2.0-or-later, same header |
| Apple silicon | Native. Upstream ships an Xcode project whose CI builds an `M1` and an `x64` target (`.github/workflows/build-mac.yml`); Mac Source Ports also lists it | Not supported. Its README: "Mac OS is not supported at this time due to the use of OpenGL 4.3 (for compute shaders), since Apple has deprecated OpenGL after version 4.1." Its macOS CI only compiles | Native through MoltenVK. macOS CI upstream; Mac Source Ports ships signed, notarized universal builds (1.35.0, built 2026-07-09) |
| Renderer | OpenGL; Apple has deprecated OpenGL but still ships 4.1 | OpenGL 4.3 with compute shaders | Vulkan; needs the Vulkan headers, glslang and MoltenVK to build on macOS |
| Rerelease (2021, and the 2026 update) | Plays it since 0.94; 0.97.0 adds the 2026 anniversary update (localization, word-wrapped centerprints); patches the rerelease's builtins (`PR_PatchRereleaseBuiltins` in `pr_edict.c`) | Plays it with no setup, finds Steam, GOG and Epic installs, loads MD5 models and KEX saves | Plays it, finds Steam, GOG and Epic installs, MD5 models, rerelease `dynamiclight` entities |
| Code size | 84,791 lines, 157 files | 117,795 lines, 166 files | 169,963 lines, 186 files |
| Bridge invasiveness | Lowest. One thread, one QC VM, the classic `Host_Frame` loop: every hook above is a single call site | Medium. Server and renderer are decoupled (from QSS via vkQuake), so the bridge must respect the split when it reads edicts and takes screenshots | Highest. A task system (`tasks.c`) runs loading and rendering on worker threads, and QSS's multiple `qcvm_t` instances (server and client QC) mean every hook has to name its VM and be thread-safe |
| Upstream activity (12 months to 2026-10-15) | 35 commits, last 2026-10-03; releases on SourceForge (0.97.0) | 67 commits, last 2026-09-09; v0.8.2 on 2026-08-11 | 486 commits, last 2026-10-14; 1.37.0 on 2026-10-12 |

### A Rust host: what it would take

A Rust preview host would port id's GPL engine (with QuakeSpasm, also GPL-2.0-or-later, as the
reference for the rerelease and modern-mod behaviour) under the project's porting process
(`docs/porting.md`), render through wgpu (Metal on macOS, DX12 or Vulkan on Windows, Vulkan on
Linux), and run the project's own L1 crates (`qc-vm`, `qc-edict`, `qc-builtins`, Phase 1) as its
QuakeC VM. It could share the Phase 5 renderer (`qs-render`, P5-05
[#146](https://github.com/quakecstudio/editor/issues/146)) and run in the IDE's process, which is
the natural road to the post-MVP embedded game view (L-06
[#190](https://github.com/quakecstudio/editor/issues/190)). For that host the bridge would be a
Rust API rather than a socket protocol; the socket stays for the other engines.

What exists already, and what would have to be written, measured in QuakeSpasm's own lines as a
rough guide to the behaviour that has to be matched:

| Subsystem | QuakeSpasm lines | In the plan today |
| --- | --- | --- |
| QuakeC VM, edicts, builtins (`pr_*`) | 4,312 | Ported in Phase 1 (P1-08 to P1-16), proved against id's VM |
| Server physics, movement and collision (`world.c`, `sv_phys.c`, `sv_move.c`, `sv_user.c`) | 3,311 | Not planned. Movement feel and collision quirks are what players notice first |
| Server main and protocol (`sv_main.c`) | 2,151 | Not planned. Includes FitzQuake protocol 666 and the rerelease's messages |
| Client, view, temp entities, demos | 4,959 | Not planned |
| Renderer (world, lightmaps, lightstyles, alias and MD5 models, sprites, particles, sky, water warp, fog) | 19,786 | P5-05 covers BSP, lightmaps and MDL for the map editor only, after the MVP |
| Sound mixer, spatialization, music | 6,732 | Not planned |
| HUD, menus, console, centerprint and rerelease localization | 6,742 | Not planned (the IDE could replace some of it) |
| Host, commands, cvars, file system | 9,014 | Partly: PAK reading and game-data discovery in Phase 1 |

Prior art shows both that it can be done and that it takes a long time. Richter, a Rust Quake
engine, stalled in pre-alpha. Seismon, its rewrite on Bevy, plays demos, but its networking has not
been tested since the rewrite began. Quake-Rusted-Metal ported a C engine to Rust module by module
with 27 bit-for-bit differential suites, on macOS only. The project's own estimate, which is not
measured, is six to twelve months of focused work on top of the current plan to reach "plays id1
and the rerelease like QuakeSpasm".

Fidelity would be proved the way the VM is: the server side (physics, movement, QC) is
deterministic, so a differential runner can step the Rust host and an oracle build (id's engine or
the QuakeSpasm fork) on the same map, progs and inputs and compare edicts after every frame.
Rendering and sound cannot be compared bit for bit; they would need image comparisons with
tolerances and listening tests. Mods are the other fidelity problem: many rely on engine extension
builtins and protocol behaviour from QuakeSpasm, FTEQW or DarkPlaces (the engine-target database,
P2-04, lists them), and each one a Rust host lacks is a mod that previews wrongly.

**Not verified here.** The acceptance criterion asks for each C candidate built and run on all
three operating systems. That was not done for this draft: it needs a Windows and a Linux machine
(or CI in `quakecstudio/preview-host`, which does not exist yet) and the Quake game data, which an
agent must not fetch. The Apple silicon and platform rows come from each project's own README and
CI configuration, not from a build. The build logs and screenshots go on the issue before this ADR
is accepted. The Rust estimates are judgement, not measurement.

## Options

| Option | Consequences |
| --- | --- |
| A. Fork QuakeSpasm | Smallest and simplest code to fork; every hook is one call site, so `PATCHES.md` stays short and upstream merges stay cheap. Native on Apple silicon today; rerelease support current (0.97.0). Plan unchanged: P3-02 is one L-size issue. Risks: OpenGL, deprecated by Apple since 2018, could be removed from a future macOS (fallback: vkQuake, option C); a C codebase beside a Rust one, so the bridge is written twice (C in the host, Rust in the adapter) |
| B. Fork Ironwail | Fastest renderer and the best store detection, but it does not run on macOS, the one platform the host exists to cover (PLAT-3). Not viable |
| C. Fork vkQuake | Most active upstream; Vulkan through MoltenVK has a future on macOS; signed Mac builds exist. Twice QuakeSpasm's size, multithreaded, with several QC VMs, so the bridge's edict and breakpoint hooks must be thread-aware, P3-03 hot reload and P3-04 stepping get harder, and the commit rate makes a patched fork costly to keep current. The Vulkan toolchain is needed on all three CI runners |
| D. Port a Rust host now, on wgpu and the project's L1 crates (chosen) | One language, one codebase, native GPU APIs everywhere, no OpenGL risk, the VM proved in Phase 1 reused, and a direct path to the embedded game view (L-06). The bridge becomes an in-process API. Cost: Phase 3 grows from one fork issue to a sequence of spec and port issues (physics and collision, server and protocol, client, renderer, sound, HUD and localization, rerelease builtins), each with a differential suite; P5-05's renderer moves into Phase 3; the MVP moves out by the six to twelve months above. Mods that depend on engine extensions the Rust host has not ported preview wrongly until they are added |
| E. Phased: fork QuakeSpasm for the MVP; plan the Rust host after it, with its own ADR (recommended by the agent, not chosen) | A's schedule and risk for the MVP. After the MVP, a post-MVP issue specifies the Rust host; it is built behind the same `EngineAdapter` trait as one more engine, and the QuakeSpasm fork becomes its oracle for differential tests, so fidelity is proved against the engine modders already use. The Rust host can then replace the fork as the default, and carries the embedded game view. Cost: the C fork is maintained until then, and some bridge work is done twice |

## Decision

**Option D. The preview host is our own engine, written in Rust from id's GPL source, rendering
through wgpu and running the project's `qc-vm`, `qc-edict` and `qc-builtins`.** It is a separate
program in this repository (`apps/preview-host`), launched beside the IDE as the v1 docking design
requires (ENG-9), and it talks to the IDE over the bridge protocol (P3-01) like any other engine. No
C engine is forked and there is no `quakecstudio/preview-host` repository.

The host is ported the way the foundations are ported (`docs/porting.md`): id's engine and, for the
rerelease and modern-mod behaviour, QuakeSpasm are the reference, each subsystem is specified in
`docs/spec/` before it is implemented, and every port cites the file and function it came from.
Fidelity is proved against an engine oracle. That oracle is a pinned QuakeSpasm built in `oracles/`,
CI only, never shipped. The server simulation must match it frame by frame. Rendering and sound are
compared with stated tolerances.

Why D, as Justin decided it: one language and one codebase, native GPU APIs on every platform
(Metal on macOS, so no dependence on Apple's deprecated OpenGL), the VM the project already proves
in Phase 1 reused instead of a second VM in C, and a direct road to the embedded game view (L-06).
FTEQW and the QuakeSpasm-lineage engines become optional adapters, not dependencies.

The agent preparing this ADR recommended option E (a QuakeSpasm fork for the MVP, the Rust host
after it) because D puts most of an engine on the MVP path; that recommendation and its reasoning
are kept in Options above. The cost is real and is planned for, not waved away: the engine work is
in Phase 3 as its own spec, port and differential-test issues, the BSP and model formats move to
Phase 1, the renderer moves from Phase 5 to Phase 3, and the schedule risk is risk R-11 in
`docs/risks.md`. Options A, B, C and E were not chosen: A and E keep a C engine on the critical path
of a Rust project and defer the native renderer, B does not run on macOS, and C has the largest and
most concurrent code to patch.

## Consequences

The plan changes are in this ADR's pull request:

- **Requirements** (`plan/requirements.yaml`, with the design document): ENG-2 now describes the
  Rust host; ARCH-2 no longer lists the host among the C programs. Two implied requirements are
  new: ENG-15 (the host's simulation matches the engine oracle frame by frame; rendering and sound
  within stated tolerances) and ENG-16 (the host reads every asset the game uses: BSP29, BSP2,
  `.lit`, MDL, SPR, WAV, the rerelease's MD5 models and `QuakeEX.kpf`).
- **Phase 1:** P5-01 (spec of the BSP, MDL and `.map` formats) and P5-02 (their readers and the
  `.map` writer) move into Phase 1 with the other L0 formats, keeping their keys. New P1-29 specifies
  and reads SPR sprites, WAV sounds and MD5 models.
- **Phase 3:** P3-02 becomes "Build the preview host app" (`apps/preview-host`), no longer a C fork.
  P5-05 (the wgpu renderer, `qs-render`, L7) moves from Phase 5 into Phase 3, keeping its key. That
  is earlier in the schedule, not in the layering: the renderer stays L7, and only `apps`
  crates (the preview host and the desktop app) use it. New issues:
  - P3-22: spec of the preview host engine (L4).
  - P3-23: the engine oracle, a pinned QuakeSpasm in `oracles/` (C tools).
  - P3-24: the server simulation, `qs-sim` (L4).
  - P3-25: its differential suite (L4).
  - P3-26: rendering the game, extending `qs-render` (L7).
  - P3-27: sound, `qs-sound` (L4).
  - P3-28: the render and sound fidelity checks (L7).

  P3-03, P3-04, P3-05 and P5-10 are Rust work now (L4 instead of C tools).
- **#13 and #14:** P0-13 (FTEQW on Apple silicon) moves from Phase 0 to Phase 3, where it gates
  only the FTEQW adapter (P3-07); it no longer blocks the Phase 0 gate or P0-14. P0-14 still decides
  the default target per OS, which is now our own host everywhere unless it decides otherwise.
- **Documents:** the design document (revision 37: Source and licensing, Platform support,
  Architecture, Engine bridge, Map editor renderer, Milestones, Risks), `docs/layers.md`,
  `docs/roadmap.md` and `docs/risks.md` (new R-11) say the same. The readiness issues are
  regenerated.
- **Not changed here:** `CLAUDE.md`, `.claude/rules/layering.md` and the `work-issue` skill still
  describe the host as a C fork in `quakecstudio/preview-host`. Agent instructions are Justin's to
  edit, so they are left for him.
- **Harder or impossible now:** the MVP waits for a playable Rust engine; mods that need engine
  extension builtins the host has not ported preview wrongly until they are added, and the
  engine-target database (P2-04) must list the host's builtins as its own target.

## Links

- Issue: [#12](https://github.com/quakecstudio/editor/issues/12); pull request: this ADR's draft PR.
- Justin's question on [#13](https://github.com/quakecstudio/editor/issues/13#issuecomment-6205060482).
- Related: [ADR-0002](0002-license-model.md) (GPL-3.0-or-later), [#14](https://github.com/quakecstudio/editor/issues/14) default preview target, [#21](https://github.com/quakecstudio/editor/issues/21) MVP scope, [#190](https://github.com/quakecstudio/editor/issues/190) embedded game view.
- Design document: [Engine bridge and live preview](../design/design-doc.md#engine-bridge-and-live-preview), [Platform support](../design/design-doc.md#platform-support).
- QuakeSpasm: [sezero/quakespasm](https://github.com/sezero/quakespasm) (`Quakespasm.txt` changelog, `MacOSX/`, `.github/workflows/build-mac.yml`, `Quake/pr_edict.c`), [SourceForge project](https://sourceforge.net/projects/quakespasm/).
- Ironwail: [andrei-drexler/ironwail](https://github.com/andrei-drexler/ironwail) (README, system requirements note 2).
- vkQuake: [Novum/vkQuake](https://github.com/Novum/vkQuake) (readme, `Quake/tasks.c`, `Quake/progs.h`).
- [Mac Source Ports: Quake](https://www.macsourceports.com/game/quake), fetched 2026-10-15.
- Rust prior art: [cormac-obrien/richter](https://github.com/cormac-obrien/richter), [eira-fransham/seismon](https://github.com/eira-fransham/seismon), [mstits/Quake-Rusted-Metal](https://github.com/mstits/Quake-Rusted-Metal).
- id's engine source: [id-Software/Quake](https://github.com/id-Software/Quake).
