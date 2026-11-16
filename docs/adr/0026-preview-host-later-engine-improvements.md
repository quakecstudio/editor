# ADR-0026: Follow the later engines in the preview host by category, the simulation staying id's

- **Status:** Accepted
- **Date:** 2026-11-15
- **Decided by:** Justin Plock, 2026-11-15
- **Issue:** [#449](https://github.com/quakecstudio/editor/issues/449)
- **Requirements:** ENG-2, ENG-15

## Question

For each kind of change QuakeSpasm, Ironwail and vkQuake make beyond id's engine, does the
preview host follow id or the later engine?

## Context

ENG-2 makes the preview host "our own engine in Rust, ported from id's GPL source", and ENG-15
holds its server simulation to "the engine oracle frame by frame on the corpus". The oracle is
id's engine ([ADR-0005](0005-default-preview-target.md), P3-23). Since
[ADR-0018](0018-quakespasm-lineage-porting-upstreams.md), QuakeSpasm, Ironwail and vkQuake are
porting upstreams for the host at the commits `docs/porting.md` pins, under
[ADR-0017](0017-port-fteqcc.md)'s clean-port policy.

Justin gave the direction on 2026-10-29, after `gl_zfix` was added off by default: "we are
building our own Rust-based quake engine that is the best of all of the reference engines". The
host has since taken later-engine behaviour one change at a time, each with a spec statement:

- **Limits:** the client keeps QuakeSpasm's limits by default, id's behind
  `ClientLimits::WINQUAKE` (`docs/spec/engine/client.md`, CLIENT-8.1, CLIENT-8.2); the mock
  engine runs vkQuake's builtin profile and runaway count for mods that need it (BUILTIN-18.26,
  LIMIT-4.3).
- **Frame rate:** the host's server ticks at a fixed rate below the display's and the client
  interpolates (P3-02, `client.md` CLIENT-5.2), which is the decoupling vkQuake's readme
  credits: "The game can run at higher frame rates than 72Hz without breaking physics", and
  Ironwail's: "To avoid physics issues the renderer is also decoupled from the server".
- **Rendering and the 2D layer:** a QuakeSpasm look beside GLQuake's (`render.md`, REND-13.8;
  `console.md`, CON-5), QuakeSpasm's two-pose blend (REND-4.12) and lightmap uploads (REND-5.7),
  `gl_zfix` (REND-1.12, as an option).
- **Formats:** QuakeSpasm's BSP loader, BSP2 included ([ADR-0012](0012-bsp-loader-default.md)).

What the three engines change, from their own notes at the pinned commits:

- **QuakeSpasm** (`Quakespasm.txt`): FitzQuake's protocol 666 by default and RMQ's 999, with
  15 kept ("Accepted values for 'x' are 15 (NetQuake), 666 (FitzQuake, default), and 999
  (RMQ)"); BSP2 and 2PSB maps; raised limits (MAX_EFRAGS and MAX_MAP_LEAFS removed,
  MAXALIASFRAMES 1024, a runaway limit of 16M); many fixes of crashes and overflows in id's C;
  lit water; viewmodel interpolation fixed "with >10Hz animations".
- **Ironwail** (`README.md`): performance by moving culling and lightmap updates to the GPU;
  lightstyle interpolation; lightmapped liquids; "a more precise ... work-around for the
  z-fighting issues present in the original levels"; Q64 HUD styles; real-time palettization.
- **vkQuake** (`readme.md`): multithreaded rendering and loading; frame rates above 72 Hz
  without breaking physics; "More modern protocol to avoid certain movement issues (from QSS)";
  MD5 and MD3 models; ray-traced shadows and dynamic lights; 8-bit colour emulation.

What is open is a rule per category, so the next change is not a decision of its own, and the
question the issue asks: whether any id behaviour in the simulation gives way.

## Options

| Option | Consequences |
| --- | --- |
| A. id everywhere else: keep what was adopted, adopt nothing more | No plan change. The host falls behind what modders play in (QuakeSpasm and its forks), and each later fix asked for becomes its own decision, against Justin's direction of 2026-10-29. |
| B. By category, the simulation id's (recommended) | Bug fixes: the server simulation (`qs-sim`) keeps id's behaviour, so ENG-15's frame-by-frame differential holds; a client-side defect id has (drawing, sound, input, the 2D layer) is fixed as the later engine fixes it, by default, with id's behaviour behind an option and a spec statement for each. Limits and formats: the later engines' by default, id's behind an option (as CLIENT-8.1 does). Frame rate: decoupled, as now. Rendering: the later engines' look by default, GLQuake's as the other look. Performance: adopted wherever the output is the same, with a measurement. Each adopted change is a port filed in `plan/` with its statement and its expected differential difference. |
| C. Later engines everywhere, the simulation too | As B, and the server simulation takes the later engines' fixes by default (vkQuake's movement protocol among them), id's behind a mode the differential runs in. Two simulation paths to keep and test; ENG-15's differential proves only the id mode; a mod's behaviour in the host can differ from id's in ways the oracle cannot check. |
| D. Each change its own decision | No category rules; every adoption a decision issue. The slowest path, and the one Justin's direction asked to leave. |

## Decision

B, as Justin chose on 2026-11-15. It writes down the rule the host already follows and Justin set on 2026-10-29,
"the best of all of the reference engines", while keeping the one thing the project's fidelity
rests on: the server simulation matches id's engine frame by frame (ENG-15, P3-25), so the
differential suite keeps proving the game logic modders write against. Everything a player sees
or hears, and every limit a modern map needs, follows the later engine by default, as QuakeSpasm
and its forks are what modders test in; id's behaviour stays one option away, so a modder can
still check a mod against the original. C buys movement fixes at the cost of two simulations and
a differential that no longer covers the default; A and D leave the host behind or slow every
change down.

## Consequences


- `plan/`: the changes adopted under B that are not yet in the host are one post-MVP issue
  (L-21), each port in it naming the engine, file and function, its spec statement and its
  expected differential difference. From the notes above: lightstyle interpolation and
  lightmapped liquids (Ironwail), the viewmodel interpolation fix for animations above 10 Hz and
  lit water (QuakeSpasm), Ironwail's z-fighting work-around in place of `gl_zfix`, and protocols
  666 and 999 for demos and the bridge's messages where the host writes them. Performance ports
  (Ironwail's GPU culling and lightmap updates, vkQuake's multithreaded loading) are filed under
  #966's audit, each with a measurement.
- `docs/porting.md`, "Which source is authoritative": the rule per category is stated once there,
  and each spec statement of an adoption cites this ADR.
- `qs-sim` keeps id's behaviour; a later engine's simulation fix is out of scope unless a later
  ADR revisits ENG-15.
- What becomes harder: every client-side change carries two behaviours, the default and id's
  option, and both are tested.

## Links

- [#449](https://github.com/quakecstudio/editor/issues/449), the decision issue.
- [ADR-0004](0004-preview-host-base.md), [ADR-0005](0005-default-preview-target.md),
  [ADR-0012](0012-bsp-loader-default.md), [ADR-0017](0017-port-fteqcc.md),
  [ADR-0018](0018-quakespasm-lineage-porting-upstreams.md).
- QuakeSpasm's notes:
  [sezero/quakespasm@c9c8ee98 `Quakespasm.txt`](https://github.com/sezero/quakespasm/blob/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5/Quakespasm.txt),
  fetched 2026-11-15.
- Ironwail's readme:
  [andrei-drexler/ironwail@08d57813 `README.md`](https://github.com/andrei-drexler/ironwail/blob/08d578136ff43d7d1ef38e636dfbfd3e844be7cd/README.md),
  fetched 2026-11-15.
- vkQuake's readme:
  [Novum/vkQuake@27789d73 `readme.md`](https://github.com/Novum/vkQuake/blob/27789d7333a46674474d871137bb71e056719628/readme.md),
  fetched 2026-11-15.
- Justin's direction of 2026-10-29, recorded in the session that added `gl_zfix`; not in a
  committed document before this ADR.
