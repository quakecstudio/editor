# xtask

Build automation for the workspace, run as `cargo xtask <command>` (the alias is in
`.cargo/config.toml`).

- **Layer:** tooling; not shipped.
- **Spec sections:** none.

| Command | What it checks |
| --- | --- |
| `adr` | Every ADR in `docs/adr/` follows `0000-template.md` and is listed in the index in `docs/adr/README.md` (ADR-0001) |
| `conventions` | Every config value matches `docs/conventions.md` (see "Checking conformance" there) |
| `clippy-fixtures` | The workspace lints reject each fixture in `fixtures/clippy/` with the lint it names, and pass the controls |
| `deny-fixtures` | `deny.toml` rejects a GPL-incompatible license and a wildcard version, and passes a control; needs the crates.io index |

Scratch workspaces go under `target/xtask/`.
