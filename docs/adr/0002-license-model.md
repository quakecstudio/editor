# ADR-0002: License the whole project GPL-3.0-or-later

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** Justin Plock
- **Issue:** [#8](https://github.com/quakecstudio/editor/issues/8)
- **Requirements:** LIC-1, LIC-2, LIC-3, LIC-6, LIC-7, ARCH-2, MAP-11, PM-3

## Question

Is QuakeC Studio licensed permissively, with id's GPL code kept at arm's length, or GPL throughout?

## Decision

The whole project is licensed **GPL-3.0-or-later**: the IDE, its crates and binaries, the oracles in `oracles/`, and the preview host fork. Every source file carries `SPDX-License-Identifier: GPL-3.0-or-later`; files ported from id's code also keep id's copyright line. `LICENSE` holds the GPL-3.0 text.

## Why

- **We can build on the real thing.** id released the Quake engine and tools under the GPL, "version 2 of the License, or (at your option) any later version" ([`pr_exec.c` header](https://raw.githubusercontent.com/id-Software/Quake/master/WinQuake/pr_exec.c)). Under the GPL we can port id's VM, formats and tools directly instead of re-deriving them through a clean room, which removes a whole layer of process, roles and risk.
- **Version 3, not version 2.** The recommended UI framework, gpui, is licensed Apache-2.0 only ([`crates/gpui/Cargo.toml`](https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/Cargo.toml)), as are other Rust crates we are likely to use. Apache-2.0 is compatible with GPLv3 but not with GPLv2, so a GPL-2.0-only project could not use them. id's "or later" grant lets us use their code under version 3.
- **It matches the community.** FTEQCC, QuakeSpasm and most Quake source ports are GPL, and so is Zed itself.

## Alternatives considered

| Option | Why not |
| --- | --- |
| MIT OR Apache-2.0 core with a clean room, GPL tools out of process (the design document's original recommendation) | Needs separate spec authors and implementers, a reader log, a process boundary and a legal review of that boundary; slower and riskier, for a benefit (closed forks) the project does not need |
| GPL-2.0-or-later | Cannot combine with Apache-2.0-only dependencies such as gpui |

## Consequences

- **No clean room.** The spec-author and implementer roles, the reader log, the clean-room labels and the guard hook are removed. P0-09 becomes "Define the porting process" (`docs/porting.md`).
- **Specs stay.** `docs/spec/` still documents formats and behaviour, now written with id's source open, so tests can cite it and agents have it as context.
- **Ports are encouraged** and keep id's copyright notice and a citation of the file and function they came from. Differential tests against id's original code remain the proof that a port behaves the same.
- **Oracles move into this repo** (`oracles/`, built by `cargo xtask oracles`); third-party sources are fetched by commit and checksum, not vendored. `quakecstudio/oracles` is no longer needed.
- **The preview host** stays a separate program and repository because it is a C engine fork with its own build, not for licensing reasons.
- **Dependencies** must be GPL-3.0-or-later compatible; `deny.toml` (P0-01) and `cargo xtask licenses` (P0-02) enforce it.
- **Distribution:** every package ships the GPL-3.0 text, notices and a corresponding-source link. Closed-source forks, and plugins linked into the app, are not possible; tools that talk to the app over MCP or ACP are unaffected. The Mac App Store is not a distribution channel.
- **Still never in the repo:** Quake's game data (pak0/pak1) and QuakeC 1.06, which has no license.
- P4-22 becomes a license compliance review (notices, corresponding source, dependency licenses) rather than a legal review of a process boundary.

This is not legal advice; the license compliance review before the first public release (P4-22) confirms the details.
