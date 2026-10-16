# ADR-0005: Run only our own preview host, with no outside engines

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** Justin Plock, 2026-10-15, in the run-phase session: "i always want to use our host. i dont' want to depend on outside engines at all" and "we are going to create our own rust-based port of the quake engine so we dont' need any adapters or outside engines."
- **Issue:** [#14](https://github.com/quakecstudio/editor/issues/14)
- **Requirements:** OPEN-2, ENG-1, ENG-2, ENG-15, UI-2

## Question

Which engine does Play run, and does the IDE launch or depend on any engine other than our own?

## Context

OPEN-2 asked whether the default preview target is our preview host or FTEQW where it is
available. [ADR-0004](0004-preview-host-base.md) then made the preview host our own engine, a
Rust port of id's GPL source on wgpu running our VM crates. With that, the outside engines the
plan still carried were optional: an FTEQW adapter (P3-07) and its Apple silicon spike (P0-13,
[#13](https://github.com/quakecstudio/editor/issues/13)), restart-loop adapters for QuakeSpasm,
Ironwail and vkQuake (P3-08), discovery of installed engines (P3-20), an engine picker in the
toolbar (UI-2, P3-12), a post-MVP DarkPlaces adapter (L-05), and a pinned QuakeSpasm built in CI
as the engine oracle that the host is compared against (P3-23).

## Options

| Option | Consequences |
| --- | --- |
| A. Our host only, everywhere; no adapters and no outside engine anywhere (decided) | Play always runs the preview host on macOS, Windows and Linux. P0-13, P3-07, P3-08, P3-20 and L-05 leave the plan; the toolbar has no engine picker; one bridge client replaces the `EngineAdapter` trait. The engine oracle becomes id's own engine built from the pinned source in `docs/porting.md`, the code the host is ported from, so no outside engine is used even in CI |
| B. Our host by default; outside engines selectable in an engine picker | Modders could check a mod in the engines players use, at the cost of five issues, per-engine protocols and tests against pinned engine builds on three OSes |
| C. FTEQW by default where installed, our host otherwise | Behaviour would differ by machine and OS, and live debugging would depend on FTEQW's protocol and its unverified Apple silicon support |

## Decision

**Option A. Play always runs our own preview host, on every OS, and the IDE never launches,
discovers or depends on an outside engine.** QuakeC Studio carries its own Rust port of the Quake
engine (ADR-0004), so adapters for other engines are not needed. The host's fidelity is proved
against id's engine, built from the pinned id-Software/Quake source as an oracle in CI only, the
same way the VM and compiler are proved against id's code. Engines such as QuakeSpasm, Ironwail,
vkQuake and FTEQW remain what players use to play exported mods; they are not part of the IDE.

## Consequences

In this ADR's pull request:

- `plan/requirements.yaml`: PLAT-4, PLAT-5, PLAT-7, ENG-5, ENG-6 and ENG-7 are removed. ENG-1
  becomes the bridge client for the preview host; UI-2 drops the engine picker; OPEN-2, MVP-3 and
  MVP-4 are reworded.
- `plan/backlog/`: P0-13 (FTEQW on Apple silicon), P3-07 (FTEQW adapter), P3-08 (restart-loop
  adapters), P3-20 (engine discovery) and L-05 (DarkPlaces adapter) are removed. P3-06 becomes
  the bridge client, P3-12 the play toolbar without a picker, P3-16's debug flow runs against the
  host only, and P3-23's oracle is id's engine. P4 and P6 acceptance criteria that loaded files
  in QuakeSpasm or vkQuake now load them in the preview host.
- The design document (revision 38), roadmap, scope, layers, testing notes, risk register (R-6
  retired) and agent instructions are updated to match.
- [ADR-0007](0007-mvp-scope.md) kept the restart-loop adapters in the MVP and
  [ADR-0008](0008-game-directories.md) describes launch flags for several engines; this ADR
  removes the adapters. ADR-0008's run directory and `-basedir`/`-game` launch still apply, to the
  preview host only.
- After merge, `plan/apply.py` syncs the issues; the removed issues (#13, #95, #96, #108, #189)
  are closed as not planned.
- Not changed: the engine-target database (P2, `qc-engines`) still knows other engines' builtins
  and extensions, so the compiler can warn when a mod calls something its players' engine lacks.
  That is data about engines, not a dependency on running them.

## Links

- Issue: [#14](https://github.com/quakecstudio/editor/issues/14); pull request: this ADR's PR.
- Related: [ADR-0004](0004-preview-host-base.md) (the Rust preview host), partly overrides
  [ADR-0007](0007-mvp-scope.md) (restart-loop adapters) and narrows
  [ADR-0008](0008-game-directories.md) (launch flags) to the preview host.
- [`docs/porting.md`](../porting.md) (the pinned id-Software/Quake commit the oracle builds).
