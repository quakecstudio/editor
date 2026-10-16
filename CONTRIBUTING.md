# Contributing to QuakeC Studio

Thank you for helping. This page covers how to build, what a change must pass and how it lands.
Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- Work is tracked as GitHub issues generated from [`plan/`](plan/README.md). Pick an open issue
  whose dependencies are closed; to change the plan, edit `plan/` rather than an issue body.
- Read [`docs/conventions.md`](docs/conventions.md), the Rust conventions, and
  [`docs/testing.md`](docs/testing.md), the bar a layer's tests must meet.
- Porting from id Software's Quake source is welcome: follow [`docs/porting.md`](docs/porting.md).
- Never commit Quake's game data (`pak0.pak`, `pak1.pak`) or QuakeC 1.06, which has no license.

## Build and check

You need [rustup](https://rustup.rs/); `rust-toolchain.toml` pins the exact toolchain, and
`cargo` installs it on first use. `cargo deny` checks come from
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny).

```bash
make ci                        # everything CI runs: cargo xtask ci
make help                      # every target: fmt, lint, test, audit, test-coverage, ...
```

`cargo xtask ci` runs, in order: `cargo fmt --all --check`, `cargo xtask conventions` (the
config matches `docs/conventions.md`), `adr`, `layers` (crates depend only on their own layer or
lower, `docs/layers.md`), `licenses` (SPDX and ported-file headers), `attribution` (every asset
is in `ATTRIBUTION.md`), clippy with `-D warnings`, the tests, `clippy-fixtures`,
`cargo deny check` and `deny-fixtures`. The last two need network access.

Shipped crates never panic: `unwrap`, `expect`, indexing, unchecked arithmetic and `as` casts
are denied by lint. An exception is `#[expect(lint, reason = "...")]` at the narrowest scope,
never `#[allow]`. Every source file starts with `// SPDX-License-Identifier: GPL-3.0-or-later`.

## Commits and pull requests

- Branch from `main` as `<type>/<issue>-<slug>`, for example `feat/45-qc-types`.
- Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): an imperative
  description of at most 72 characters with no trailing period.
- Open a pull request that says what changed and how each acceptance criterion is met, with
  `Closes #<issue>`. `main` only accepts changes through the merge queue, which squash-merges.

## License

QuakeC Studio is licensed GPL-3.0-or-later ([ADR-0002](docs/adr/0002-license-model.md)). By
contributing you agree that your contribution is licensed the same way. Dependencies must be
compatible with GPL-3.0-or-later; `deny.toml` lists the allowed licenses.
