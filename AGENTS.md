# AGENTS.md

See [`CLAUDE.md`](CLAUDE.md) for the project overview, architecture, conventions and commands,
and [`.claude/rules/`](.claude/rules/) for the detailed rules. Both apply to every agent,
whatever tool runs it.

## Environment setup

### What a session needs

| Tool | Why | Where it comes from |
| --- | --- | --- |
| rustup | The exact toolchain in `rust-toolchain.toml` (1.99.0 with rustfmt and clippy) | [rustup.rs](https://rustup.rs/); cargo installs the pinned toolchain on first use |
| A C toolchain and `pkg-config` | Build scripts of dependencies, and the oracles (`cargo xtask oracles`) | The OS (`build-essential` on Debian and Ubuntu, Xcode command line tools on macOS) |
| Python 3 with PyYAML | Everything in `plan/` | `python3-yaml`, or `pip install pyyaml` |
| `gh`, signed in to the `quakecstudio` org | Issues, PRs and the merge queue | [cli.github.com](https://cli.github.com/) |
| `prek` | The pre-commit hooks | `pip install prek` |
| `shellcheck`, `shfmt` | Shell linting in the hooks | The OS package manager |
| `curl`, `shasum` or `sha256sum` | `scripts/refresh-specs.sh` | The OS |
| `cargo-deny` | `cargo deny check` | `cargo install --locked cargo-deny` |
| `cargo-nextest` | `make test` and the test step of `cargo xtask ci` | `cargo install --locked cargo-nextest` |

### Claude Code on the web and other Linux containers

`.claude/hooks/session-setup.sh` runs at session start (registered in
`.claude/settings.json`). On Linux it installs the system packages above, rustup and `prek`
when they are missing, and installs the git hooks from `.pre-commit-config.yaml`. Each
step skips silently if the network policy blocks its download, and the hook always exits 0.
It does nothing on macOS or Windows, where you set up the tools yourself.

`cargo-deny`, `cargo-nextest`, `cargo-llvm-cov` and `cargo-mutants` are not installed by the hook because they
take minutes to build; install them when a task needs them.

### Network

- `cargo xtask deny-fixtures` and `cargo deny` need the crates.io index.
- `scripts/refresh-specs.sh` fetches from `raw.githubusercontent.com`; `--verify` is offline.
- `cargo xtask oracles` fetches id's sources from `raw.githubusercontent.com` once, into
  `target/oracles/src/`, and checks their SHA-256 on every run.
- `plan/status.py` and `plan/apply.py` call the GitHub API through `gh`.

## Key commands

| Task | Command |
| --- | --- |
| Everything CI runs | `make ci` (`cargo xtask ci`) |
| Format | `cargo fmt --all` |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` |
| Tests | `cargo test --locked` |
| Cross-crate tests and the spec-citation ratchet | `cargo test -p qs-tests` |
| Plan checks | `python3 plan/check.py --write-coverage` |
| Phase progress | `python3 plan/status.py --phase P0` |

## Gotchas

- Scratch workspaces of `xtask` live under `target/xtask/`; delete `target/` when disk runs low.
- The spec-citation ratchet reads `docs/spec/` and every `.rs` file under `crates/` and
  `apps/`. When a spec statement changes, update `docs/spec/requirements.tsv` with the
  contents the failing test prints.
- `specs/` must stay byte-identical to the origins; never reformat it, and let
  `scripts/refresh-specs.sh` write it.
- Quake's game data (`pak0.pak`, `pak1.pak`) is never in the repository or CI; CI uses the
  open assets in [`docs/test-assets.md`](docs/test-assets.md), fetched by checksum with
  `cargo xtask assets fetch`.
