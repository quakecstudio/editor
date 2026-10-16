# ADR-NNNN: Short title in the imperative, naming the choice

- **Status:** Proposed
- **Date:** YYYY-MM-DD
- **Decided by:** pending
- **Issue:** [#N](https://github.com/quakecstudio/editor/issues/N)
- **Requirements:** REQ-1, REQ-2

<!--
Copy this file to docs/adr/NNNN-slug.md, using the number and file name the decision issue's
deliverable names, and add a row to the index in docs/adr/README.md. `cargo xtask adr` checks
the header, the sections below (in this order) and the index. Plain hyphens, no em or en dashes.

Status is one of: Proposed, Accepted, Rejected, Deprecated, Superseded by ADR-NNNN.
An agent writes an ADR as Proposed; only a person marks it Accepted (ADR-0001). On acceptance,
set Date to the day it was decided and Decided by to that person. Delete this comment.
-->

## Question

The decision in one sentence, phrased as a question.

## Context

What forces the decision now: the requirement IDs and design-document sections it answers, the
issues and layers waiting on it, and the facts it rests on. Cite every source in Links and label
anything that could not be verified.

## Options

Two to four real options. For each, what it is and its consequences for the plan: which issues,
layers or phases it changes. Once the ADR is accepted, keep the options that were not chosen and
say why not.

| Option | Consequences |
| --- | --- |
| A | ... |
| B | ... |

## Decision

While Proposed: the recommendation and why, in one paragraph. Once Accepted: the decision,
stated so a reader can act on it without the rest of the document, and why.

## Consequences

What changes because of the decision: in `plan/` (requirements, issues, dependencies), in the
code and conventions, and what becomes impossible or harder. Name the issues that carry each
change.

## Links

- The decision issue and the pull request that merged this ADR.
- Related ADRs: ADR-NNNN, and any ADR this one supersedes or is superseded by.
- Sources: every page, file or document the context and options rely on.
