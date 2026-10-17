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

The single-token deletion check of id 1.01 (`cargo xtask parse-corpus --deletions`, P1-21)
reparses each file once per token and is the one step that took minutes, so it runs only in
the merge group, which every change passes before it reaches `main`. In that run the
`Test Assets (linux)` job builds xtask at `opt-level` 3 (`CARGO_PROFILE_DEV_OPT_LEVEL`), once
for all its steps, with debug assertions and overflow checks still on. Every other step and
job runs on the pull request too, so a platform problem still shows before queueing. A
step skipped this way still lets its job report success, so the required check names never
change.

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
| `Test Assets (linux)`, `Test Assets (macos)`, `Test Assets (windows)` | each OS | the `language_coverage.py` tests, `cargo xtask corpus verify`, then `cargo xtask assets fetch` and `precache` ([test-assets.md](test-assets.md)); on Linux only, `cargo xtask corpus fetch id-1.01`, which writes id 1.01 to the cache by checksum, `corpus/language_coverage.py`, which checks that [`docs/spec/language.md`](spec/language.md) covers every construct id's QuakeC 1.01 uses, `cargo xtask lex-corpus`, which lexes every `.qc` file and preprocesses the files `progs.src` lists with no errors, `cargo xtask oracles id101` on the same files, which compiles them with id's qcc, checks the progs.dat's SHA-256 and traces two scenarios with the VM oracle, `cargo xtask parse-corpus`, which parses id 1.01 with no errors and checks that every file of the rerelease, Copper and Arcane Dimensions round-trips through the syntax tree, then, in the merge group only, `cargo xtask parse-corpus --deletions`, which deletes each token of id 1.01 in turn to check that recovery stays local, `cargo xtask oracles rerelease` on the six rerelease codebases, which compiles each with FTEQCC to a version 6 progs.dat with vanilla opcodes only and a `.lno`, checks both against recorded SHA-256 digests and keeps them in `target/oracles/rerelease` (cached on `oracles/`, the xtask oracle code and `corpus/manifest.toml`), `cargo xtask corpus build`, which checks that every progs.dat those oracles build round-trips through `qc-progs` byte for byte, `cargo xtask differential --fast`, which runs id 1.01 end to end in the VM oracle and QuakeC Studio's runner and fails on any divergence DIFF-10.1 does not list (the differential VM suite QA-1 names; the full corpus runs nightly, [below](#differential)), `cargo xtask corpus fetch` of every map in the manifest and `cargo xtask map-corpus`, which checks that each reads and writes back through `qs-map` byte for byte, and the `qs-vfs` `walk` example on the LibreQuake lite asset, which loads every BSP, `.lit`, alias model, sprite and sound in it; the download cache, which holds the asset archives and the repository archives `corpus fetch` reads, is keyed on the SHA-256 of `corpus/manifest.toml` and saved after the last fetch |

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
| `differential` | `cargo xtask differential --fast` | `Test Assets (linux)` | `Test Assets (linux)` |

The skipped steps are the `SKIP` value of the `cargo xtask ci` job in `ci.yml`. An xtask test
(`every_step_ci_skips_runs_in_another_job_with_the_same_command`) fails unless each one is a
`run:` line of `ci.yml` with the step's exact command, and `cargo xtask ci --skip` fails on a
name no step has. Every job here is a required check, so a skipped step still has to pass
before a change merges.

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
| `Differential` | Linux | `cargo xtask differential --full`: builds the oracles, compiles the rerelease with FTEQCC, builds the corpus progs, then runs every oracle-built codebase (id 1.01 and the six rerelease codebases) in the VM oracle and QuakeC Studio's runner, comparing traces (DIFF-9) and measuring statement coverage (DIFF-11); enforces zero divergences and the 90% statement-coverage bar (TEST-6) |

[`.github/workflows/readiness.yml`](../.github/workflows/readiness.yml) runs only on demand
(`workflow_dispatch`, with the crates to measure as its `crates` input), never on pull requests,
in the merge queue or on a schedule, and is **not** a required check (ADR-0009). A readiness
reviewer runs it and links the run in the report ([testing.md](testing.md#measuring-a-layer)):

| Check | Runs on | What it does |
| --- | --- | --- |
| `Readiness coverage` | Linux | `cargo xtask readiness lines` (cargo-llvm-cov, stable) and `branches` (cargo-llvm-cov `--branch`, date-pinned nightly) |
| `Readiness mutation` | Linux | `cargo xtask readiness mutation` (a full cargo-mutants run per crate) |
| `Readiness flakes (linux)`, `(macos)`, `(windows)` | each OS | `cargo xtask readiness flakes`: each crate's tests 20 times |
| `Readiness ratchet` | Linux | `cargo xtask readiness spec`, then `cargo xtask ratchet` over every measurement, `ratchets.toml` and its history; uploads the `readiness` artifact |

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
