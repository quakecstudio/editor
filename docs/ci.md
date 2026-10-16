# CI and the protection of main

CI runs on every pull request, every merge-queue group and every push to `main`
(QA-1, PM-4, CONV-7). Nothing reaches `main` except through the merge queue, and the
queue merges a group only when every required check below has passed on it.

## Workflows

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml):

| Check | Runs on | What it does |
| --- | --- | --- |
| `Format` | Linux | `cargo fmt --all --check` |
| `Clippy` | Linux | `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` |
| `Unit Tests (linux)`, `Unit Tests (macos)`, `Unit Tests (windows)` | each OS | `cargo test --locked --workspace --all-features` |
| `Build (linux-x86_64)`, `Build (linux-aarch64)` | x86_64 and arm64 Linux runners | `cargo build --profile ci --locked --workspace` for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` |
| `Build (windows-x86_64)`, `Build (windows-aarch64)` | Windows | the same for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` (cross-compiled) |
| `Build (macos-universal)` | macOS | builds `aarch64-apple-darwin` and `x86_64-apple-darwin`, then joins every binary with `lipo` and checks both architectures are in it |
| `cargo xtask ci` | Linux | the same command as `make ci`: fmt, conventions, ADRs, layers, licenses, attribution, clippy, tests, clippy fixtures, cargo-deny and deny fixtures |
| `cargo-deny` | Linux | `cargo-deny-action` with `check advisories bans licenses sources` |
| `Dependency Review` | Linux | `dependency-review-action` on pull requests; skipped on merge groups and pushes, which a required check accepts |
| `Test Assets (linux)`, `Test Assets (macos)`, `Test Assets (windows)` | each OS | the corpus script tests, `corpus/verify.py`, then `corpus/assets.py fetch` and `precache` ([test-assets.md](test-assets.md)); the download cache is keyed on the SHA-256 of `corpus/manifest.toml` |

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
