# Layering

Read before adding a crate or a dependency between crates.

**A crate may depend only on crates in its own layer or a lower one** (ARCH-1, design doc
"Architecture"). Never upward. [`docs/layers.md`](../../docs/layers.md) explains the model,
including why crates in one layer may build on each other.

| Layer | Holds |
| --- | --- |
| L0 formats | File formats as id defined them: progs.dat, .lno, PAK, WAD2, .map, BSP29/BSP2, MDL; the paths module; logging; diagnostics; the game filesystem and install detection; line diffs; git; checkpoints as git objects |
| L1 VM | What the engine executes: the VM, edicts, builtins |
| L2 syntax | Preprocessor, lossless CST, `progs.src` project graph |
| L3 semantics | Resolution, types, dialects, engine-target database, LSP, map entity definitions |
| L4 compilers and engine runtime | Our QuakeC and map compilers (bsp, vis, light); the engine bridge and its client; the preview host's game client, simulation and sound |
| L5 commands | Command engine, session, undo: one registry for the UI, CLI, agent and MCP; the workbench, layout, settings and templates |
| L6 agent | Agent loop, model providers, checkpoints, MCP and ACP |
| L7 UI | The app on the chosen framework (ADR-0003): the code editor view and the wgpu renderer |
| apps | Desktop app, CLI, LSP server, preview host |
| tests | Test-only crates, outside the ranking |
| tooling | Build automation, outside the ranking |

**Which crate is in which layer** is the generated table in
[`docs/layers.md`](../../docs/layers.md#the-crates), written from `layers.toml` by
`cargo xtask layers --write` and checked by `cargo xtask layers`. It is the only list of
crates; do not copy it here.

The design is in
[`docs/design/design-doc.md`](../../docs/design/design-doc.md) (Architecture) and
[`docs/roadmap.md`](../../docs/roadmap.md#how-the-code-is-layered). Each crate's README
states its layer. Planned crates are named as in the design doc; the final names take the
project prefix from ADR-0006 (`docs/conventions.md`, Repository layout).

## Enforcement

- `layers.toml` maps every crate to a layer and `cargo xtask layers` fails on an upward or
  unmapped dependency, and when the crate table in `docs/layers.md` is stale. A new crate is
  added to `layers.toml`, and the table rewritten with `cargo xtask layers --write`, in the
  same PR.
- Test-only crates (`qs-tests`, `qs-bench` and `qs-differential`, layer `tests`) and tooling (`xtask`, layer `tooling`) may depend
  on anything; nothing may depend on them.

## What follows from it

- **The UI is a thin top layer.** Everything a user can do is a command in L5, so the UI, the
  CLI, the agent and MCP share one implementation. Logic in L7 that is not rendering belongs
  in a lower layer.
- **Lower layers know nothing of higher ones.** Callbacks, traits or data passed down
  replace upward calls. A type shared by two layers lives in the lower one.
- **Readiness gates climbing.** Work in a layer that builds on a lower layer waits for that
  layer's readiness review ([`testing.md`](testing.md)); `plan/check.py` adds the dependency.
- **C programs are separate processes.** The oracles (`oracles/`, CI only) talk to the editor
  over files or a socket, never by linking. The preview host is a Rust app
  (`apps/quakec-engine`, ADR-0004) that also runs as its own process, over a local socket.
