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
| [0003](0003-ui-framework.md) | Build the editor UI on egui | Accepted |
| [0004](0004-preview-host-base.md) | Build the preview host in Rust on wgpu | Accepted |
| [0005](0005-default-preview-target.md) | Run only our own preview host, with no outside engines | Accepted |
| [0006](0006-product-name.md) | Keep the name QuakeC Studio | Accepted |
| [0007](0007-mvp-scope.md) | Confirm the MVP In list and move ACP, next-edit and the updater after it | Accepted |
| [0008](0008-game-directories.md) | Launch engines from a linked run directory inside the project | Accepted |
| [0009](0009-test-readiness-bar.md) | Adopt the readiness bar, measured at readiness reviews | Accepted |
| [0010](0010-visual-design.md) | Use warm graphite with one ember accent, Plex and JetBrains Mono, and native chrome | Accepted |
| [0011](0011-app-directories.md) | Use each OS's native application directories | Accepted |
| [0012](0012-bsp-loader-default.md) | Load BSPs by the FitzQuake lineage's rules by default, with WinQuake and GLQuake as modes | Accepted |
| [0013](0013-differential-coverage-bar.md) | Make zero divergences the differential suite's bar and report coverage without a flat 90% gate | Accepted |
| [0014](0014-engine-fact-sources.md) | Use FTEQW's and DarkPlaces' extension files as fact-only sources | Superseded by ADR-0015 |
| [0015](0015-darkplaces-client-menu-fact-sources.md) | Also use DarkPlaces' client and menu QuakeC definitions as fact-only sources | Accepted |
| [0016](0016-quakespasm-lineage-fact-sources.md) | Use the QuakeSpasm lineage's builtin tables as fact-only sources | Accepted |
| [0017](0017-port-fteqcc.md) | Port FTEQCC's compiler behaviour from FTEQW's source | Accepted |
| [0018](0018-quakespasm-lineage-porting-upstreams.md) | Make QuakeSpasm, Ironwail and vkQuake porting upstreams for the preview host | Accepted |
<!-- /adr:index -->

A `type:decision` issue reserves the ADR number its deliverable names in `plan/backlog/`;
every number the plan reserves so far has its file here. Pick a new number above every file
here and every number the plan reserves.
