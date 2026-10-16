# xtask

Build automation for the workspace, run as `cargo xtask <command>` (the alias is in
`.cargo/config.toml`).

- **Layer:** tooling; not shipped.
- **Spec sections:** none.

| Command | What it checks |
| --- | --- |
| `ci` | Everything a change must pass before it merges, in one run: `cargo fmt --check`, `conventions`, `adr`, `layers`, `licenses`, `attribution`, clippy with `-D warnings`, the tests, `clippy-fixtures`, `cargo deny check` and `deny-fixtures`. Every step runs; it exits non-zero if any failed. `make ci` and CI run this |
| `adr` | Every ADR in `docs/adr/` follows `0000-template.md` and is listed in the index in `docs/adr/README.md` (ADR-0001) |
| `conventions` | Every config value matches `docs/conventions.md` (see "Checking conformance" there) |
| `clippy-fixtures` | The workspace lints reject each fixture in `fixtures/clippy/` with the lint it names, and pass the controls |
| `deny-fixtures` | `deny.toml` rejects a GPL-incompatible license and a wildcard version, and passes a control; needs the crates.io index |
| `layers` | Every workspace crate is in `layers.toml`, and depends only on crates in its own layer or lower ([`docs/layers.md`](../docs/layers.md)) |
| `licenses` | Every file is classified; source files start with an allowed SPDX line and pass the ported-file header rule (H1 to H5 in [`docs/porting.md`](../docs/porting.md)); no file carries a disallowed SPDX tag; `LICENSE` is the GPL-3.0 text |
| `attribution` | Every asset file (images, fonts, sounds, Quake formats) has a row in [`ATTRIBUTION.md`](../ATTRIBUTION.md) with an allowed license, and every row names a file in the repository |

`licenses` and `attribution` check the tracked files plus untracked files that are not
ignored, so a new file is caught before it is committed.

Scratch workspaces go under `target/xtask/`; the self-tests' fixture workspaces go under
`target/xtask-tests/`.
