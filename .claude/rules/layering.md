# Layering

Read before adding a crate or a dependency between crates.

**A crate may depend only on crates in its own layer or a lower one** (ARCH-1, design doc
"Architecture"). Never upward. [`docs/layers.md`](../../docs/layers.md) explains the model,
including why crates in one layer may build on each other.

| Layer | Holds | Crates (existing or planned) |
| --- | --- | --- |
| L0 formats | File formats as id defined them: progs.dat, .lno, PAK, WAD2, .map, BSP29/BSP2, MDL; the paths module; logging; diagnostics; the game filesystem and install detection | `qs-paths`, `qs-log`, `qc-diag`, `qc-progs`, `qc-types`, `qc-lno`, `qs-pak`, `qs-wad`, `qs-wav`, `qs-bsp`, `qs-map`, `qs-mdl`, `qs-vfs`, `qs-install` |
| L1 VM | What the engine executes: the VM, edicts, builtins | `qc-vm`, `qc-edict`, `qc-builtins`, `qc-trace`, `qs-porting-example` |
| L2 syntax | Preprocessor, lossless CST, `progs.src` project graph | `qc-lexer`, `qc-preproc`, `qc-syntax`, `qc-project` |
| L3 semantics | Resolution, types, dialects, engine-target database, LSP, map entity definitions | `qc-sema`, `qc-engines` |
| L4 bridges | Our QuakeC and map compilers; the engine bridge | |
| L5 commands | Command engine, session, undo: one registry for the UI, CLI, agent and MCP | |
| L6 agent | Agent loop, model providers, checkpoints, MCP and ACP | |
| L7 UI | The app on the chosen framework (ADR-0003) | |
| apps | Desktop app, CLI, LSP server binaries | `apps/*` |
| tests | Test-only crates, outside the ranking | `qs-tests`, `qs-bench`, `qs-differential` |
| tooling | Build automation, outside the ranking | `xtask` |

The table is the summary; the design is in
[`docs/design/design-doc.md`](../../docs/design/design-doc.md) (Architecture) and
[`docs/roadmap.md`](../../docs/roadmap.md#how-the-code-is-layered). Each crate's README
states its layer. Planned crates are named as in the design doc; the final names take the
project prefix from ADR-0006 (`docs/conventions.md`, Repository layout).

## Enforcement

- `layers.toml` maps every crate to a layer and `cargo xtask layers` fails on an upward or
  unmapped dependency. A new crate is added to `layers.toml` in the same PR.
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
  (`apps/preview-host`, ADR-0004) that also runs as its own process, over a local socket.
