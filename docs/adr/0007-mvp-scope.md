# ADR-0007: Confirm the MVP In list and move ACP, next-edit and the updater after it

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** Justin Plock, 2026-10-15, accepting the recommendation (option C)
- **Issue:** [#21](https://github.com/quakecstudio/editor/issues/21)
- **Requirements:** MVP-4, MVP-3

## Question

Is the design document's MVP In list the MVP, and which of the nine items the plan puts on the MVP
path without the In list naming them stay before the MVP gate?

## Context

The MVP is done when a modder can open the rerelease QuakeC, ask the agent for a new weapon, review
the diff and see it fire in a live game on macOS, Windows and Linux (MVP-1); it closes with the
Phase 4 gate, [#136](https://github.com/quakecstudio/editor/issues/136). The design document's In
list (MVP-4, restated in [`docs/scope.md`](../scope.md)) names the semantic core, the editor, our
compiler, the preview host with the FTEQW adapter on Windows and Linux, the agent panel and MCP
server, signed builds and mod export. The plan derives Phases 2 to 4 from the whole design
document, so it also schedules before the gate nine items the In list does not name. Each one either
stays (yes) or moves to the Post-MVP backlog (no), where it waits for the MVP gate and is linked from
`docs/scope.md` (MVP-3).

What each item is in the plan today:

| Item | Issue | Size | Design document | What depends on it |
| --- | --- | --- | --- | --- |
| ACP hosting | [#125](https://github.com/quakecstudio/editor/issues/125) P4-14 | M | Product principle 6; Editor and UI | Only the Phase 4 gate |
| Cmd/Ctrl+K inline edit | [#121](https://github.com/quakecstudio/editor/issues/121) P4-10 | M | Editor and UI | P4-11; an approved mockup (P0-25) |
| Next-edit suggestions | [#122](https://github.com/quakecstudio/editor/issues/122) P4-11 | M | Editor and UI | Only the Phase 4 gate; shown in the approved play-loop mockup |
| tree-sitter grammar | [#71](https://github.com/quakecstudio/editor/issues/71) P2-15 | M | Language core, Outputs | P2-16; risk R-8's mitigation |
| Editor extensions (VS Code, Zed, Neovim) | [#72](https://github.com/quakecstudio/editor/issues/72) P2-16 | M | Language core | Risk R-8's mitigation |
| Restart-loop adapters (QuakeSpasm, Ironwail, vkQuake) | [#96](https://github.com/quakecstudio/editor/issues/96) P3-08 | S | Engine bridge; Platform support | Only the Phase 3 gate |
| KEX rerelease compile target | [#97](https://github.com/quakecstudio/editor/issues/97) P3-09 | S | Platform support (compile target only) | Only the Phase 3 gate |
| Update mechanism | [#130](https://github.com/quakecstudio/editor/issues/130) P4-19 | S | Not named; implied by QA-11 | Only the Phase 4 gate |
| Map tools | P5-08 [#149](https://github.com/quakecstudio/editor/issues/149), P6-13 [#170](https://github.com/quakecstudio/editor/issues/170) | M | Map editor | Already in Phases 5 and 6, after the MVP gate |

## Options

The decision is the In list plus a yes or no per item, so the options are three cuts of the same
list.

| Option | Consequences |
| --- | --- |
| A. Everything the plan schedules stays (all yes) | No plan change. Phase 4 keeps 32 issues, including three (ACP, next-edit, updater) that the MVP demo does not use, so the MVP takes longer than it has to |
| B. In list only (all nine no) | Smallest MVP: six issues leave Phases 2 to 4. It also drops what the MVP's own story uses: the KEX target for the rerelease the demo is built on, the restart-loop adapter that mod export's acceptance test launches QuakeSpasm through, inline edit from the approved mockups, and the tree-sitter and LSP reach that mitigates risk R-8 |
| C. Keep what the MVP story and its risks need, move the rest (recommended) | Yes: Cmd/Ctrl+K, tree-sitter, editor extensions, restart-loop adapters, KEX target. No: ACP hosting, next-edit, update mechanism. Map tools: no, and nothing moves (already in Phases 5 and 6). Three issues move from Phase 4 to the Post-MVP backlog |

## Decision

Decided: **confirm the In list as written, and take option C.**

| Item | MVP | Why |
| --- | --- | --- |
| ACP hosting | No | The MCP server (In list) already lets outside agents such as Claude Code use the IDE's tools; running them inside the panel is a second integration with a younger protocol and is not needed for the demo |
| Cmd/Ctrl+K inline edit | Yes | Small on top of staged edits and hunk review (P4-04), and part of the approved mockups (P0-25) |
| Next-edit suggestions | No | Not needed for the demo, and it needs tuning to feel right. The approved play-loop mockup shows one, so the docs mark it as later |
| tree-sitter grammar | Yes | Highlighting in Zed and Neovim, and the main mitigation for risk R-8 (small audience) together with the LSP |
| Editor extensions | Yes | Thin wrappers around the LSP server that ships anyway; they reach modders in the editors they already use (R-8). They stay unpublished until the license review (P4-22) |
| Restart-loop adapters | Yes | Small once the adapter trait exists; lets modders preview in the engines they play with, and mod export's acceptance test (P4-28) launches the exported mod in QuakeSpasm |
| KEX target | Yes | The MVP is built on the rerelease, whose players run the KEX engine; compile-target only, small |
| Update mechanism | No | Releases are published on GitHub with checksums; an opt-in update check can follow the MVP without changing anything a user has |
| Map tools | No | Already after the MVP in Phases 5 and 6; no change |

These answers trim Phase 4, the longest phase before the gate, by three issues that the MVP demo
does not touch. They keep every item that the demo, mod export or a recorded risk relies on.

## Consequences

The plan changes are in this ADR's pull request:

- `plan/backlog/p4.yaml` to the LATER milestone in `plan/backlog/p5-p7.yaml`: P4-11 (#122), P4-14
  (#125) and P4-19 (#130), keeping their keys, so `plan/apply.py` moves the existing issues instead
  of creating new ones. They leave the Phase 4 gate's dependencies and gain the MVP gate (P4-25)
  through `plan/check.py`.
- `plan/backlog/readiness.yaml`, regenerated: the P4 L7 and apps readiness reviews no longer cover
  them.
- `plan/backlog/p4.yaml` P4-20: the user guide covers MCP only.
- `docs/scope.md`: three Post-MVP rows; `docs/roadmap.md`: Phase 4 no longer lists next-edit or ACP.
- Requirements UI-7, UI-11 and QA-11 are unchanged and still covered, now by post-MVP issues. The
  design document does not change: its In list is confirmed and does not name these items.
- After merge: `python3 plan/apply.py --repo quakecstudio/editor` moves the three issues to the
  Post-MVP milestone.

## Links

- Partly overridden by [ADR-0005](0005-default-preview-target.md): the IDE runs only our own preview host, so the outside-engine adapters are removed.

- Issue: [#21](https://github.com/quakecstudio/editor/issues/21); pull request: this ADR's draft PR.
- Design document: [MVP scope](../design/design-doc.md#mvp-scope), [Editor and UI](../design/design-doc.md#editor-and-ui), [Product principles](../design/design-doc.md#product-principles).
- [`docs/scope.md`](../scope.md), [`docs/risks.md`](../risks.md) (R-8), [`docs/design/README.md`](../design/README.md) (approved mockups).
- Related: ADR-0004, proposed in [#221](https://github.com/quakecstudio/editor/pull/221) (the preview host, and why the FTEQW adapter is optional).
