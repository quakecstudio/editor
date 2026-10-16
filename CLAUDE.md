# CLAUDE.md

Guidance for Claude Code and other agents working in this repository. [`AGENTS.md`](AGENTS.md)
covers environment setup; [`.claude/rules/`](.claude/rules/) holds the detailed rules this file
summarises.

## Project overview

QuakeC Studio: a Rust IDE for QuakeC with its own compiler, live in-game preview, a built-in
agent and a map editor, on macOS, Windows and Linux. The core principle: **the foundations
model what the Quake engine actually executes**, ported from id's GPL source and proven
against it, so every feature above agrees with the game.

Read [`docs/roadmap.md`](docs/roadmap.md) first: it is the overall plan and says where we
are. Then [`docs/design/design-doc.md`](docs/design/design-doc.md) for the why,
[`docs/scope.md`](docs/scope.md) for what the MVP includes, and
[`docs/risks.md`](docs/risks.md) for what could go wrong.

## Architecture quick reference

```
editor/
├── crates/        qs-* library crates, one per layer component; qs-tests: the one cross-crate test binary (tests/it)
├── apps/          desktop app, CLI, LSP server binaries (names from ADR-0006)
├── xtask/         build automation: cargo xtask <command>, called from make
├── docs/          roadmap, design and mockups, ADRs (adr/), engine behaviour specs (spec/), conventions, testing bar
├── specs/         cached public specifications (LSP, MCP, ACP), verbatim; grep before fetching
├── plan/          requirements and backlog as data; GitHub issues are generated from it
├── scripts/       refresh-specs.sh
└── .claude/       rules/, skills/, agents/, hooks/
```

**Layers.** A crate may depend only on crates in its own layer or a lower one;
`layers.toml` maps every crate and `cargo xtask layers` enforces it.
[`docs/layers.md`](docs/layers.md) explains the model and
[`.claude/rules/layering.md`](.claude/rules/layering.md) has the table.

| Layer | Holds |
| --- | --- |
| L0 formats | progs.dat, .lno, PAK and WAD2, .map, BSP29/BSP2, MDL; the paths module (`qs-paths`) |
| L1 VM | qc-vm, qc-edict, qc-builtins: what the engine executes |
| L2 syntax | Preprocessor, lossless CST, progs.src project graph |
| L3 semantics | Resolution, types, dialects, engine-target database, LSP, map entity definitions |
| L4 bridges | Our QuakeC and map compilers; the engine bridge |
| L5 commands | Command engine, session, undo: one registry for UI, CLI, agent and MCP |
| L6 agent | Agent loop, model providers, checkpoints, MCP and ACP |
| L7 UI | The app on the chosen framework |
| apps | Desktop app, CLI, LSP server |

**Separate processes.** The oracles (id's VM and qcc, a pinned FTEQCC, later id's map tools
and ericw-tools, and id's engine as the engine oracle) are C programs in
`oracles/`, run only in CI to prove our ports match. The preview host is our own Rust app in
`apps/preview-host` ([ADR-0004](docs/adr/0004-preview-host-base.md)), built on our QuakeC VM,
`qs-sim`, `qs-sound` and the wgpu renderer, and runs as its own process, talking to the IDE over
a local socket. The game runs in its own docked window in v1; the embedded game view is
post-MVP.

**Two kinds of spec.** Engine and format behaviour is written up in
[`docs/spec/`](docs/spec/README.md) from id's source, with stable statement IDs that code and
tests cite. Public protocols we implement are cached verbatim in [`specs/`](specs/README.md).

## How work is organised

- Every piece of work is a GitHub issue generated from [`plan/`](plan/README.md). Each issue
  states its deliverable, acceptance criteria, verification, requirements and dependencies,
  and carries the shared definition of done from `plan/backlog/_meta.yaml`.
- Phases run in order (P0 to P7, then LATER). An issue depends on the previous phase's gate,
  and on the readiness review of any lower layer it builds on (`docs/testing.md`). Do not
  start an issue whose dependencies are open.
- To change the plan, edit `plan/requirements.yaml` or `plan/backlog/*.yaml`, run the checks
  below, open a PR, and sync GitHub with `plan/apply.py` after it merges. Do not edit issue
  bodies by hand; the sync overwrites them.
- Decisions go in `docs/adr/`, from `docs/adr/0000-template.md`;
  [ADR-0001](docs/adr/0001-record-decisions.md) is the process and `cargo xtask adr` checks it.
- `python3 plan/status.py --phase P0` shows what is closed, ready and waiting in a phase.

## Working autonomously

Use the project skills and subagents; [`docs/agents/README.md`](docs/agents/README.md)
explains them and how to run a whole phase under `/goal`.

- `/work-issue <n>` for one issue, `/run-phase <phase>` to drive a phase through the
  `implementer` and `reviewer` subagents.
- `needs:human` issues (decisions, hardware, credentials, legal, the MVP release) are
  prepared, never closed, by agents: `/prepare-decision`. When an agent stops on something
  only a person can resolve, it labels the issue `escalated` and comments.
- `/readiness-review` for `type:readiness` issues, `/gate-review` to close a phase gate once
  nothing in the phase straggles, `/plan-change` to change the plan.

## Commands

```bash
make ci                                   # exactly what CI runs (cargo xtask ci); must pass before any merge

make help                                 # every Makefile target: fmt, lint, test, test-coverage, test-mutants, audit, ...

cargo build --locked
cargo test --locked                       # every unit test, plus the qs-tests binary
cargo test -p qs-tests                    # cross-crate tests, including the spec-citation ratchet
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo deny check advisories bans licenses sources

cargo xtask conventions                   # config files match docs/conventions.md
cargo xtask adr                           # ADRs follow the template and are indexed
cargo xtask clippy-fixtures               # the lints reject what they target
cargo xtask deny-fixtures                 # deny.toml rejects bad licenses and wildcards (needs network)
cargo xtask layers                        # no crate depends on a higher layer (layers.toml)
cargo xtask licenses                      # SPDX lines and ported-file headers; skips cached specs
cargo xtask attribution                   # every asset is listed in ATTRIBUTION.md
cargo xtask oracles                       # build the oracles from id's sources and run the VM vectors (network, C compiler)
cargo xtask fuzz qc-progs --time 10m      # one fuzz target (nightly, cargo-fuzz); no target runs all

scripts/refresh-specs.sh --verify         # cached specs match specs/manifest.tsv (offline)
scripts/refresh-specs.sh                  # refresh specs/ from the origins
prek run                                  # the pre-commit hooks
```

`make ci` runs every `cargo` and `cargo xtask` step above and `refresh-specs.sh --verify`,
but never a refresh; the individual commands are for narrowing down a failure.

The plan:

```bash
python3 plan/check.py --write-coverage            # every requirement covered, dependencies valid, templates complete
(cd plan && python3 -m unittest test_check.py)    # the checker's own tests
python3 plan/gen_readiness.py                     # after adding or relabelling feature or test issues
python3 plan/apply.py --repo quakecstudio/editor  # sync labels, milestones and issues (idempotent)
python3 plan/status.py                            # the current phase's progress; the last line is the verdict
python3 plan/status.py --readme                   # refresh the README's Next run block (current phase and /goal command)
(cd plan && python3 -m unittest test_status.py)   # status tests
```

**Toolchain:** Rust 1.99.0, edition 2024, pinned in `rust-toolchain.toml`; max line width 100.
Python 3 with PyYAML for `plan/`.

## Code conventions

[`docs/conventions.md`](docs/conventions.md) is the full set, adopted from vouch, and
`cargo xtask conventions` checks the configuration against it.

### No panics

Shipped crates never panic. Clippy denies, among others: `unwrap_used`, `expect_used`,
`panic`, `unreachable`, `todo`, `unimplemented`, `exit`; `indexing_slicing` and
`string_slice` (use `.get()`); `arithmetic_side_effects`, `integer_division`,
`modulo_arithmetic` (use `checked_*`, `wrapping_*` or `saturating_*`, whichever the spec
says); the `cast_*` lints (use `try_from` or a spelled-out conversion); `float_cmp`. `unsafe`
is denied everywhere.

An exception is `#[expect(lint, reason = "...")]` at the narrowest scope, never `#[allow]`
and never crate-wide in a shipped crate. In the VM and compiler the reason cites the spec
statement: `#[expect(clippy::float_cmp, reason = "spec VM-12.3: exact IEEE comparison")]`.
Test modules opt out locally with a module-level `#![expect(..., reason = "test code: ...")]`.

### Porting from id's source

The whole project is GPL-3.0-or-later ([ADR-0002](docs/adr/0002-license-model.md)), so
id's source may be ported directly. Keep id's copyright line, cite the file, function and
commit in the `Ported from` header line, document the behaviour in `docs/spec/`, and prove
it with a differential test against the oracle. [`docs/porting.md`](docs/porting.md) is the
process, with a worked example; [`.claude/rules/porting.md`](.claude/rules/porting.md) is the
checklist.

### Everything else

- **Files:** every source file starts with `// SPDX-License-Identifier: GPL-3.0-or-later`.
- **Dependencies:** declared once in `[workspace.dependencies]` with an exact version and
  `default-features = false`; licenses compatible with GPL-3.0-or-later (`deny.toml`). Add
  sparingly, and justify each one.
- **Diagnostics:** `tracing`, never `println!`. Paths come only from `qs-paths`.
- **Docs:** public items have rustdoc; every crate README states its purpose, layer and the
  spec sections it implements.
- **UI:** match the approved mockups in [`docs/design/`](docs/design/README.md).

## Testing

[`.claude/rules/testing.md`](.claude/rules/testing.md) has the detail;
[`docs/testing.md`](docs/testing.md) is the bar.

- A test of one crate lives in that crate. Cross-crate tests are modules in
  `crates/qs-tests/tests/it/`, never new files under `tests/`.
- **Cite the statement a test pins:** `// spec VM-12.3: <paraphrase>` on the line above. The
  spec-citation ratchet (`crates/qs-tests/tests/it/spec_coverage.rs`) fails when a statement
  in `docs/spec/` loses its last citing test, and `docs/spec/requirements.tsv` must list every
  statement.
- Fixtures come from factories taking a spec struct whose `Default` is the ordinary case.
- Tests never wait on the wall clock.
- **Tests before climbing:** a layer's dependants wait until its suite passes readiness.

## Git

`main` only accepts changes through the merge queue: work on a branch named
`<type>/<issue>-<slug>` and open a PR with `Closes #<issue>`. Conventional Commits, at most 72
characters, imperative. **No Co-Authored-By trailers.** An agent may add its own PR to the
merge queue once CI passes (`.claude/skills/work-issue/scripts/land.sh`), except for
`needs:human` issues, which wait for Justin. Parallel agents use separate worktrees. See
[`.claude/rules/branching.md`](.claude/rules/branching.md) and
[`.claude/rules/commits-and-issues.md`](.claude/rules/commits-and-issues.md).

## What not to do

1. **Don't panic in shipped code**: no `unwrap`, `expect`, `panic!`, `[]` indexing, unchecked
   arithmetic or `as` casts. Write out what C does instead.
2. **Don't state what a spec, or id's code, requires from memory.** Quote it: the section and
   the verbatim text, from `specs/`, `docs/spec/`, id's source at the pinned commit, or a URL
   fetched this session. Report MUST, SHOULD and MAY as written, and say so when the source is
   silent. See [`.claude/rules/specs-are-source-of-truth.md`](.claude/rules/specs-are-source-of-truth.md).
3. **Don't port without the four parts**: header with id's copyright and citation, spec
   section, citing tests, differential result.
4. **Don't depend upward** across layers, and don't put logic in the UI that belongs in a
   command.
5. **Never bundle Quake game data** (pak0/pak1) or QuakeC 1.06, which has no license. Never
   commit id's C; fetch third-party sources by commit and checksum.
6. **Don't edit issue bodies on GitHub** or leave follow-up work in a comment: change `plan/`.
7. **Don't weaken a check to get green**: no skipped tests, loosened lints, lowered ratchets or
   new baseline entries for code you wrote.
8. **Don't decide for Justin.** `needs:human` work is prepared, not merged.
9. **Don't diagnose with commands that write**, and after three identical failures stop and
   re-diagnose ([`.claude/rules/development-discipline.md`](.claude/rules/development-discipline.md)).
10. **Don't use em or en dashes** in docs, commits or messages; use plain hyphens.

## File locations

| Need | Location |
| --- | --- |
| The plan: requirements and issues | `plan/requirements.yaml`, `plan/backlog/*.yaml` |
| Overall plan and current phase | `docs/roadmap.md` |
| Design and approved mockups | `docs/design/` |
| Decisions | `docs/adr/` (template `0000-template.md`) |
| Engine behaviour specs | `docs/spec/` (index `requirements.tsv`, baseline `coverage-baseline.tsv`) |
| Cached public specs | `specs/` (roster `specs/manifest.tsv`) |
| Porting process | `docs/porting.md`; worked example `crates/qs-porting-example/` |
| Rust conventions | `docs/conventions.md` |
| Testing bar and readiness reports | `docs/testing.md`, `docs/readiness/` |
| Gate reports | `docs/gates/` |
| Cross-crate tests | `crates/qs-tests/tests/it/` (one module per area) |
| App directories | `crates/qs-paths/` |
| Agent rules, skills, subagents, hooks | `.claude/rules/`, `.claude/skills/`, `.claude/agents/`, `.claude/hooks/` |
| Issue and PR templates | `.github/ISSUE_TEMPLATE/`, `.github/pull_request_template.md` |

## Specifications

Cached in `specs/`, byte-identical to their origins (`specs/README.md`); grep there before
fetching:

- [Language Server Protocol 3.18](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.18/specification/): `specs/lsp/3.18/`
- [Model Context Protocol 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28): `specs/mcp/2026-07-28/`
- [Agent Client Protocol v1](https://agentclientprotocol.com/protocol/overview): `specs/acp/v1/`

id's source, the specification for everything in L0 and L1, is pinned in
[`docs/porting.md`](docs/porting.md#upstreams): `id-Software/Quake` and the `qcc/` directory of
`id-Software/Quake-Tools`.
