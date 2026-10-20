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
- **Spec sections:** ENGT-1 (sources), ENGT-3 (engines) and ENGT-4 (the missing-builtin warning)
  of [`docs/spec/engine-targets.md`](../../docs/spec/engine-targets.md). Written from the spec,
  not ported.
- **Not this crate's:** ENGT-2, the extraction from the fact sources, which the generator `cargo
  xtask engines` implements.
- **Spec statements:** QCD-7.17 (`W0311`) of
  [`docs/spec/language-diagnostics.md`](../../docs/spec/language-diagnostics.md).
- **Shipped:** yes.

## Sources and generation

The QuakeSpasm, Ironwail, vkQuake, FTEQW, DarkPlaces and rerelease tables in `src/tables.rs`
are generated, never edited by hand. [`sources.toml`](sources.toml) pins each fact source by repository, commit and SHA-256,
with its repository's license file:

| Source | File | License (repository) |
| --- | --- | --- |
| `fteqw` | `fte-team/fteqw`, `quakec/menusys/fteextensions.qc` | GPL-2.0, no header in the file |
| `darkplaces` | `DarkPlacesEngine/darkplaces`, `dpdefs/dpextensions.qc` | GPL-2.0, no header in the file |
| `darkplaces-client` | `DarkPlacesEngine/darkplaces`, `dpdefs/csprogsdefs.qc` | GPL-2.0, no header in the file |
| `darkplaces-menu` | `DarkPlacesEngine/darkplaces`, `dpdefs/menudefs.qc` | GPL-2.0, no header in the file |
| `rerelease` | `id-Software/quake-rerelease-qc`, `quakec/defs.qc` | GPL-2.0-or-later, the file's header |
| `quakespasm` | `sezero/quakespasm`, `Quake/pr_cmds.c` | GPL-2.0-or-later, the file's header |
| `quakespasm-rerelease` | `sezero/quakespasm`, `Quake/pr_edict.c` | GPL-2.0-or-later, the file's header |
| `ironwail` | `andrei-drexler/ironwail`, `Quake/pr_cmds.c` | GPL-2.0-or-later, the file's header |
| `vkquake` | `Novum/vkQuake`, `Quake/pr_cmds.c` | GPL-2.0-or-later, the file's header |
| `vkquake-ext` | `Novum/vkQuake`, `Quake/pr_ext.c` | GPL-2.0-or-later, the file's header |

Only facts are extracted from them: builtin numbers and engine names, QuakeC names, signatures
and extension names. No comment text or code is copied, and none of them is a porting upstream;
[ADR-0014](../../docs/adr/0014-engine-fact-sources.md) records the decision and its legal
basis, [ADR-0015](../../docs/adr/0015-darkplaces-client-menu-fact-sources.md), which
supersedes it, adds DarkPlaces' client and menu QuakeC definitions, and
[ADR-0016](../../docs/adr/0016-quakespasm-lineage-fact-sources.md) adds the builtin tables of
the QuakeSpasm lineage, read from their C array initialisers (ENGT-2.11 to ENGT-2.17). The
vanilla table is `qc-builtins`', ported from id's engine.

```bash
cargo xtask engines           # fetch the pinned files by checksum and write src/tables.rs
cargo xtask engines --check   # fail if src/tables.rs differs from what the sources give (CI)
cargo test -p qc-engines
```

## Public shape

- `Engine`: the seven engines, with `name`, `id`, `from_id`, `from_target` (every project
  file target, ENGT-3.5 and ENGT-3.6), and, for a module kind, `has_table`, `builtins`,
  `builtin` and `extensions`.
- `Builtin` and `BuiltinKey`: a table entry, keyed by number (`#N`) or engine name (`#0:NAME`).
- `SOURCES` and `Source`: the pinned fact sources and their checksums.
- `missing_builtins` and `FileDiagnostic` (`check`): the `W0311` warnings for a module of a
  kind checked against a target engine, for builtins declared `#N` and, under FTEQCC,
  `#0:NAME`, `#"NAME"` and `:NAME` (ENGT-4.14), in the dialect the module was parsed in: a
  number is the one the dialect's compiler binds, `(int)` of the literal's float value
  (ENGT-4.12), and under FTEQCC the value FTEQCC reads, hex, octal and `i` literals included
  (ENGT-4.11, ENGT-4.13).

## Module kinds

Every engine has a server QuakeC table. FTEQW also has client (CSQC) and menu QuakeC tables,
read from the same pinned `fteextensions.qc` with `CSQC` or `MENU` defined, and DarkPlaces
has them from its `csprogsdefs.qc` and `menudefs.qc` (ENGT-2.10); Ironwail and vkQuake have
client QuakeC tables from their own builtin tables (ENGT-3.11). A module of a kind its
engine has no table for is not checked (ENGT-4.8).

## What it is not, yet

The C sources give no signatures for QuakeSpasm's and Ironwail's own builtins, and no
QuakeC names for the positional arrays of QuakeSpasm and vkQuake, so those entries carry the
C function's name and an empty signature unless id's table names them (ENGT-2.14,
ENGT-3.9).
