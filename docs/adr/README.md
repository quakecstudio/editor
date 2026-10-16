# Architecture decision records

Every decision that shapes the plan is a `type:decision` issue that resolves into one ADR here.
[ADR-0001](0001-record-decisions.md) is the process: how ADRs are numbered, who may accept them,
and how a decision is changed. New ADRs start from [`0000-template.md`](0000-template.md); the
`prepare-decision` skill drafts them for a person to decide.

`cargo xtask adr` checks that every ADR has the template's header and sections and that the
index below lists every ADR, in order, with the title and status its file states.

## Index

<!-- adr:index -->
| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-record-decisions.md) | Record decisions as ADRs resolved from decision issues | Accepted |
| [0002](0002-license-model.md) | License the whole project GPL-3.0-or-later | Accepted |
| [0004](0004-preview-host-base.md) | Fork QuakeSpasm as the base of the preview host | Proposed |
| [0011](0011-app-directories.md) | Use each OS's native application directories | Accepted |
<!-- /adr:index -->

ADR-0003 to ADR-0010 are reserved by the Phase 0 issues that name them in their deliverables
(`python3 plan/status.py --phase P0` shows which are still open). Pick a new number above every
file here and every number the plan reserves.
