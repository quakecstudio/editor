# Rust conventions

QuakeC Studio follows the conventions of [vouch-sh/vouch](https://github.com/vouch-sh/vouch).
Where this project departs from vouch, the departure and its reason are stated inline
as **Differs from vouch**. Requirement IDs (`CONV-n`) are tracked in
`plan/requirements.yaml`; issue P0-01 puts the configuration in place.

Reference: vouch `main` as of 2026-10-15 (`Cargo.toml`, `.rustfmt.toml`, `.clippy.toml`,
`deny.toml`, `rust-toolchain.toml`, `.pre-commit-config.yaml`, `.github/workflows/`,
`CLAUDE.md`, `AGENTS.md`, `.claude/rules/`).

## Toolchain and workspace (CONV-1)

- `rust-toolchain.toml` pins an exact stable channel (vouch: `1.99.0`) with `rustfmt` and `clippy` components.
- Root `Cargo.toml`: `resolver = "3"`, `members = ["crates/*", "apps/*", "xtask"]`.
- `[workspace.package]` holds `version`, `edition = "2024"`, `license`, `rust-version`,
  `repository`, `publish = false`; every crate inherits them with `*.workspace = true`.
- Version is CalVer `YYYY.MM.N`, as in vouch.
- Every crate has `[lints] workspace = true`.

## Formatting (CONV-2)

`.rustfmt.toml`, identical to vouch:

```toml
edition = "2024"
max_width = 100
hard_tabs = false
tab_spaces = 4
use_field_init_shorthand = true
newline_style = "Unix"
```

`cargo fmt --all --check` runs in CI and in the pre-commit hook. Formatting is never a follow-up commit.

## Lints (CONV-3)

`[workspace.lints.rust]`, as in vouch: `unsafe_code = "deny"`, `unreachable_pub = "warn"`,
`non_ascii_idents = "deny"`, `trivial_casts`, `trivial_numeric_casts`, `unused_lifetimes`,
`single_use_lifetimes` = `"warn"`.

`[workspace.lints.clippy]`, the vouch no-panic set, all `deny`:

| Group | Lints |
| --- | --- |
| Explicit panics | `unwrap_used`, `expect_used`, `panic`, `unreachable`, `todo`, `unimplemented`, `exit` |
| Panics in `Result` fns | `unwrap_in_result`, `panic_in_result_fn`, `get_unwrap` |
| Indexing | `indexing_slicing`, `string_slice` |
| Arithmetic | `arithmetic_side_effects`, `integer_division`, `modulo_arithmetic` |
| Casts | `cast_possible_truncation`, `cast_sign_loss`, `cast_possible_wrap`, `cast_precision_loss`, `checked_conversions` |
| Floats | `float_cmp`, `float_cmp_const`, `lossy_float_literal`, `zero_divided_by_zero` |
| Correctness | `fallible_impl_from`, `recursive_format_impl`, `infinite_iter`, `unchecked_time_subtraction` |
| Stack | `large_stack_arrays`, `large_stack_frames` |
| UB belt-and-braces | `mem_replace_with_uninit`, `uninit_assumed_init`, `uninit_vec` |
| Hygiene | `dbg_macro`, `print_stdout`, `print_stderr`, `let_underscore_must_use`, `map_unwrap_or`, `allow_attributes_without_reason` |
| Concurrency | `await_holding_lock`, `large_futures`, `mem_forget`, `mutex_atomic`, `rc_buffer` |
| Paths | `absolute_paths` (configured in `.clippy.toml`) |

`too_many_lines = "warn"` with a threshold of 150 in `.clippy.toml`, enforced by `-D warnings`;
a longer function carries `#[expect(clippy::too_many_lines, reason = "...")]`.
`module_name_repetitions` and `similar_names` are allowed.

`.clippy.toml` copies vouch's `absolute-paths-max-segments = 2`, an
`absolute-paths-allowed-crates` list naming every external crate,
`cognitive-complexity-threshold = 30`, `too-many-lines-threshold = 150`,
`allowed-idents-below-min-chars = ["x", "y", "z", "i", "j", "k", "n", "s", "e"]` and
`avoid-breaking-exported-api = false`.

Exceptions are made with `#[expect(lint, reason = "...")]`, never `#[allow]`, and never
crate-wide in a shipped crate.

**This matters more here than in vouch.** The VM and compiler must reproduce C's integer
and float behaviour exactly. `arithmetic_side_effects` and the cast lints force every
wrap, saturate or checked conversion to be spelled out, which is how the spec's
semantics become visible in the code.

**Differs from vouch, to be confirmed in P0-01:** rendering and windowing (wgpu surfaces,
platform window handles) may need `unsafe`. If so, it is confined to one platform crate
with a module-level `#[expect(unsafe_code, reason = "...")]` and a `SAFETY:` comment on
every block. No other crate may use `unsafe`.

Test code keeps `[lints] workspace = true`. As in vouch, each test module opts out of the
no-panic lints locally with
`#![expect(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, reason = "test code: ...")]`.

**Float comparisons in the VM and compiler are exact by specification.** vouch's comment on
`float_cmp` says to compare with an epsilon; that is wrong for `qc-vm` and `qc-codegen`, whose
semantics require exact IEEE comparison. There the lint is expected per site with
`#[expect(clippy::float_cmp, reason = "spec VM-n.m: exact comparison")]`, never replaced by an epsilon.

## Dependencies (CONV-4)

- Every dependency is declared once in `[workspace.dependencies]` with an exact version
  (`version = "=x.y.z"`) and `default-features = false`.
- Crates use `dep = { workspace = true, features = [...] }` and enable only the features they need.
- Add dependencies sparingly. Preferences, from vouch where they apply:
  `rustls` over OpenSSL, `jiff` over `chrono`, `clap` for CLIs, `thiserror` for library
  errors, `anyhow` only in binaries, `tracing` for diagnostics, `secrecy` and `zeroize`
  for model-provider API keys, the `keyring` crates for OS keychains.

## cargo-deny (CONV-5)

`deny.toml` follows vouch: `[graph] all-features = true` with our six targets
(`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`,
`aarch64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`;
**differs from vouch** twice: vouch targets musl on Linux, but a GPU desktop app links the
system graphics stack, so Linux builds use glibc; and vouch has no `x86_64-apple-darwin`, which
this project adds for the universal macOS build);
`[advisories]` with every ignore justified in a comment; `[licenses]` allowing only licenses compatible with GPL-3.0-or-later
(**differs from vouch**: no GPL-2.0-only, no proprietary; ADR-0002) and `confidence-threshold = 0.8`; `[bans]` with `wildcards = "deny"` and
`multiple-versions = "warn"`, each `skip` carrying a `reason`; `[sources]` with
`unknown-registry = "deny"`, `unknown-git = "warn"`, crates.io as the only registry and every git
source pinned to a rev. Runs as `make audit` and in CI via `cargo-deny-action` with
`check advisories bans licenses sources`.

## Build profiles (CONV-6)

```toml
[profile.release]
strip = true
lto = true
codegen-units = 1
panic = "unwind"
opt-level = 3

[profile.ci]
inherits = "release"
lto = "thin"
codegen-units = 16
```

**Differs from vouch:** vouch uses `opt-level = "z"` (size). An editor, compiler and renderer
are latency-bound, so this project uses `opt-level = 3`. The P0-11 benchmarks confirm the choice.

**Differs from vouch:** vouch uses `panic = "abort"`. Here release builds use `panic = "unwind"`,
because the command guard (PRIN-7) must catch a panic in one command and keep the session
alive. Panics remain denied by lint; unwinding is the last line of defence, not a pattern.

## CI (CONV-7)

As in vouch:
- Every third-party action is pinned to a full commit SHA with a version comment, and a
  workflow fails any PR that adds an unpinned one (`zgosalvez/github-actions-ensure-sha-pinned-actions`).
- Workflows are linted with `zizmor`. They use least-privilege `permissions`, set
  `persist-credentials: false` on checkout, and keep static matrix values only.
- Separate jobs: Format, Clippy (`--locked --all-targets --all-features -- -D warnings`),
  Unit Tests (Linux, macOS, Windows), Build (every target, `--profile ci --locked`),
  cargo-deny, dependency-review.
- `Swatinem/rust-cache` for caching; `--locked` on every cargo command; `CARGO_BUILD_WARNINGS: deny`
  in the workflow environment, as in vouch.

## Make targets (CONV-8)

`Makefile` is the entry point, as in vouch; logic that needs Rust lives in `xtask` and is
called from `make`. Targets: `fmt`, `lint`, `check`, `test`, `test-integration`,
`test-fuzz`, `test-coverage` (cargo-llvm-cov), `test-mutants` (cargo-mutants), `audit`,
`docs-build`, `help`, as in vouch. **Additions:** `bench` (performance budgets) and `ci`
(runs exactly what CI runs).

## Pre-commit hooks (CONV-9)

`.pre-commit-config.yaml` run with `prek`, SHA-pinned, installed for pre-commit and
pre-push, matching vouch's hook set:
- pre-commit: trailing whitespace, end-of-file, merge conflicts, large files (1 MB), YAML and
  TOML checks, private-key detection, `mixed-line-ending --fix=lf`, `cargo fmt`, shellcheck,
  shfmt, actionlint, zizmor.
- pre-push: `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test`.

Vendored spec text under `specs/` is excluded so cached text stays byte-identical.

## Branches, commits and PRs (CONV-10)

- Conventional Commits 1.0.0; description imperative, no trailing period, at most 72 characters.
- Branches: `feat/{issue}-{slug}`, `fix/{issue}-{slug}`, `{type}/{issue}-{slug}`.
- Never push to `main`. PRs are squash-merged and the PR title becomes the commit subject.
- Agents working in parallel each use their own worktree.
- **Differs from vouch:** no `Co-Authored-By` trailers on commits (project owner's rule).

## Tests (CONV-11)

From vouch's CLAUDE.md:
- A test that exercises one crate lives in that crate. Tests that need several crates go in
  the single `qs-tests` integration binary (`crates/qs-tests/tests/it/`) as a new module,
  never a new file under `tests/`.
- Inline `#[cfg(test)] mod tests` until about 500 lines, then a sibling `foo/tests.rs`.
- Fixtures come from factory functions taking a spec struct whose `Default` is the ordinary
  case; tests spell out only the axis they test.
- Tests never wait on the wall clock. Rust crates take an injected clock; the preview host
  is covered instead by its fixed-timestep headless mode (P3-05).
- **A test that pins a normative statement cites it** by its stable ID, e.g. `// spec VM-12.3:
  division by zero yields ...`. This project applies the rule to `docs/spec/` statements; the
  ID scheme is in [`spec/README.md`](spec/README.md).
  `docs/spec/requirements.tsv` lists every normative statement, and a coverage test
  ratchets it, as `specs/requirements.tsv` and `spec_coverage.rs` do in vouch.

## Specifications are the source of truth (CONV-12)

vouch's rule: never state what a specification requires from memory; open it and quote
it with section and source. Here that means:
- **Behaviour specs** (`docs/spec/`) document engine and format behaviour, written with id's
  source open and citing the file and function; tests cite them.
- **Public specifications** this project implements (LSP, MCP, ACP, JSON Schema,
  the Steam library file format, and similar) are cached verbatim under `specs/` with
  `specs/manifest.tsv` holding the origin URL and sha256, as in vouch.
- Third-party source code (id's engine and tools, FTEQCC, ericw-tools) is fetched by commit and
  checksum when a build needs it, never vendored.

## Agent instructions (CONV-13)

As in vouch: `CLAUDE.md` holds the project overview, architecture, conventions and
commands; `AGENTS.md` points to it and covers environment setup; `.claude/rules/` holds
`branching.md`, `commits-and-issues.md`, `continuous-improvement.md`,
`development-discipline.md`, `specs-are-source-of-truth.md`, plus this project's `porting.md`,
`layering.md` and `testing.md` (the readiness rule in `docs/testing.md`).
`.claude/hooks/session-setup.sh` installs the toolchain in remote sessions.

## Repository layout (CONV-14)

```
crates/      qs-* library crates, one per layer component; qs-tests for cross-crate tests
apps/        desktop app, CLI, LSP server binaries (names from ADR-0006)
xtask/       build automation called from make
fuzz/        libfuzzer targets, outside the workspace
docs/        mdBook: guide, ADRs, specs, gates
specs/       cached public specifications (verbatim)
plan/        requirements, backlog, coverage check
packaging/   per-OS installers
```

Crate names use a short project prefix (`qs-`), as vouch uses `vouch-`, settled with the
product name in ADR-0006.

## Source file headers

Every source file starts with an SPDX line, as in vouch, but with this project's license
(**differs from vouch**, which is Apache-2.0 OR MIT; ADR-0002):
`// SPDX-License-Identifier: GPL-3.0-or-later`. A file ported from id's code keeps id's
copyright line, `// Copyright (C) 1996-1997 Id Software, Inc.`, above ours, and cites the file
and function it came from.
`make audit` fails a file without one.

## Repository files (CONV-15)

`LICENSE` (GPL-3.0, ADR-0002), `README.md`, `CONTRIBUTING.md`,
`SECURITY.md`, `CODE_OF_CONDUCT.md`, `.gitattributes`, `.gitignore`, `.env.example` if needed.

## Diagnostics and paths (CONV-16)

- `tracing` for all diagnostics. `print_stdout` and `print_stderr` are denied except at a
  binary's crate root. Log output is English.
- All file locations come from one `paths` module (vouch: `vouch-common/src/paths.rs`). No
  ad-hoc home-directory joins.
- **Open question (P0-01):** vouch uses XDG directories on every platform, including macOS.
  A desktop app usually uses `~/Library/Application Support` on macOS and `%APPDATA%` on
  Windows. P0-01 records the choice in an ADR.
- User-facing strings go through one catalog so translation is possible later (vouch uses
  Fluent via `vouch-i18n`). Translations themselves are not in scope for v1.
