# qc-engines

The engine-target database (LANG-6): which builtins and extensions each Quake engine a project
can target supports, and the warning for a call of a builtin the target lacks (PRIN-8). The
editor, the compiler and the agent read it, so the agent cites builtins the target really has
(AGT-9).

A QuakeC builtin is declared as `= #N`, and the number is the only link between a progs and
the engine; engines differ in which numbers they implement. This crate holds a table for each
of seven engines: Vanilla (id's engine), QuakeSpasm, Ironwail, vkQuake, FTEQW, DarkPlaces and
KEX (the 2021 rerelease).

- **Layer:** L3 semantics. It depends on `qc-sema` (L3), `qc-syntax` and `qc-project` (L2),
  `qc-builtins` (L1, id's vanilla table) and `qc-diag` (L0).
- **Spec sections:** ENGT-1 (sources), ENGT-3 (engines) and ENGT-4 (the missing-builtin
  warning) of [`docs/spec/engine-targets.md`](../../docs/spec/engine-targets.md), and
  QCD-7.17 (`W0311`) of
  [`docs/spec/language-diagnostics.md`](../../docs/spec/language-diagnostics.md). The
  generator implements ENGT-2. Written from the spec, not ported.
- **Shipped:** yes.

## Sources and generation

The FTEQW, DarkPlaces and rerelease tables in `src/tables.rs` are generated, never edited by
hand. [`sources.toml`](sources.toml) pins each fact source by repository, commit and SHA-256,
with its repository's license file:

| Source | File | License (repository) |
| --- | --- | --- |
| `fteqw` | `fte-team/fteqw`, `quakec/menusys/fteextensions.qc` | GPL-2.0, no header in the file |
| `darkplaces` | `DarkPlacesEngine/darkplaces`, `dpdefs/dpextensions.qc` | GPL-2.0, no header in the file |
| `rerelease` | `id-Software/quake-rerelease-qc`, `quakec/defs.qc` | GPL-2.0-or-later, the file's header |

Only facts are extracted from them: builtin numbers and engine names, QuakeC names, signatures
and extension names. No comment text or code is copied, and none of them is a porting upstream;
[ADR-0014](../../docs/adr/0014-engine-fact-sources.md) records the decision and its legal
basis. The vanilla table is `qc-builtins`', ported from id's engine.

```bash
cargo xtask engines           # fetch the pinned files by checksum and write src/tables.rs
cargo xtask engines --check   # fail if src/tables.rs differs from what the sources give (CI)
cargo test -p qc-engines
```

## Public shape

- `Engine`: the seven engines, with `name`, `id`, `from_id`, `from_target` (a project file's
  target), `builtins`, `builtin` and `extensions`.
- `Builtin` and `BuiltinKey`: a table entry, keyed by number (`#N`) or engine name (`#0:NAME`).
- `SOURCES` and `Source`: the pinned fact sources and their checksums.
- `missing_builtins` and `FileDiagnostic` (`check`): the `W0311` warnings for a module checked
  against a target engine.

## What it is not, yet

The tables are for server QuakeC; CSQC and menu QuakeC tables, calls of `#0:NAME` builtins
(which the vanilla parser does not accept), and the QuakeSpasm lineage's own extension lists
are later work (see the spec's ENGT-3 notes).
