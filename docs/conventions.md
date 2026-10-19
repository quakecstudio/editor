# Rust conventions

QuakeC Studio follows the conventions of [vouch-sh/vouch](https://github.com/vouch-sh/vouch).
Where this project departs from vouch, the departure and its reason are stated inline
as **Differs from vouch**. Requirement IDs (`CONV-n`) are tracked in
`plan/requirements.yaml`; issue P0-01 puts the configuration in place.

Reference: vouch `main` as of 2026-10-15 (`Cargo.toml`, `.rustfmt.toml`, `.clippy.toml`,
`deny.toml`, `rust-toolchain.toml`, `.pre-commit-config.yaml`, `.github/workflows/`,
`CLAUDE.md`, `AGENTS.md`, `.claude/rules/`).

**Status:** adopted by P0-01 ([#1](https://github.com/quakecstudio/editor/issues/1)) on
2026-10-15, with every "differs from vouch" item below confirmed.

## Checking conformance

This document is the source of the workspace configuration, and three commands check that
the files agree with it:

- `cargo xtask conventions` compares every fenced block here whose info string names a file
  (` ```toml file=deny.toml `) with that file: each key in the block must have the same value
  in the file, and arrays must match exactly. A block marked `exact` also forbids extra keys
  in every table it spells out with plain keys only. The clippy table below, between its
  `conventions:clippy-deny` markers, must list exactly the lints that are `deny`. The command
  also checks the rules that are not single values: CalVer, the exact toolchain, pinned
  workspace dependencies, inherited package fields and lints, and reasons on cargo-deny skips.
- `cargo xtask clippy-fixtures` runs clippy with the workspace lints over the fixtures in
  `xtask/fixtures/clippy/`. Each fixture names the one lint it must trigger, or `pass`.
- `cargo xtask deny-fixtures` runs `cargo deny` with `deny.toml` over scratch crates that use a
  GPL-incompatible license or a wildcard version, plus a control that must pass. It needs
  network access to the crates.io index.

Change this document and the configuration in the same PR.

## Toolchain and workspace (CONV-1)

- `rust-toolchain.toml` pins an exact stable channel (vouch: `1.99.0`) with `rustfmt` and `clippy` components.
- Root `Cargo.toml`: `resolver = "3"`, `members = ["crates/*", "apps/*", "xtask"]`.
- `[workspace.package]` holds `version`, `edition = "2024"`, `license`, `rust-version`,
  `repository`, `publish = false`; every crate inherits them with `*.workspace = true`.
- Version is CalVer `YYYY.MM.N`, as in vouch.
- Every crate has `[lints] workspace = true`.
- `fuzz/` is excluded from the workspace.

```toml file=Cargo.toml
[workspace]
resolver = "3"
members = ["crates/*", "apps/*", "xtask"]
exclude = ["fuzz"]

[workspace.package]
edition = "2024"
license = "GPL-3.0-or-later"
repository = "https://github.com/quakecstudio/editor"
publish = false
```

The channel is `1.99.0`, the same as vouch, and `rust-version` equals it.

Nightly Rust is allowed in two places only: fuzzing with cargo-fuzz (`cargo xtask fuzz`, which
`make test-fuzz` and the scheduled `Fuzz` workflow call), and branch coverage
in the `Readiness` workflow, which pins a dated nightly (`NIGHTLY` in
`.github/workflows/readiness.yml`, equal to `readiness::NIGHTLY` in xtask; ADR-0009).

```toml file=rust-toolchain.toml
[toolchain]
components = ["rustfmt", "clippy"]
```

## Formatting (CONV-2)

`.rustfmt.toml`, identical to vouch:

```toml file=.rustfmt.toml exact
edition = "2024"
max_width = 100
hard_tabs = false
tab_spaces = 4
use_field_init_shorthand = true
newline_style = "Unix"
```

`cargo fmt --all --check` runs in CI and in the pre-commit hook. Formatting is never a follow-up commit.

## Lints (CONV-3)

`[workspace.lints.rust]`, as in vouch (vouch's `linker_messages = "allow"` works around a
vouch-server linker note and is not copied):

```toml file=Cargo.toml exact
[workspace.lints.rust]
unsafe_code = "deny"
unreachable_pub = "warn"
non_ascii_idents = "deny"
trivial_casts = "warn"
trivial_numeric_casts = "warn"
unused_lifetimes = "warn"
single_use_lifetimes = "warn"
```

`[workspace.lints.clippy]`, the vouch no-panic set, all `deny` (vouch's
`disallowed_methods = "allow"` serves its request-time rules and is not copied):

<!-- conventions:clippy-deny -->
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
<!-- /conventions:clippy-deny -->

`too_many_lines = "warn"` with a threshold of 150 in `.clippy.toml`, enforced by `-D warnings`;
a longer function carries `#[expect(clippy::too_many_lines, reason = "...")]`.
`module_name_repetitions` and `similar_names` are allowed.

```toml file=Cargo.toml
[workspace.lints.clippy]
too_many_lines = "warn"
module_name_repetitions = "allow"
similar_names = "allow"
```

`.clippy.toml` copies vouch's settings below, plus an `absolute-paths-allowed-crates` list
naming `std`, `core`, `alloc`, `proc_macro` and every external crate in
`[workspace.dependencies]`:

```toml file=.clippy.toml
absolute-paths-max-segments = 2
cognitive-complexity-threshold = 30
too-many-lines-threshold = 150
allowed-idents-below-min-chars = ["x", "y", "z", "i", "j", "k", "n", "s", "e"]
avoid-breaking-exported-api = false
```

Exceptions are made with `#[expect(lint, reason = "...")]`, never `#[allow]`, and never
crate-wide in a shipped crate.

**This matters more here than in vouch.** The VM and compiler must reproduce C's integer
and float behaviour exactly. `arithmetic_side_effects` and the cast lints force every
wrap, saturate or checked conversion to be spelled out, which is how the spec's
semantics become visible in the code.

**Same as vouch, settled in P0-01: no crate needs `unsafe`.** Windowing and rendering do not
require it: wgpu's `Instance::create_surface` is a safe function that takes any window
implementing the `raw-window-handle` traits (winit's windows do), so only the deprecated
`create_surface_unsafe` path needs `unsafe`. The editor's UI framework (ADR-0003) does
its own platform work inside its own crates. `unsafe_code` therefore stays `deny` in every crate. If a
later need is proven, the `unsafe` goes in one platform crate with a module-level
`#[expect(unsafe_code, reason = "...")]` and a `SAFETY:` comment on every block, and this
section changes in the same PR.

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
(**differs from vouch**: no GPL-2.0-only, no proprietary, and vouch's `OpenSSL` dropped because the
OpenSSL license is not GPL-compatible; `GPL-3.0-or-later` added for our own crates; ADR-0002) and
`confidence-threshold = 0.8`; `[bans]` with `wildcards = "deny"` and
`multiple-versions = "warn"`, each `skip` carrying a `reason`; `[sources]` with
`unknown-registry = "deny"`, `unknown-git = "warn"`, crates.io as the only registry and every git
source pinned to a rev. Runs as `make audit`, in `cargo xtask ci` and in CI's `cargo-deny` job
as `cargo deny --locked --all-features check advisories bans licenses sources`.

As in vouch, `allow-wildcard-paths = true` lets our own unpublished crates depend on each other
by path without a version. **Differs from vouch:** `unused-allowed-license = "allow"`, because
the allow list states which licenses are compatible, not which ones the graph uses today.

```toml file=deny.toml
[graph]
all-features = true
targets = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
]

[licenses]
confidence-threshold = 0.8
allow = [
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "CDLA-Permissive-2.0",
    "GPL-3.0-or-later",
    "ISC",
    "MIT",
    "MIT-0",
    "MPL-2.0",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
]
unused-allowed-license = "allow"

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true

[sources]
unknown-registry = "deny"
unknown-git = "warn"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

## Build profiles (CONV-6)

```toml file=Cargo.toml exact
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

The workflows, the required checks on `main` and the ruleset are in [`ci.md`](ci.md).

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
`branching.md`, `commits-and-issues.md`, `development-discipline.md`,
`specs-are-source-of-truth.md`, plus this project's `porting.md`, `layering.md` and
`testing.md` (the readiness rule in `docs/testing.md`).
`.claude/hooks/session-setup.sh`, registered in `.claude/settings.json`, installs the
toolchain in remote Linux sessions.

**Differs from vouch:** no `continuous-improvement.md`. vouch's file configures its
dev-agents plugin cycle; here the `plan/` skills (`/work-issue`, `/run-phase`,
`/plan-change`) and `docs/agents/README.md` fill that role. Issue forms and the PR template
live in `.github/`, and `plan/check.py` checks that they ask for a deliverable, acceptance
criteria, verification and requirement IDs and carry the definition of done.

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
and function it came from ([`porting.md`](porting.md#file-headers)).
`cargo xtask licenses` (run by `make audit` and `make ci`) fails a file without one. It
classifies every file: source files (Rust, Python, shell, C, QuakeC and the `Makefile`) need
the SPDX line first, after a `#!` line if there is one; docs and config files are covered by `LICENSE`;
assets (images, fonts, sounds, Quake formats) are listed in `ATTRIBUTION.md` and checked by
`cargo xtask attribution`; cached specifications listed in `specs/manifest.tsv` keep their own
licenses, recorded in `specs/README.md`, and are not scanned. A file of a type with no rule fails until it is classified in
`xtask/src/licenses.rs`.

## Repository files (CONV-15)

`LICENSE` (GPL-3.0, ADR-0002), `README.md`, `CONTRIBUTING.md`,
`SECURITY.md`, `CODE_OF_CONDUCT.md`, `.gitattributes`, `.gitignore`, `.env.example` if needed.

## Diagnostics and paths (CONV-16)

- `tracing` for all diagnostics. `print_stdout` and `print_stderr` are denied except at a
  binary's crate root. Log output is English.
- All file locations come from one `paths` module (vouch: `vouch-common/src/paths.rs`). No
  ad-hoc home-directory joins.
  Here that module is the `qs-paths` crate.
- **Differs from vouch:** vouch uses XDG directories on every platform, including macOS.
  QuakeC Studio is a desktop app and uses each OS's native locations instead:
  `~/Library/...` on macOS, `%APPDATA%` and `%LOCALAPPDATA%` on Windows, XDG on Linux
  ([ADR-0011](adr/0011-app-directories.md)).
- User-facing strings go through one catalog so translation is possible later (vouch uses
  Fluent via `vouch-i18n`). Translations themselves are not in scope for v1.

## Type design (CONV-17)

These rules are this project's own (the project owner's request, 2026-10-19), not taken from
vouch. They apply to shipped crates; test code may bend them where a test reads better for
it. No lint checks them: the implementer's pre-land code review
([`work-issue`](../.claude/skills/work-issue/SKILL.md), "Land it") checks every PR against
them, and P2-60 audits the L0 to L3 crates. Code written before them is changed by that
audit's refactoring issues, not by unrelated PRs. Each example is real code in the
repository as of 2026-10-19; the "after" is the shape the rule asks for.

### Enums for closed sets of choices

A closed set of choices is an enum, not a `bool` or a string. A `bool` argument says nothing
at the call site (`lower_body(body, &ty, true)`), two booleans can hold a combination that
means nothing, and a string can hold a typo. An enum names each choice, and `match` makes the
compiler find every place a new variant has to be handled.

`crates/qc-sema/src/lower.rs` turns the `Dialect` it is given into a `bool` and threads that
through eleven functions:

```rust
pub fn lower_file(parse: &Parse, dialect: Dialect) -> FileDecls {
    let fte = dialect.has_fteqcc_extensions();
    // ...
                    lower_declaration(&decl, fte, &mut out.decls);
}

fn lower_body(fn_body: &ast::FunctionBody, fn_ty: &Type, fte: bool) -> RawBody {
```

After: pass the `Dialect` (it is `Copy`) and ask it where the answer matters, so a dialect
with rules of its own changes `Dialect`, not every signature.

```rust
fn lower_body(fn_body: &ast::FunctionBody, fn_ty: &Type, dialect: Dialect) -> RawBody {
    // ...
    let bad = !dialect.has_fteqcc_extensions() && (base.is_field() || base.is_function());
```

The same file reads a declaration's qualifiers as words and compares strings, stripping the
leading underscores at every comparison:

```rust
fn qualifiers(decl: &ast::Declaration) -> Vec<String> { /* the words, as written */ }

let words = qualifiers(decl);
let has = |word: &str| words.iter().any(|w| w.trim_start_matches('_') == word);
let redefinable = has("wrap") || has("weak") || has("accumulate");
```

After: one `Qualifier` enum, with the spelling rules (FTEF-15.1) in the one function that
reads a word, so a misspelt qualifier is a compile error rather than a silent `false`:

```rust
/// A declaration qualifier `lower.rs` reads (FTEF-15.1, FTEF-9.2).
enum Qualifier {
    Wrap, Weak, Accumulate, Extern, Var, Noref, Unused, Virtual, Nonvirtual, Static,
}

impl Qualifier {
    /// The qualifier a word spells, with or without its leading underscores.
    fn from_word(word: &str) -> Option<Self> { /* ... */ }
}

let quals: Vec<Qualifier> = qualifiers(decl);
let redefinable = quals
    .iter()
    .any(|q| matches!(q, Qualifier::Wrap | Qualifier::Weak | Qualifier::Accumulate));
```

### Newtypes for IDs and units

An ID or a unit is a newtype with a private field, built by a constructor and read by an
accessor. A `pub` tuple field lets any caller make an ID from any number and read it back as
a bare number, so the type stops nothing; with the field private, the crate that owns the
type decides how one is made and can change its representation without touching callers.

`crates/qc-sema/src/symbol.rs`:

```rust
pub struct FileId(pub usize);
pub struct DefId(pub usize);
```

After:

```rust
/// A file's index in a module's ordered file graph (`qc_project::Module::graph`).
pub struct FileId(usize);

impl FileId {
    /// The file at `index` in the module's file graph.
    #[must_use]
    pub const fn new(index: usize) -> Self { Self(index) }

    /// The file's index in the module's file graph.
    #[must_use]
    pub const fn index(self) -> usize { self.0 }
}
```

Make the constructor `pub(crate)` when only the owning crate should make one.

### Behaviour that reads one type is a method on it

A function whose work is reading one type's data is a method on that type, not a free
function taking it. A method is found where the type is documented, sits beside the related
methods, and keeps the type's rules in one place instead of in each crate that needs them.

`crates/qc-sema/src/resolve.rs` and `crates/qc-sema/src/nav.rs`:

```rust
fn component_type(ty: &Type) -> Option<Type> { /* float for vector, .float for .vector */ }

fn covers(span: qc_diag::Span, offset: usize) -> bool {
    if span.start == span.end {
        offset == span.start
    } else {
        offset >= span.start && offset < span.end
    }
}
```

After: `Type::component` in `crates/qc-sema/src/ty.rs`, and `Span::covers` in
`crates/qc-diag/src/lib.rs` beside `Span::contains`, where every crate that has a span can
use it:

```rust
impl Type {
    /// The type of each component of a value of this type (QC-11.7).
    pub(crate) fn component(&self) -> Option<Type> { /* ... */ }
}

impl Span {
    /// Whether this span covers byte `offset`: `[start, end)`, or exactly `start` when empty.
    #[must_use]
    pub const fn covers(self, offset: usize) -> bool { /* ... */ }
}
```

### No single-use one-line helpers

A one-line function with one caller is inlined, unless its name says something the
expression does not: a spec rule, a C idiom, or an `#[expect]` that would otherwise sit in
the middle of a larger function. Otherwise a reader jumps to the helper only to find it does
nothing more than its body.

`crates/qc-builtins/src/math.rs` has one caller of:

```rust
/// An `i32` as f64, exact for every `i32`.
fn int_to_f64(v: i32) -> f64 {
    f64::from(v)
}

let result = (360.0 / 65536.0) * int_to_f64(masked);
```

After: `f64::from` already says the conversion is exact, as the first line of the same
function, `anglemod`, shows.

```rust
let scaled = f64::from(a) * (65536.0 / 360.0);
// ...
let result = (360.0 / 65536.0) * f64::from(masked);
```

`int_to_f32` in the same file stays: its `#[expect(clippy::cast_precision_loss, reason =
"spec BUILTIN-6: ...")]` is what its name stands for.

### Generics and traits instead of duplicated code

Code that differs only in a type is written once, generic over that type or behind a trait.
Copies drift: a bound check fixed in one stays wrong in the others.

`le_i32` is copied byte for byte in `crates/qs-pak/src/lib.rs` and `crates/qs-wad/src/lib.rs`,
with variants in `qs-mdl`, `qs-wav`, `qs-bsp` (`i32_at`, `u32_at`) and `qc-lno` (`read_u32`),
each with its own bound check:

```rust
/// The signed little-endian 32-bit integer at `offset`, or 0 past the end of `bytes`.
fn le_i32(bytes: &[u8], offset: usize) -> i32 {
    let word = bytes
        .get(offset..offset.saturating_add(4))
        .and_then(|word| <[u8; 4]>::try_from(word).ok())
        .unwrap_or_default();
    i32::from_le_bytes(word)
}
```

`crates/qc-progs/src/records.rs` already has the shape the rule asks for: the bound check is
written once, generic over the width, and each integer type is a one-line use of it.

```rust
fn read_array<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

pub(crate) fn read_i32(bytes: &[u8], at: usize) -> Option<i32> {
    read_array(bytes, at).map(i32::from_le_bytes)
}
```

Its `Record` trait does the same for tables: `Progs::table<T: Record>` is one function for
every table rather than one per record type. Whether copies in several crates become one
shared module is a layering decision ([`layers.md`](layers.md)); the duplication is a
finding either way.

### No stringly-typed errors in library crates

A library crate returns an error enum, not a `String` or a `Box<dyn Error>` made from text.
A caller cannot match on a sentence, a test that compares one breaks when the wording
changes, and the variants are the list of what can go wrong. A binary may turn errors into
text at the top, where a person reads them.

`crates/qc-syntax/src/recovery.rs`:

```rust
pub(crate) fn check_one(/* ... */) -> Result<usize, String> {
    // ...
    if root.text() != edited {
        return Err("the tree does not give back the text".to_owned());
    }
    // ...
    if prefix < damaged {
        return Err(format!("declaration {prefix} before the deletion changed"));
    }
```

After: an enum with one variant per failure, and `Display` for the text, as `ReadError` in
`crates/qc-progs/src/read.rs` does:

```rust
/// Why one edited source fails the recovery check.
pub enum RecoveryFault {
    /// The tree does not give back the text.
    NotLossless,
    /// The root is not a source file.
    NotSourceFile,
    /// A declaration before the deletion changed.
    PrefixChanged { declaration: usize },
}
```
