# CI and the protection of main

CI runs on every pull request and every merge-queue group (QA-1, PM-4, CONV-7). Nothing
reaches `main` except through the merge queue, and the queue merges a group only when every
required check below has passed on it, so a push to `main` is not checked again.

## When each run happens

| Run | Ref | What runs | Why |
| --- | --- | --- | --- |
| Pull request | `refs/pull/<n>/merge` | every job; `Test Assets (linux)` without the deletion check | finds a problem on every platform before the change is queued |
| Merge group | `refs/heads/gh-readonly-queue/main/...` | every job and every step except `Dependency Review` | checks exactly what lands on `main` |
| Cache run on `main` (`ci.yml` only) | `refs/heads/main` | every job except `Format`, `cargo-deny` and `Dependency Review`, as on a pull request | fills the caches; see below |

That is the full path. A pull request or merge group that changes only documentation, the
plan or agent files takes the light path instead ([below](#light-changes)).

The single-token deletion checks of id 1.01 (`cargo xtask parse-corpus --deletions`, P1-21)
and of the rerelease under the FTEQCC profile (`--fteqcc --deletions`, P2-51) reparse each
file once per token and are the steps that take minutes, so they run only in
the merge group, which every change passes before it reaches `main`. In that run the
`Test Assets (linux)` job builds xtask at `opt-level` 3 (`CARGO_PROFILE_DEV_OPT_LEVEL`), once
for all its steps, with debug assertions and overflow checks still on. Every other step and
job runs on the pull request too, so a platform problem still shows before queueing. A
step skipped this way still lets its job report success, so the required check names never
change.

### Light changes

A pull request or merge group that changes only documentation, the plan or agent files
takes the light path: every required check still reports success under its exact name, but
only the checks that read those files do any work. The first job of `ci.yml`,
`Classify the change` (not a required check), runs
[`scripts/ci-changes.sh`](../scripts/ci-changes.sh) on the run's diff, and every other job
needs it:

| Event | Diff |
| --- | --- |
| Pull request | the merge commit against its first parent, the base branch |
| Merge group | `merge_group.base_sha` against the group's head |
| Push to `main` | `before` against the pushed commit; every path in the push filter is full, so this run is always full |
| Schedule, `workflow_dispatch` | none: full |

A change is light when every changed file is on this allowlist, and full otherwise. An empty
diff, a diff that cannot be computed (an unknown or unfetchable base, an all-zero `before`)
and a failed `Classify the change` job are all full. Renames count both paths.

| Light | Why only `cargo xtask ci` and xtask's tests read it |
| --- | --- |
| `docs/**`, except `docs/spec/**` | read by `cargo xtask` `adr`, `conventions`, `licenses` (and `docs/porting.md` through it), `attribution` and `contrast` (`docs/design/tokens.toml`), and by xtask's tests: the same checks on the repository, the porting-header rules and the benchmark budgets (`docs/perf-budgets.md`) |
| `plan/**` | read by the `plan` and `plan-tests` steps of `cargo xtask ci`, and by xtask's ratchet tests (`plan/backlog/readiness.yaml`) |
| `.claude/**`, `.kiro/**` | read only by `cargo xtask licenses` and its test, and by xtask's tests of `.claude/skills/work-issue/scripts/check-pr-body.sh` |
| `*.md` at the root (`README.md`, `CLAUDE.md`, `AGENTS.md`, `ATTRIBUTION.md`, ...) | read only by `cargo xtask licenses` and `attribution` and their tests |

Never light, whatever directory they are in:

- `docs/spec/**`: the spec-citation ratchet in `qs-tests` (`spec_coverage.rs`), the test
  vectors several crates read, and `corpus/language_coverage.py` in `Test Assets (linux)`.
- Any `.rs` file: `qs-tests` reads every `.rs` file in the tree for spec citations.
- Everything else, including `.github/**`, `specs/**`, `scripts/**`, crate READMEs and any
  path the script does not know.

No crate but xtask reads a light path in its tests or build: the only repository files the
other crates' tests open or embed with `include_str!` and `include_bytes!` are under
`docs/spec/`, and no crate has a build script. On the light path:

| Check | What it does |
| --- | --- |
| `cargo xtask ci` | everything, as on a full run: the plan checks, cached specs, conventions, ADRs, layers, licenses, attribution, contrast and both fixture checks |
| `Unit Tests (linux)`, `(macos)`, `(windows)` | `cargo test --locked -p xtask --all-features` on each OS instead of the whole workspace, and no oracles |
| `Format`, `Clippy`, `cargo-deny`, `Build (macos-universal)` | the job is skipped by its `if:`, which reports success |
| `Build (linux-x86_64)` and the other build entries, `Test Assets (linux)`, `(macos)`, `(windows)` | every step is skipped, so the job reports success under its matrix name |
| `Dependency Review`, `Pinned Actions`, `zizmor` | unchanged; `.github/**` is never light |

GitHub's [troubleshooting page for required status checks](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/collaborating-on-repositories-with-code-quality-features/troubleshooting-required-status-checks#handling-skipped-but-required-checks)
decides the shape: "If a workflow is skipped due to path filtering, branch filtering or a
commit message, then checks associated with that workflow will remain in a "Pending" state.
A pull request that requires those checks to be successful will be blocked from merging."
and "If, however, a job within a workflow is skipped due to a conditional, it will report
its status as "Success"." So `ci.yml` has no `paths:` filter on pull requests or merge
groups. The same page warns: "When a job fails, any jobs that depend on the failed job are
skipped and do not report a failure." Every job that needs `Classify the change` therefore
has `!cancelled()` in its `if:` and treats an empty output as full, so a failed
classification runs everything rather than skipping it. The page does not say what name a
matrix job skipped by its `if:` reports, so the matrix jobs skip their steps instead and
each entry reports its own name.

The script's tests are `xtask/src/ci/changes_tests.rs` (each category, the unknown-path
fallback, renames, an empty diff and an unknown base); they run with xtask's tests, so on
the light path too.

### Caches

A cache is scoped to the ref of the run that saves it, and a run can restore only caches of
its own ref, of `main` and, for a pull request, of its base branch ([GitHub's dependency
caching reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache)).
A merge group runs on its own temporary branch, so a cache saved there can be restored by no
pull request and no later group. Every cache in `ci.yml` is therefore saved only by a run on
`main` (`save-if` for `Swatinem/rust-cache`, `actions/cache/save` with an `if` for the
others) and only restored elsewhere. A cache changes only when its key does, so `ci.yml`
runs on a push to `main` only when a file a key depends on changes (`Cargo.lock`, any
`Cargo.toml`, `.cargo/config.toml`, `rust-toolchain.toml`, `oracles/`, the xtask oracle code,
`corpus/manifest.toml` or `ci.yml` itself, whose `env` is part of the Rust cache keys). It
also runs weekly, so a cache evicted after seven days unused or one whose key changed with
the runner image is refilled, and on demand (`workflow_dispatch`). The push paths must list
every input of every cache key.

## Workflows

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml):

| Check | Runs on | What it does |
| --- | --- | --- |
| `Format` | Linux | `cargo fmt --all --check` |
| `Clippy` | Linux | `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` |
| `Unit Tests (linux)`, `Unit Tests (macos)`, `Unit Tests (windows)` | each OS | `cargo test --locked --workspace --all-features`, then `cargo xtask oracles`: fetches id's and FTEQCC's sources by checksum, builds the VM oracle, id's qcc and FTEQCC with the system C compiler, and runs every VM vector through the VM oracle ([`oracles/README.md`](../oracles/README.md)); the fetched sources and binaries are cached, keyed on `oracles/` and the xtask code that builds them |
| `Build (linux-x86_64)`, `Build (linux-aarch64)` | x86_64 and arm64 Linux runners | `cargo build --profile ci --locked --workspace` for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` |
| `Build (windows-x86_64)`, `Build (windows-aarch64)` | Windows | the same for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` (cross-compiled) |
| `Build (macos-universal)` | macOS | builds `aarch64-apple-darwin` and `x86_64-apple-darwin`, then joins every binary with `lipo` and checks both architectures are in it |
| `cargo xtask ci` | Linux | the command `make ci` runs, less the steps another job runs with the same command (`cargo xtask ci --skip`, [below](#which-job-runs-each-step-of-cargo-xtask-ci)): the plan checks (`plan/check.py` and its tests, with PyYAML installed), the cached specifications (`scripts/refresh-specs.sh --verify`, offline: fails naming any file under `specs/` whose SHA-256 differs from `specs/manifest.tsv`, that is listed but missing, or that has no row), conventions, ADRs, layers, licenses, attribution, contrast, clippy fixtures and deny fixtures; on a cache run on `main`, fmt and cargo-deny too |
| `cargo-deny` | Linux | `cargo deny --locked --all-features check advisories bans licenses sources` with cargo-deny 0.20.2, the version the `cargo xtask ci` job installs |
| `Dependency Review` | Linux | `dependency-review-action` on pull requests; skipped on merge groups and cache runs, which a required check accepts |
| `Test Assets (linux)`, `Test Assets (macos)`, `Test Assets (windows)` | each OS | the `language_coverage.py` tests, `cargo xtask corpus verify`, then `cargo xtask assets fetch` and `precache` ([test-assets.md](test-assets.md)); on Linux only, `cargo xtask engines --check`, which fetches the engine-target database's fact sources by checksum and fails when `crates/qc-engines/src/tables.rs` differs from what they give ([engine-targets.md](spec/engine-targets.md), ENGT-1.2), `cargo xtask corpus fetch id-1.01`, which writes id 1.01 to the cache by checksum, `corpus/language_coverage.py`, which checks that [`docs/spec/language.md`](spec/language.md) covers every construct id's QuakeC 1.01 uses, `cargo xtask lex-corpus`, which lexes every `.qc` file and preprocesses the files `progs.src` lists with no errors, `cargo xtask oracles id101` on the same files, which compiles them with id's qcc, checks the progs.dat's SHA-256 and traces two scenarios with the VM oracle, `cargo xtask parse-corpus`, which parses id 1.01 with no errors and checks that every file of the rerelease, Copper and Arcane Dimensions round-trips through the syntax tree, then, in the merge group only, `cargo xtask parse-corpus --deletions`, which deletes each token of id 1.01 in turn to check that recovery stays local, and the same with `--fteqcc` on the rerelease under the FTEQCC profile, `cargo xtask oracles rerelease` on the six rerelease codebases, which compiles each with FTEQCC to a version 6 progs.dat with vanilla opcodes only and a `.lno`, checks both against recorded SHA-256 digests and keeps them in `target/oracles/rerelease` (cached on `oracles/`, the xtask oracle code and `corpus/manifest.toml`), `cargo xtask corpus build`, which checks that every progs.dat those oracles build round-trips through `qc-progs` byte for byte, `cargo xtask differential --fast`, which runs id 1.01 end to end in the VM oracle and QuakeC Studio's runner and fails on any divergence DIFF-10.1 does not list (the differential VM suite QA-1 names; the full corpus runs nightly, [below](#differential)), `cargo xtask corpus fetch` of every map in the manifest and `cargo xtask map-corpus`, which checks that each reads and writes back through `qs-map` byte for byte, and the `qs-vfs` `walk` example on the LibreQuake lite asset, which loads every BSP, `.lit`, alias model, sprite and sound in it; the download cache, which holds the asset archives and the repository archives `corpus fetch` reads, is keyed on the SHA-256 of `corpus/manifest.toml` and saved after the last fetch |

### Which job runs each step of cargo xtask ci

`make ci` runs every step of `cargo xtask ci` (`xtask/src/ci.rs`). In CI, five of them have a
job of their own that runs the same command, so the `cargo xtask ci` job leaves them out with
`--skip` and each runs once per CI run. A run on `main` skips `Format` and `cargo-deny`, so
there the `cargo xtask ci` job runs fmt and deny itself.

| Step (`--skip` name) | Command | Pull request and merge group | Cache run on `main` |
| --- | --- | --- | --- |
| `fmt` | `cargo fmt --all --check` | `Format` | `cargo xtask ci` |
| `plan` | `python3 plan/check.py` | `cargo xtask ci` | `cargo xtask ci` |
| `plan-tests` | `python3 -m unittest discover -s plan -p test_*.py` | `cargo xtask ci` | `cargo xtask ci` |
| `specs` | `bash scripts/refresh-specs.sh --verify` | `cargo xtask ci` | `cargo xtask ci` |
| `conventions`, `adr`, `layers`, `licenses`, `attribution`, `contrast` | `cargo xtask <name>` | `cargo xtask ci` | `cargo xtask ci` |
| `clippy` | `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | `Clippy` | `Clippy` |
| `test` | `cargo test --locked --workspace --all-features` | `Unit Tests (linux)` (and macOS and Windows) | `Unit Tests (linux)` |
| `clippy-fixtures` | `cargo xtask clippy-fixtures` | `cargo xtask ci` | `cargo xtask ci` |
| `deny` | `cargo deny --locked --all-features check advisories bans licenses sources` | `cargo-deny` | `cargo xtask ci` |
| `deny-fixtures` | `cargo xtask deny-fixtures` | `cargo xtask ci` | `cargo xtask ci` |
| `fuzz-build` | `cargo check --manifest-path fuzz/Cargo.toml --locked` | `cargo xtask ci` | `cargo xtask ci` |
| `differential` | `cargo xtask differential --fast` | `Test Assets (linux)` | `Test Assets (linux)` |

The skipped steps are the `SKIP` value of the `cargo xtask ci` job in `ci.yml`. An xtask test
(`every_step_ci_skips_runs_in_another_job_with_the_same_command`) fails unless each one is a
`run:` line of `ci.yml` with the step's exact command, and `cargo xtask ci --skip` fails on a
name no step has. Every job here is a required check, so a skipped step still has to pass
before a change merges.

The `fuzz-build` step builds the cargo-fuzz targets in [`fuzz/`](../fuzz) with `cargo check`
on the pinned stable toolchain, without running them. The fuzz crate is outside the workspace
with its own lock file, so the `clippy` and `test` steps never build it, and a struct or API
change can break a target unnoticed until the nightly `Fuzz` run (it did on 2026-10-22, when a
new `ResolveOptions` field broke `fuzz/fuzz_targets/qc_sema.rs`). `cargo check` catches that
class of error, a type error, without the sanitizer instrumentation `cargo xtask fuzz` needs
nightly for.

[`.github/workflows/secure_workflows.yml`](../.github/workflows/secure_workflows.yml):

| Check | What it does |
| --- | --- |
| `Pinned Actions` | fails if any workflow uses an action not pinned to a full commit SHA (`zgosalvez/github-actions-ensure-sha-pinned-actions`) |
| `zizmor` | audits every workflow with zizmor and fails on any finding |

[`.github/workflows/bench.yml`](../.github/workflows/bench.yml) runs weekly and on demand
(`workflow_dispatch`), never on pull requests or in the merge queue, and is **not** a required
check:

| Check | Runs on | What it does |
| --- | --- | --- |
| `Benchmarks` | Linux | `cargo xtask bench` against the history saved from `main`, then uploads the history as the `bench-history` artifact ([perf-budgets.md](perf-budgets.md#in-ci)) |

[`.github/workflows/fuzz.yml`](../.github/workflows/fuzz.yml) runs nightly and on demand
(`workflow_dispatch`), never on pull requests or in the merge queue, and is **not** a required
check:

| Check | Runs on | What it does |
| --- | --- | --- |
| `Fuzz` | Linux | `cargo xtask fuzz --time 10m`: every cargo-fuzz target in `fuzz/` for 10 minutes on nightly Rust, the `parser` target seeded with the `.qc` files of id 1.01 and the rerelease (`cargo xtask corpus fetch`), and the `qc-vm` target run against the id VM oracle (`cargo xtask oracles` builds it and `QS_VM_ORACLE` points at it, so well-formed random progs are compared in both VMs, FND-10); on a crash, uploads `fuzz/artifacts/` as the `fuzz-artifacts` artifact |

### Differential

[`.github/workflows/differential.yml`](../.github/workflows/differential.yml) runs nightly and
on demand (`workflow_dispatch`), never on pull requests or in the merge queue, and is **not** a
required check (docs/ci.md): the full corpus takes minutes, and the fast subset (id 1.01, zero
divergences) runs on every change in the `Test Assets (linux)` job above.

| Check | Runs on | What it does |
| --- | --- | --- |
| `Differential` | Linux | `cargo xtask differential --full`: builds the oracles, compiles the rerelease with FTEQCC, builds the corpus progs, then runs every oracle-built codebase (id 1.01 and the six rerelease codebases) in the VM oracle and QuakeC Studio's runner, comparing traces (DIFF-9) and measuring statement coverage (DIFF-11); enforces zero divergences, and reports statement coverage without gating on it ([ADR-0013](adr/0013-differential-coverage-bar.md), TEST-6) |

[`.github/workflows/readiness.yml`](../.github/workflows/readiness.yml) runs only on demand
(`workflow_dispatch`, with the crates to measure as its `crates` input), never on pull requests,
in the merge queue or on a schedule, and is **not** a required check (ADR-0009). A readiness
reviewer runs it and links the run in the report ([testing.md](testing.md#measuring-a-layer)):

| Check | Runs on | What it does |
| --- | --- | --- |
| `Readiness coverage` | Linux | `cargo xtask readiness lines` (cargo-llvm-cov, stable) and `branches` (cargo-llvm-cov `--branch`, date-pinned nightly) |
| `Readiness mutation (<crate> <k/n>)` | Linux | `cargo xtask readiness mutation --shard k/n` (one shard of a cargo-mutants run of one crate, one job per shard, 10 shards for qc-preproc, 4 for qc-syntax and 1 for any other crate; xtask limits each test binary to 3 GiB of data memory on Linux) |
| `Readiness flakes (linux)`, `(macos)`, `(windows)` | each OS | `cargo xtask readiness flakes`: each crate's tests 20 times |
| `Readiness ratchet` | Linux | `cargo xtask readiness spec`, then `cargo xtask ratchet` over every measurement (a crate's mutation shards added into one score), `ratchets.toml` and its history; uploads the `readiness` artifact |

Every action is pinned to a commit SHA with its version in a comment, every job has
least-privilege `permissions`, and every checkout sets `persist-credentials: false`.

**Differs from vouch:** vouch skips its build matrix in merge groups and reports a stand-in
check instead. Here every job runs in the merge group, so what merges has been built for
every target at the commit that lands.

## Protection of main

The `default-branch` ruleset ([settings](https://github.com/quakecstudio/editor/rules/25187180))
applies to `main`:

- **Pull request required**, with squash as the only merge method. No approving review is
  required: agents land their own PRs once CI passes, except `needs:human` issues.
- **Merge queue**, squash, building up to 5 groups.
- **Required status checks**: every check in the `ci.yml` and `secure_workflows.yml` tables above (not `Benchmarks`), by the exact name it
  reports. A required check whose name never reports holds the queue until it times out,
  so a job is renamed, added or removed in the same change as the ruleset.
- **No force pushes** and **no deletion**. Nobody can bypass the ruleset.

The repository allows squash merges only and deletes branches after merging.

To see the live ruleset:

```bash
gh api repos/quakecstudio/editor/rulesets/25187180
```

## Pre-commit hooks

[`.pre-commit-config.yaml`](../.pre-commit-config.yaml) runs with
[prek](https://github.com/j178/prek). `prek install` installs it for pre-commit (file
hygiene, private keys, `cargo fmt`, shellcheck, shfmt, actionlint, zizmor) and pre-push
(clippy and the tests). The hooks are a fast local copy of the CI checks, not a
replacement for them.
