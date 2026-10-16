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
| [0003](0003-ui-framework.md) | Build the editor UI on egui | Proposed |
| [0004](0004-preview-host-base.md) | Build the preview host in Rust on wgpu | Accepted |
| [0005](0005-default-preview-target.md) | Run only our own preview host, with no outside engines | Accepted |
| [0006](0006-product-name.md) | Keep the name QuakeC Studio | Accepted |
| [0007](0007-mvp-scope.md) | Confirm the MVP In list and move ACP, next-edit and the updater after it | Accepted |
| [0008](0008-game-directories.md) | Launch engines from a linked run directory inside the project | Accepted |
| [0009](0009-test-readiness-bar.md) | Adopt the readiness bar, measured at readiness reviews | Accepted |
| [0010](0010-visual-design.md) | Use warm graphite with one ember accent, Plex and JetBrains Mono, and native chrome | Accepted |
| [0011](0011-app-directories.md) | Use each OS's native application directories | Accepted |
<!-- /adr:index -->

ADR-0003 to ADR-0010 are reserved by the Phase 0 issues that name them in their deliverables
(`python3 plan/status.py --phase P0` shows which are still open). Pick a new number above every
file here and every number the plan reserves.
