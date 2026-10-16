# ADR-0004: Fork QuakeSpasm as the base of the preview host

- **Status:** Proposed
- **Date:** 2026-10-15
- **Decided by:** pending
- **Issue:** [#12](https://github.com/quakecstudio/editor/issues/12)
- **Requirements:** ENG-2, PLAT-3

## Question

Which QuakeSpasm-lineage engine, QuakeSpasm, Ironwail or vkQuake, does `quakecstudio/preview-host`
fork to add the IDE bridge?

## Context

The design document's Engine bridge section makes the preview host "a small GPL fork of a
QuakeSpasm-lineage engine with one added module, the IDE bridge" (ENG-2), and the host is what
makes live preview identical on macOS, Windows and Linux (PLAT-3), because FTEQW's macOS support is
unverified ([#13](https://github.com/quakecstudio/editor/issues/13), risk R-6 in
[`docs/risks.md`](../risks.md)). The choice blocks the default preview target
([#14](https://github.com/quakecstudio/editor/issues/14)), the bridge protocol spec
([#89](https://github.com/quakecstudio/editor/issues/89), P3-01) and, through them, the fork
itself ([#90](https://github.com/quakecstudio/editor/issues/90), P3-02), hot reload (P3-03), debugging (P3-04) and headless mode (P3-05). P3-02 asks for a
fork that keeps its changes outside `bridge/` small enough to list one by one in `PATCHES.md`, so
how much of the engine the bridge has to touch matters as much as features.

What the bridge needs from the engine is the same in all three candidates, because they share
QuakeSpasm's code: a socket polled once per `Host_Frame`, and hooks in `PR_LoadProgs` (progs
loaded), `ED_Alloc` and `ED_Free` (edict spawned and removed), the field writes in the interpreter
(edict changed), `PR_RunError` (QC error) and the statement loop of `PR_ExecuteProgram`
(breakpoints and stepping). The differences are in what surrounds those functions.

### Facts gathered on 2026-10-15

Sources were read at each project's default branch on that date (tarballs from GitHub; activity
from the GitHub API). Line counts are `wc -l` over `Quake/*.c` and `Quake/*.h`, which include a
few bundled helper libraries, so compare them as relative sizes only.

| Criterion | QuakeSpasm | Ironwail | vkQuake |
| --- | --- | --- | --- |
| License | GPL-2.0-or-later (`Quake/host.c` header: "or (at your option) any later version"), so usable under GPL-3.0-or-later (ADR-0002) | GPL-2.0-or-later, same header | GPL-2.0-or-later, same header |
| Apple silicon | Native. Upstream ships an Xcode project whose CI builds an `M1` and an `x64` target (`.github/workflows/build-mac.yml`); Mac Source Ports also lists it | Not supported. Its README: "Mac OS is not supported at this time due to the use of OpenGL 4.3 (for compute shaders), since Apple has deprecated OpenGL after version 4.1." Its macOS CI only compiles | Native through MoltenVK. macOS CI upstream; Mac Source Ports ships signed, notarized universal builds (1.35.0, built 2026-07-09) |
| Renderer | OpenGL (fixed function plus optional GLSL); Apple has deprecated OpenGL but still ships 4.1 | OpenGL 4.3 with compute shaders | Vulkan; needs the Vulkan headers, glslang and MoltenVK to build on macOS |
| Rerelease (2021, and the 2026 update) | Plays it since 0.94; 0.97.0 adds the 2026 anniversary update (localization, word-wrapped centerprints); patches the rerelease's builtins (`PR_PatchRereleaseBuiltins` in `pr_edict.c`) | Plays it with no setup, finds Steam, GOG and Epic installs, loads MD5 models and KEX saves | Plays it, finds Steam, GOG and Epic installs, MD5 models, rerelease `dynamiclight` entities |
| Code size | 84,791 lines, 157 files | 117,795 lines, 166 files | 169,963 lines, 186 files |
| Bridge invasiveness | Lowest. One thread, one QC VM, the classic `Host_Frame` loop: every hook above is a single call site | Medium. Server and renderer are decoupled (from QSS via vkQuake), so the bridge must respect the split when it reads edicts and takes screenshots | Highest. A task system (`tasks.c`) runs loading and rendering on worker threads, and QSS's multiple `qcvm_t` instances (server and client QC) mean every hook has to name its VM and be thread-safe |
| Upstream activity (12 months to 2026-10-15) | 35 commits, last 2026-10-03; releases on SourceForge (0.97.0) | 67 commits, last 2026-09-09; v0.8.2 on 2026-08-11 | 486 commits, last 2026-10-14; 1.37.0 on 2026-10-12 |

**Not verified here.** The acceptance criterion asks for each candidate built and run on all three
operating systems. That was not done for this draft: it needs a Windows and a Linux machine (or CI
in `quakecstudio/preview-host`, which does not exist yet) and the Quake game data, which an agent
must not fetch. The Apple silicon and platform rows above come from each project's own README and
CI configuration, not from a build. The build logs and screenshots go on the issue before this ADR
is accepted.

## Options

| Option | Consequences |
| --- | --- |
| A. QuakeSpasm (recommended) | Smallest and simplest code to fork; every hook is one call site, so `PATCHES.md` stays short and upstream merges stay cheap. Native on Apple silicon today. Rerelease support is current (0.97.0). Risk: the renderer is OpenGL, which Apple deprecated in 2018; if a future macOS drops it, the host moves to vkQuake (option C). Slowest of the three on very large maps, which matters little for a preview of one mod |
| B. Ironwail | Fastest renderer and the best store detection, but it does not run on macOS, so PLAT-3 fails on the one platform the host exists to cover. It would need a renderer port before Phase 3 could pass its gate. Not viable |
| C. vkQuake | Most active upstream, Vulkan (future-proof on macOS through MoltenVK), signed Mac builds exist. Twice QuakeSpasm's size, multithreaded, with several QC VMs: the bridge's edict and breakpoint hooks must be thread-aware, P3-03 hot reload and P3-04 stepping get harder, and the high commit rate makes keeping a patched fork current more work. Build needs the Vulkan toolchain on all three CI runners |

## Decision

Recommendation: **fork QuakeSpasm (option A).** It is the only candidate that is both native on
Apple silicon today and small and single-threaded, which is what a fork maintained beside an IDE
needs: the bridge touches six well-known functions, each at one call site, and the rest of the
engine stays as upstream wrote it. Its rerelease support is current with the 2026 update, its
license is GPL-2.0-or-later like the others, and its slower pace is an advantage for a fork that
has to merge upstream. Ironwail is ruled out by macOS. vkQuake is the fallback, chosen if the
three-OS build check finds QuakeSpasm broken on a current macOS, or later if Apple removes OpenGL;
because the hook points are the same functions in both, the bridge module would move with modest
rework.

## Consequences

If accepted:

- `plan/backlog/p3.yaml`: P3-02's deliverable already says "forked from the engine chosen in
  ADR-0004"; it gains "QuakeSpasm (upstream `sezero/quakespasm`), at a pinned release tag" and an
  acceptance line that the fork builds with the upstream Xcode project or Makefile on macOS and
  with MinGW or MSVC on Windows. No new issues and no dependency changes.
- P3-01 (the bridge protocol) can assume one QC VM and a single-threaded host loop; CSQC and
  multiple VMs stay post-MVP with CSQC itself (L-03, L-09).
- P0-14 (default preview target) can decide with the host's platforms known: native on all three
  operating systems.
- Risk R-6 gains a note in `docs/risks.md` at the next gate review: the host's macOS support rests
  on Apple's deprecated OpenGL, with vkQuake as the named fallback.
- `quakecstudio/preview-host` must be created by Justin before P3-02 starts (agents do not create
  repositories).

## Links

- Issue: [#12](https://github.com/quakecstudio/editor/issues/12); pull request: this ADR's draft PR.
- Related: [ADR-0002](0002-license-model.md) (GPL-3.0-or-later), [#13](https://github.com/quakecstudio/editor/issues/13) FTEQW on Apple silicon, [#14](https://github.com/quakecstudio/editor/issues/14) default preview target.
- Design document: [Engine bridge and live preview](../design/design-doc.md#engine-bridge-and-live-preview), [Platform support](../design/design-doc.md#platform-support).
- QuakeSpasm: [sezero/quakespasm](https://github.com/sezero/quakespasm) (`Quakespasm.txt` changelog, `MacOSX/`, `.github/workflows/build-mac.yml`, `Quake/pr_edict.c`), [SourceForge project](https://sourceforge.net/projects/quakespasm/).
- Ironwail: [andrei-drexler/ironwail](https://github.com/andrei-drexler/ironwail) (README, system requirements note 2).
- vkQuake: [Novum/vkQuake](https://github.com/Novum/vkQuake) (readme, `Quake/tasks.c`, `Quake/progs.h`).
- [Mac Source Ports: Quake](https://www.macsourceports.com/game/quake), fetched 2026-10-15.
