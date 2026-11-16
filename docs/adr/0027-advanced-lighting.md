# ADR-0027: Schedule ericw-tools' advanced lighting by how much the corpus uses it

- **Status:** Accepted
- **Date:** 2026-11-15
- **Decided by:** Justin Plock, 2026-11-15
- **Issue:** [#195](https://github.com/quakecstudio/editor/issues/195)
- **Requirements:** MAP-21

## Question

Which of ericw-tools' advanced lighting features does `qs-map-light` take, and when?

## Context

MAP-21 says "ericw-tools' advanced lighting features come later". `qs-map-light` today is id's
`light` with coloured light (`docs/spec/light.md`, LIGHT-1 to LIGHT-7, P7-01), and
[ADR-0025](0025-ericw-tools-vis-and-light.md) makes ericw-tools' `light/` a porting upstream at
the commit `docs/porting.md` pins (`ericwa/ericw-tools@36eec1da`). None of the features below is
in it: a map that uses them compiles darker, or flatter, than ericw-tools compiles it, in the
Lit view and in the game.

The features, as ericw-tools' `docs/light.rst` at that commit describes them:

- **Sunlight**, `_sunlight`: "the brightness of the sunlight coming from an unseen sun in the
  sky. Sky brushes ... will emit sunlight at an angle specified by the "_sun_mangle" key", with
  `_sunlight_penumbra`, "the penumbra width, in degrees, of sunlight", and a second sun.
- **Sky dome**, `_sunlight2`: "a dome of lights arranged around the upper hemisphere (i.e.
  ambient light, coming from above the horizon)".
- **Surface lights**, `_surface`: "Makes surfaces with the given texture name emit light ...
  spaced about 128 units ... apart and positioned 2 units above the surfaces".
- **Minimum light**, `_minlight`: "a global minimum light level of "n" across the whole map".
- **Phong**, `_phong` on a brush model: "phong shading on this model ... (softens columns etc)".
- **Dirt**, `_dirt`: "dirtmapping (ambient occlusion) ... borrowed from q3map2. This adds
  shadows to corners and crevices".
- **Bounce**, `_bounce`: "Non-zero enables bounce lighting ... The value is the maximum number
  of bounces to perform".

How LibreQuake, the corpus's largest map set, uses them: of its 91 `.map` files at
`LibreQuake@681890f`, counted on 2026-11-15 by the files whose text holds the key, `_sunlight2`
47, `_phong` 44, `_minlight` 43, `_surface` 42, `_sunlight` 41, `_sunlight_penumbra` 28,
`_dirt` 21, `_bounce` 19, `_anglescale` 1, `_sun2` and `_deviance` none.

## Options

| Option | Consequences |
| --- | --- |
| A. All of them, in two tranches by use (recommended) | First tranche: sunlight with its penumbra and second sun, the sky dome, surface lights, minimum light and phong, each a port from ericw-tools' `light/` with a spec section in `docs/spec/light.md` and a differential against ericw-tools' `light` (the `light-diff` of P7-01, ericw mode). Second tranche: dirt and bounce, the costliest to compute, each with a `qs-bench` budget so the background full compile (P7-02) keeps its time. One post-MVP issue per tranche, each feature's spec section and port together. |
| B. The first tranche only | As A without dirt and bounce: maps that use them (21 and 19 of LibreQuake's) keep compiling flatter than ericw-tools makes them. |
| C. None: a profile runs ericw-tools' own `light` | No port. A modder who needs these features adds ericw-tools' `light` as an external task in a compile profile (P7-08's profile tasks). The Lit view, the background compile and region compile do not have them, and the external tool is a separate install. |
| D. Reject all | MAP-21 is closed unmet; half of LibreQuake's maps light differently with our tools than with the tools they were built for. |

## Decision

A, as Justin chose on 2026-11-15. The corpus answers the question: about half of LibreQuake's maps rely on the sky
dome, phong, minimum light, surface lights or sunlight, so a modder opening them sees darker,
flatter lighting in our Lit view and our compile than the map's author saw. Porting them from
ericw-tools, which ADR-0025 already allows, with the same differential P7-01 used for coloured
light, makes our compile match the tool those maps were made for. Dirt and bounce come second
because fewer maps use them and they cost the most time, which the background compile has to
keep within its budget.

## Consequences

- `plan/`: two issues in the post-MVP backlog, one per tranche, each a port with its spec
  sections as `docs/porting.md` requires: L-22, "Port ericw-tools' sunlight, sky dome, surface
  lights, minimum light and phong", then L-23, "Port ericw-tools' dirt and bounce lighting". Each port's acceptance holds
  `light-diff` to ericw-tools' output on the corpus maps that use its keys, and dirt and bounce
  each add a `qs-bench` budget.
- `docs/spec/light.md`: a section per feature, written from ericw-tools' `light/` at the pinned
  commit.
- MAP-21 is covered by those issues; this ADR closes #195.

## Links

- [#195](https://github.com/quakecstudio/editor/issues/195), the decision issue.
- [ADR-0023](0023-ericw-tools-porting-upstream.md), [ADR-0025](0025-ericw-tools-vis-and-light.md).
- ericw-tools' light documentation:
  [ericwa/ericw-tools@36eec1da `docs/light.rst`](https://github.com/ericwa/ericw-tools/blob/36eec1da2a194467e6baac2f444c2dd04b57d266/docs/light.rst),
  fetched 2026-11-15.
- LibreQuake at `681890f1216d339a989b55d7c6a6cfd95db0b33e`, `lq1/maps/src`: the key counts, taken
  locally with `rg -l` over its `.map` files on 2026-11-15.
