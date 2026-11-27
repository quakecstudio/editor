# CI and the protection of main

CI runs on every pull request (QA-1, PM-4, CONV-7). Nothing reaches `main` except through a
pull request, which merges only when every required check below has passed on it, so a push
to `main` is not checked again. The merge queue is off (since 2026-10-30: it re-ran the full
suite on PRs that were already green). A PR need not be up to date with `main` to merge, so
after each merge the agent merges `main` into any other open branch before landing it. The
`merge_group` triggers stay in the workflows so that turning the queue back on needs only the
ruleset; with it on, every job also runs on each merge group.

## When each run happens

| Run | Ref | What runs | Why |
| --- | --- | --- | --- |
| Pull request | `refs/pull/<n>/merge` | every job; `Test Assets (linux)` runs the deletion checks only when the parser chain changed | finds a problem on every platform before the change merges |
| Merge group, only with the queue on | `refs/heads/gh-readonly-queue/main/...` | every job and every step except `Dependency Review` | checks exactly what lands on `main` |
| Cache run on `main` (`ci.yml` only) | `refs/heads/main` | every job except `Format`, `cargo-deny` and `Dependency Review`, but only the steps that fill a cache: `Unit Tests` builds the tests without running them (`nextest --no-run`) and the oracles without their checks (`oracles build`), and `Test Assets` only fetches and builds what its caches hold; the tests passed on the pull request | fills the caches; see below |

That is the full path. A pull request or merge group that changes only documentation, the
plan or agent files takes the light path instead ([below](#light-changes)).

The single-token deletion checks of id 1.01 (`cargo xtask parse-corpus --deletions`, P1-21)
and of the rerelease under the FTEQCC profile (`--fteqcc --deletions`, P2-51) reparse each
file once per token and are the steps that take minutes. They say something new only when
the parser chain they exercise changes, so `Classify the change` sets a `parser` output when
the diff touches `qc-diag`, `qc-lexer`, `qc-preproc`, `qc-syntax`, the xtask code that drives
them (`parse_corpus.rs`, `lex_corpus.rs`), `corpus/manifest.toml` or `Cargo.lock`, or when
there is no diff to compare (the weekly and manual runs). The checks run on such a pull
request, on those runs, and in every merge group when the queue is on; never in a cache run
on `main`. In a run that does them, the `Test Assets (linux)` job builds xtask at `opt-level`
3 (`CARGO_PROFILE_DEV_OPT_LEVEL`), once for all its steps, with debug assertions and overflow
checks still on. A step skipped this way still lets its job report success, so the required
check names never change.

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
| Merge group (queue on) | `merge_group.base_sha` against the group's head |
| Push to `main` | `before` against the pushed commit; every path in the push filter is full, so this run is always full |
| Schedule, `workflow_dispatch` | none: full |

A change is light when every changed file is on this allowlist, and full otherwise. An empty
diff, a diff that cannot be computed (an unknown or unfetchable base, an all-zero `before`)
and a failed `Classify the change` job are all full. Renames count both paths.

| Light | Why only `cargo xtask ci` and xtask's tests read it |
| --- | --- |
| `docs/**`, except `docs/spec/**` and `docs/agent-reference/**` | read by `cargo xtask` `adr`, `conventions`, `licenses` (and `docs/porting.md` through it), `attribution`, `contrast` (`docs/design/tokens.toml`) and `commands` (`docs/commands.md`), and by xtask's tests: the same checks on the repository, the porting-header rules and the benchmark budgets (`docs/perf-budgets.md`) |
| `plan/**` | read by the `plan` and `plan-tests` steps of `cargo xtask ci` |
| `.claude/**`, `.kiro/**` | read only by `cargo xtask licenses` and its test, and by xtask's tests of `.claude/skills/work-issue/scripts/check-pr-body.sh` |
| `*.md` at the root (`README.md`, `CLAUDE.md`, `AGENTS.md`, `ATTRIBUTION.md`, ...) | read only by `cargo xtask licenses` and `attribution` and their tests |

Never light, whatever directory they are in:

- `docs/spec/**`: the spec-citation ratchet in `qs-tests` (`spec_coverage.rs`), the test
  vectors several crates read, and `corpus/language_coverage.py` in `Test Assets (linux)`.
- `docs/agent-reference/**`: `qs-workbench` embeds each topic for `project.reference`, and
  its tests check that the reference names only commands there are (L-45).
- Any `.rs` file: `qs-tests` reads every `.rs` file in the tree for spec citations.
- Everything else, including `.github/**`, `specs/**`, `scripts/**`, crate READMEs and any
  path the script does not know.

No crate but xtask reads a light path in its tests or build: the only repository files the
other crates' tests open or embed with `include_str!` and `include_bytes!` are under
`docs/spec/` and `docs/agent-reference/`, and no crate has a build script. On the light path:

| Check | What it does |
| --- | --- |
| `cargo xtask ci` | everything, as on a full run: the plan checks, cached specs, conventions, ADRs, layers, licenses, attribution, contrast, the command engine's release-build guard check, the command reference check and both fixture checks |
| `Unit Tests (linux)`, `(macos)`, `(windows)` | `cargo nextest run --locked -p xtask --all-features` on each OS instead of the whole workspace, and no oracles |
| `Format`, `Clippy`, `cargo-deny`, `Build (macos-universal)` | the job is skipped by its `if:`, which reports success |
| `Build (linux-x86_64)` and the other build entries, `Test Assets (linux)`, `(macos)`, `(windows)` | every step is skipped, so the job reports success under its matrix name |
| `Dependency Review` | unchanged; `.github/**` is never light |

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
the runner image is refilled, and on demand (`workflow_dispatch`); those two runs test in
full. The push paths must list
every input of every cache key.

### Stopping at the first failure

A run stops as soon as one test fails, at two levels (P2-146):

- Within a job, `Unit Tests` runs the workspace with
  [cargo-nextest](https://nexte.st/) (`cargo nextest run`, installed through
  `taiki-e/install-action` at a pinned version), whose default is to stop scheduling tests
  after the first failure, so the job ends at that test instead of running every binary
  to the end. The light path's xtask-only step uses the same command. nextest runs no doc
  tests, and the workspace has none: every rustdoc code block is `text`.
- Across a matrix, `Unit Tests` and `Build` set
  `fail-fast: true`, so the first OS or target that fails cancels its siblings and frees
  their runners. A cancelled job shows as cancelled, not failed: the failure to read is the
  one that was not cancelled.

## Workflows

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml):

| Check | Runs on | What it does |
| --- | --- | --- |
| `Format` | Linux | `cargo fmt --all --check` |
| `Clippy` | Linux | `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, after installing ALSA's headers (`libasound2-dev`) |
| `Unit Tests (linux)`, `Unit Tests (macos)`, `Unit Tests (windows)` | each OS | `cargo nextest run --locked --workspace --all-features` with cargo-nextest 0.9.152 (on Linux after installing ALSA's headers, `libasound2-dev`, which the preview host's sound output links, and Mesa's software Vulkan driver, `mesa-vulkan-drivers`, for the UI snapshot harness, which renders through wgpu; a failing run keeps the snapshots that did not match as the `snapshots-<os>` artifact), then `cargo xtask oracles`: fetches id's and FTEQCC's sources by checksum, builds the VM oracle, id's qcc and FTEQCC with the system C compiler, and runs every VM vector through the VM oracle ([`oracles/README.md`](../oracles/README.md)); the fetched sources and binaries are cached, keyed on `oracles/` and the xtask code that builds them. The builtin differential, FTEQCC and qcc on 78870 cases and half an hour on a runner, runs live only when the change touches `oracles/`, `xtask/src/oracles.rs`, `xtask/src/oracles/` or `xtask/fixtures/oracles/`, or the base is unknown; otherwise `cargo xtask oracles --recorded` checks the profiles and the project graph against the outcomes `xtask/fixtures/oracles/builtin-outcomes.json` records, and the FTEQCC profile against the 1000 grammar programs and outcomes `xtask/fixtures/oracles/grammar-outcomes.json` records (DIFF-13), in about a minute, and the nightly `Differential` workflow runs the live check on `main` |
| `Build (linux-x86_64)`, `Build (linux-aarch64)` | x86_64 and arm64 Linux runners | after installing ALSA's headers, `cargo build --profile ci --locked --workspace` for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` |
| `Build (windows-x86_64)`, `Build (windows-aarch64)` | Windows | the same for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` (cross-compiled) |
| `Build (macos-universal)` | macOS | builds `aarch64-apple-darwin`, the one macOS target (Apple silicon), and keeps the preview host with its SHA-256 as the `quakec-engine-macos-arm64` artifact; every build job keeps the preview host and its SHA-256 as the `quakec-engine-<target>` artifact for seven days (P3-02). The job keeps the name the default-branch ruleset requires |
| `cargo xtask ci` | Linux | the command `make ci` runs, less the steps another job runs with the same command (`cargo xtask ci --skip`, [below](#which-job-runs-each-step-of-cargo-xtask-ci)): the plan checks (`plan/check.py` and its tests, with PyYAML installed), the cached specifications (`scripts/refresh-specs.sh --verify`, offline: fails naming any file under `specs/` whose SHA-256 differs from `specs/manifest.tsv`, that is listed but missing, or that has no row), conventions, ADRs, layers, licenses, attribution, contrast, the `qs-commands` `release_guard` example built and run under `--profile release` (the command guard catches a panic in a release build, P2-12; a test target would not show it, since Cargo builds tests with unwind whatever the profile says), the `commands` check (`docs/commands.md` matches the command registry); on a cache run on `main`, fmt and cargo-deny too |
| `cargo-deny` | Linux | `cargo deny --locked --all-features check advisories bans licenses sources` with cargo-deny 0.22.0, the version the `cargo xtask ci` job installs |
| `Dependency Review` | Linux | `dependency-review-action` on pull requests; skipped on merge groups and cache runs, which a required check accepts |
| `Test Assets (linux)`, `Test Assets (macos)`, `Test Assets (windows)` | each OS | the `language_coverage.py` tests, `cargo xtask corpus verify`, then `cargo xtask assets fetch` and `precache` ([test-assets.md](test-assets.md)), `cargo xtask oracles engine` on `oracles/engine/routes/start.route`, which builds the oracles and plays the route through id's server on LibreQuake's start map twice, checking the two traces agree and match `start.expected.jsonl` in canonical form, so the run is the same on every OS (P3-23, [`oracles/README.md`](../oracles/README.md#the-engine-oracle)), then `cargo xtask sim-diff` on the same route, which plays it through `qs-sim` too and fails on the first frame whose globals or edict fields differ from id's server's (P3-25, [DIFF-16](spec/differential.md)); on Linux only, `cargo xtask engines --check`, which fetches the engine-target database's fact sources by checksum and fails when `crates/qc-engines/src/tables.rs` differs from what they give ([engine-targets.md](spec/engine-targets.md), ENGT-1.2), `cargo xtask corpus fetch id-1.01`, which writes id 1.01 to the cache by checksum, `corpus/language_coverage.py`, which checks that [`docs/spec/language.md`](spec/language.md) covers every construct id's QuakeC 1.01 uses, `cargo xtask lex-corpus`, which lexes every `.qc` file and preprocesses the files `progs.src` lists with no errors, `cargo xtask oracles id101` on the same files, which compiles them with id's qcc, checks the progs.dat's SHA-256 and traces two scenarios with the VM oracle, `cargo xtask parse-corpus`, which parses id 1.01 with no errors and checks that every file of the rerelease, Copper and Arcane Dimensions round-trips through the syntax tree, then, when the parser chain changed ([above](#when-each-run-happens)), `cargo xtask parse-corpus --deletions`, which deletes each token of id 1.01 in turn to check that recovery stays local, and the same with `--fteqcc` on the rerelease under the FTEQCC profile, `cargo xtask oracles rerelease` on the six rerelease codebases, which compiles each with FTEQCC to a version 6 progs.dat with vanilla opcodes only and a `.lno`, checks both against recorded SHA-256 digests and keeps them in `target/oracles/rerelease` (cached on `oracles/`, the xtask oracle code and `corpus/manifest.toml`), `cargo xtask corpus build`, which checks that every progs.dat those oracles build round-trips through `qc-progs` byte for byte, `cargo xtask differential --fast`, which runs id 1.01 end to end in the VM oracle and QuakeC Studio's runner and fails on any divergence DIFF-10.1 does not list (the differential VM suite QA-1 names; the full corpus runs nightly, [below](#differential)), `cargo xtask compiler-diff --fast`, which compiles id 1.01 with id's qcc and with `qc-codegen`, calls every function of each progs in `qc-vm` 32 times and fails on any divergence or on less than 90% of qcc's statements executed (the compiler differential, [DIFF-12](spec/differential.md); 128 rounds run nightly), `cargo xtask corpus fetch` of every map in the manifest and `cargo xtask map-corpus`, which checks that each reads and writes back through `qs-map` byte for byte, the `qs-vfs` `walk` example on the LibreQuake lite asset, which loads every BSP, `.lit`, alias model, sprite and sound in it, and, on every OS, `cargo xtask bridge-client` on the same asset, which builds the preview host and runs `qs-engine`'s client against it headless, launched through a run root whose `id1` is a link (P3-06), then `cargo xtask scenario` on `xtask/fixtures/scenarios/start.scenario`, which plays the scenario in `qs-sim` on LibreQuake's start map ten times and fails unless every run passes with the same result (P3-19, SCN-2.5); the download cache, which holds the asset archives and the repository archives `corpus fetch` reads, is keyed on the SHA-256 of `corpus/manifest.toml` and saved after the last fetch |

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
| `nextest` | `cargo nextest run --locked --workspace --all-features` | `Unit Tests (linux)` (and macOS and Windows) | `Unit Tests (linux)` |
| `run` | `cargo run --locked -p qs-commands --profile release --example release_guard` | `cargo xtask ci` | `cargo xtask ci` |
| `commands` | `cargo xtask commands --check` | `cargo xtask ci` | `cargo xtask ci` |
| `keymap` | `cargo xtask keymap --check` | `cargo xtask ci` | `cargo xtask ci` |
| `deny` | `cargo deny --locked --all-features check advisories bans licenses sources` | `cargo-deny` | `cargo xtask ci` |
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

[`.github/workflows/secure_workflows.yml`](../.github/workflows/secure_workflows.yml) runs only
on a pull request that changes `.github/`, and its checks are not required:

| Check | What it does |
| --- | --- |
| `Locked Actions` | `gh actions-lock --verify`: fails when `.github/workflows/actions.lock`, which records the commit of every action's version tag, is out of step with the workflows or a locked commit no longer verifies upstream |
| `zizmor` | audits every workflow with zizmor and fails on any finding |

[`.github/workflows/bench.yml`](../.github/workflows/bench.yml) runs weekly and on demand
(`workflow_dispatch`), never on pull requests or in the merge queue, and is **not** a required
check:

| Check | Runs on | What it does |
| --- | --- | --- |
| `Benchmarks` | Linux | `cargo xtask bench` against the history saved from `main`, then uploads the history as the `bench-history` artifact ([perf-budgets.md](perf-budgets.md#in-ci)) |

[`.github/workflows/release.yml`](../.github/workflows/release.yml) runs on demand
(`workflow_dispatch`, a dry run), on a pushed `v*` tag, and as a dry run on a pull request that
changes `packaging/` or either release workflow; it is **not** a required check ([releasing.md](releasing.md)). It calls the reusable
[`.github/workflows/reusable-build.yml`](../.github/workflows/reusable-build.yml), the only
workflow that reads the Apple and Azure signing secrets, which builds, signs and attests every
package, so each attestation names the reusable workflow as its signer:

| Check | Runs on | What it does |
| --- | --- | --- |
| `Fetch LibreQuake for the install tests` | Linux | `cargo xtask assets fetch`, then uploads LibreQuake lite for the install jobs' `quakec-ide doctor` |
| `Build / Signing secrets are set` | Linux | fails naming each signing secret that is not set, printing no value |
| `Build / Version` | Linux | reads the workspace version and fails a tag that does not name it |
| `Build / Package, sign and notarize on macOS` | macOS | builds for Apple silicon, makes the app, signs it with the hardened runtime from `build.keychain`, notarizes and staples it, makes the installer package, signs it as the Developer ID Installer identity, notarizes and staples it, and attests it |
| `Build / Package on Windows (x64, arm64)` | Windows | on a tag, signs the four executables with aas-sign (Azure Artifact Signing over OIDC) and checks each signature; makes the zip and the MSI; on a tag, signs the MSI with Azure/artifact-signing-action after azure/login and checks its signature; attests them |
| `Build / Package on Linux (x86_64, aarch64)` | Linux | makes the deb, rpm, AppImage and Flatpak, signs the rpm with the key of packages.quakecstudio.com and attests them |
| `Install on macOS`, `Install on Windows (x64, arm64)`, `Install on Linux (x86_64, aarch64)` | each | installs each package on a clean runner and checks it with `quakec-ide doctor` |
| `Checksums and publish` | Linux | writes `SHA256SUMS`; on a tag, publishes the GitHub release |
| `Add to packages.quakecstudio.com` | Linux | adds the debs and rpms to a checkout of `quakecstudio/packages` and writes and signs its metadata, and writes the release feed `latest.json` the app's update check reads (not for a pre-release); on a tag with no `-`, pushes it |
| `Update the Homebrew tap` | Linux | writes the cask into a checkout of `quakecstudio/homebrew-tap`; on a tag with no `-`, pushes it |
| `Submit to winget` | Linux | only on a tag with no `-`: opens a pull request against `microsoft/winget-pkgs` with komac; its failure does not fail the run |
| `Regenerate the compatibility list` | Linux | writes `docs/compatibility.md` from the corpus and fails a tag whose list is out of date |

[`.github/workflows/evals.yml`](../.github/workflows/evals.yml) runs the agent's evals on a real
model (`cargo xtask evals --run`, [`evals/README.md`](../evals/README.md)). It runs weekly and on
demand, never on pull requests, and is **not** a required check. Each run stops at 2 US dollars.
Without the `QS_EVAL_API_KEY`
secret it does nothing:

| Check | Runs on | What it does |
| --- | --- | --- |
| `Agent evals` | Linux | fetches the starters' QuakeC and LibreQuake; runs each task within its cost cap; adds each run to the history kept in the Actions cache; writes the report to the job summary; uploads the report and the history |

On every full CI run, Test Assets (linux) runs `cargo xtask evals --check`. Each task's scenario
must fail at the task's start and pass with its reference solution.

It also runs `cargo xtask offline` over id 1.01 and LibreQuake (P4-16, PRIN-9). With no model
provider set up, each step runs with the network denied and traced: the CLI's `new` from a local
copy, `build`, `check` and `test`, an MCP session, and the desktop app's headless session. A
control step (`new` from GitHub) must fail and show its request in the trace.
[`docs/privacy.md`](privacy.md) lists every request the app can make.

### Nightly runs

`Fuzz` and `Differential` run on a schedule (`timezone: "America/New_York"`), `Fuzz` at 15:43
and `Differential` at 16:47; GitHub starts scheduled runs late under load, through November
2026 six to nine hours after their cron times, so they run in the evening. Every job has its
own `timeout-minutes`, about three times what the job last took: the longest, CRMod 7's
per-function run, took 58 minutes (run 44402847936), and the whole run under an hour.

Most of `Differential`'s runner time is the compiler differentials: generated programs, each
codebase's 128 rounds, and each codebase's functions on their own. They prove the compiler and
the VM against the oracles, so the same inputs give the same result. The `Compiler changes` job
hashes those inputs (the `qc-*` crates, the VM side's simulation and format crates,
`qs-differential`, xtask, the oracles, the corpus pins, `Cargo.lock`, the toolchain and the
workflow) and looks for the marker `Compiler differentials passed` saves on `main` once every
one of them has passed on those inputs. With the marker found, the compiler differentials are
skipped; a run started by hand (`workflow_dispatch`) runs them anyway. The jobs that only need
the oracles' programs run `cargo xtask oracles build`; the oracles' own vectors and
differentials run once, in the `Differential` job.

[`.github/workflows/fuzz.yml`](../.github/workflows/fuzz.yml) runs nightly and on demand
(`workflow_dispatch`), never on pull requests or in the merge queue, and is **not** a required
check:

| Check | Runs on | What it does |
| --- | --- | --- |
| `Fuzz the oracles` | Linux | `cargo xtask oracles fuzz all --time 600`: the VM oracle's own libFuzzer targets (scenario, init, progs) for 10 minutes each with clang 18; on a crash, uploads the crashing inputs |
| `Fuzz` | Linux | `cargo xtask fuzz --time 10m`: every cargo-fuzz target in `fuzz/` for 10 minutes on nightly Rust, the `parser` target seeded with the `.qc` files of id 1.01 and the rerelease (`cargo xtask corpus fetch`), and the `qc-vm` target run against the id VM oracle (`cargo xtask oracles` builds it and `QS_VM_ORACLE` points at it, so well-formed random progs are compared in both VMs, FND-10); on a crash, uploads `fuzz/artifacts/` as the `fuzz-artifacts` artifact |

### Differential

[`.github/workflows/differential.yml`](../.github/workflows/differential.yml) runs nightly and
on demand (`workflow_dispatch`), never on pull requests or in the merge queue, and is **not** a
required check (docs/ci.md): the full corpus takes minutes, and the fast subset (id 1.01, zero
divergences) runs on every change in the `Test Assets (linux)` job above.

| Check | Runs on | What it does |
| --- | --- | --- |
| `Compiler changes` | Linux | hashes the compiler differentials' inputs and looks for the marker saved when they last all passed on `main` on them; the compiler and per-function jobs below run only without it, or on a run started by hand, and the run's summary says which ([Nightly runs](#nightly-runs)) |
| `Differential` | Linux | `cargo xtask oracles`, the oracles' own vectors and the live builtin and grammar differentials, once a night (the pull requests that change no oracle code check them against their recordings), then `cargo xtask differential --full`: builds the oracles, compiles the rerelease with FTEQCC, builds the corpus progs, then runs every oracle-built codebase (id 1.01 and the six rerelease codebases) in the VM oracle and QuakeC Studio's runner, comparing traces (DIFF-9) and measuring statement coverage (DIFF-11); enforces zero divergences, and reports statement coverage without gating on it ([ADR-0013](adr/0013-differential-coverage-bar.md), TEST-6); then, in a `Compiler differential` job per codebase, in parallel and only when `Compiler changes` finds no marker, `cargo xtask oracles build` and `cargo xtask compiler-diff --full <codebase>`, the compiler differential with 128 rounds of calls on id 1.01 against qcc and on each rerelease codebase and corpus mod at the bar against the pinned FTEQCC (DIFF-12, DIFF-12.30, DIFF-12.18), each job's results table published in the run's summary |
| `Compiler differential (<codebase>)` | Linux | only when `Compiler changes` finds no marker, `cargo xtask oracles build`, `cargo xtask corpus fetch <codebase>` and `cargo xtask compiler-diff --full <codebase>`: the job per codebase the `Differential` row describes |
| `Compiler differential (generated programs)` | Linux | only when `Compiler changes` finds no marker, `cargo xtask oracles build` and `cargo xtask compiler-diff --generated 10000 --seed 1`: 10,000 programs made from a seed, each compiled by our compiler and the pinned FTEQCC and the two run on the same calls (DIFF-14) |
| `Per-function differential (<codebase>)` | Linux | only when `Compiler changes` finds no marker, `cargo xtask oracles build` and `cargo xtask compiler-diff --functions <codebase>`: each function of a codebase the pinned FTEQCC compiles, cut into a program with others of its group and called alone, held to zero divergences and to DIFF-15.6's coverage bar (DIFF-15). Arcane Dimensions is the longest, over an hour of calls on one runner, so it runs as four `Per-function differential (arcane-dimensions, shard <k>/4)` jobs, `--shard <k>/4`, each uploading the statements it reached, and a `Per-function differential (arcane-dimensions, coverage)` job holds them together to the bar with `--merge` (DIFF-15.10) |
| `Compiler differentials passed` | Linux | on `main`, once `Differential` and every compiler and per-function job passed on inputs with no marker yet, saves the marker `Compiler changes` looks for |
| `Map tool oracles` | Linux | `cargo xtask oracles maps`: id's qbsp, vis and light from qutils and a pinned ericw-tools, fetched by checksum, built (ericw-tools with CMake, TBB and Embree 4, which the job installs) and each compiling `oracles/maps/room.map` to BSP29 (P6-03); then `cargo xtask oracles models`, which builds id's modelgen from the same pins and compares the models it and `qs-mdl` write from 1000 generated scripts (L-47, MDL-8); 30 minutes at most |
| `View differential (<os>)` | Linux, macOS | `cargo xtask assets fetch`, then `cargo xtask view-diff` with `--map` for each of LibreQuake lite's ten maps: the preview host's frames and sound against id's whole engine (VIEWDIFF) |
| `Simulation differential (<os>)` | Linux, macOS, Windows | `cargo xtask assets fetch`, then `cargo xtask sim-diff` on `oracles/engine/routes/start.route` with `--map` for each of LibreQuake lite's ten maps: `qs-sim` and id's server must have the same state after the map loads and after every frame (P3-25, DIFF-16.5) |

Every action is named by its version tag and locked to that tag's commit in
`.github/workflows/actions.lock` (`gh actions-lock` rewrites it after a workflow changes; "The locked commit is what runs when
the workflow executes", GitHub's Workflow dependency lockfiles, https://gh.io/actions-lockfile,
fetched 2026-11-16, so a tag moved upstream does not change what runs), every job has
least-privilege `permissions`, and every checkout sets `persist-credentials: false`.

**Differs from vouch:** vouch skips its build matrix in merge groups and reports a stand-in
check instead. Here every job runs on the pull request (and in the merge group when the queue
is on), so what merges has been built for every target.

## Protection of main

The `default-branch` ruleset ([settings](https://github.com/quakecstudio/editor/rules/25187180))
applies to `main`:

- **Pull request required**, with squash as the only merge method. No approving review is
  required: agents squash-merge their own PRs once CI passes, except `needs:human` issues.
- **No merge queue** since 2026-10-30. The ruleset lives in `quakecstudio/infra`
  (`environments/github/main.tf`, `merge_queue`); turning the queue back on is that one
  change, and `land.sh` then queues the PR instead of merging it.
- **Required status checks**: every check in the `ci.yml` table above (not `Benchmarks`), by the exact name it
  reports. A required check whose name never reports blocks every merge,
  so a job is renamed, added or removed in the same change as the ruleset.
- **No force pushes** and **no deletion**. Organization admins and the repository admin role
  can bypass the ruleset; agents never do.

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
