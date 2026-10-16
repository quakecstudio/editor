# ADR-0001: Record decisions as ADRs resolved from decision issues

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** Justin Plock, in the approved design document (PM-2); written up by the P0-07 implementer
- **Issue:** [#7](https://github.com/quakecstudio/editor/issues/7)
- **Requirements:** PM-2

## Question

How does QuakeC Studio make and record the decisions that shape the plan, so that people and
agents can find what was decided, why, and what it changed?

## Context

The design document's project-management section asks for decisions to be tracked as issues and
resolved into architecture decision records in `docs/adr/` (PM-2). Much of the work is done by
agents, which cannot make product, legal or spending decisions and need a written answer they
can cite. Phase 0 already reserves ADR numbers in its decision issues' deliverables: ADR-0002 for
the license model, ADR-0003 for the UI framework spike (P0-10), ADR-0004 to ADR-0010 for the
open decisions, and ADR-0011 was taken by the workspace scaffold (P0-01) for application
directories.

## Options

| Option | Consequences |
| --- | --- |
| Decisions in issue comments only | Nothing to cite from code or docs; the reasoning is lost when the issue scrolls, and agents cannot find it from the repository |
| Free-form notes in `docs/` | No fixed shape, so a reader cannot tell a proposal from a decision, and nothing can be checked |
| ADRs from a template in `docs/adr/`, one per decision issue, with an index checked by `cargo xtask adr` (chosen) | One file per decision, a fixed set of sections, a status that says whether it is settled, and checks that keep the index honest |

## Decision

**Every decision is a `type:decision` issue that resolves into one ADR in `docs/adr/`.**

- **One issue, one file.** A `type:decision` issue names its ADR file, `docs/adr/NNNN-slug.md`,
  in its deliverable; `plan/check.py` fails a decision issue that does not. Decision issues are
  also `needs:human`.
- **Numbers.** Numbers are four digits and never reused; 0000 is the template. The plan
  reserves a number when an issue names its file; a new ADR takes the next number above every
  existing file and every number the plan has reserved. A plan item that cannot know its number
  yet writes `00NN` and takes the next free number when the ADR is written.
- **Template.** Every ADR follows [`0000-template.md`](0000-template.md): a header with Status,
  Date, Decided by, Issue and Requirements, then the sections Question, Context, Options,
  Decision, Consequences and Links, in that order. Further sections, such as Why, may sit
  between them.
- **Status.** One of Proposed, Accepted, Rejected, Deprecated or Superseded by ADR-NNNN. An
  agent writes an ADR as Proposed with a recommendation and opens it as a draft pull request
  (the `prepare-decision` skill); only a person marks it Accepted and merges it, which closes
  the issue. An implementer may record a choice the plan explicitly delegates to it, as
  ADR-0011 does, and says so in Decided by.
- **Changing a decision.** An accepted ADR is not rewritten to say something else. A new ADR
  supersedes it, and the old one's status becomes Superseded by the new number. Corrections
  that do not change the decision (links, typos, the section layout) are fine.
- **Index.** [`README.md`](README.md) lists every ADR with its number, title and status between
  the `adr:index` markers. `cargo xtask adr` fails when the index, a header or the sections
  disagree with this ADR.

## Consequences

- Each decision issue (P0-08, P0-12, P0-14, P0-15, P0-21, P0-22, P0-24, P0-25 and P4-22) has a
  fixed place for its answer, and `prepare-decision` drafts it from the template. The P0-10
  spike records its outcome the same way, in ADR-0003.
- `plan/check.py` checks that every decision issue names an ADR file; `cargo xtask adr` checks
  the ADRs themselves. `cargo xtask ci` (P0-02) runs `adr` with the other checks.
- ADR-0002 and ADR-0011, written before the template existed, follow it.

## Links

- Issue: [#7](https://github.com/quakecstudio/editor/issues/7)
- Design document, project management: [`docs/design/design-doc.md`](../design/design-doc.md) (PM-2 in [`plan/requirements.yaml`](../../plan/requirements.yaml))
- The `prepare-decision` skill: [`.claude/skills/prepare-decision/SKILL.md`](../../.claude/skills/prepare-decision/SKILL.md)
- Michael Nygard, [Documenting Architecture Decisions](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
