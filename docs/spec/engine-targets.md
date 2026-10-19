# Engine targets

Prefix: `ENGT` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

The engine-target database (LANG-6, P2-04): which builtins and extensions each Quake engine
a project can target supports, where those facts come from, and the diagnostic for a call to
a builtin the target engine lacks (PRIN-8). Implemented by `qc-engines`, with the generator
`cargo xtask engines` (`xtask/src/engines.rs`).

The three files the tables are generated from are **fact sources, not porting upstreams**:
only facts are taken from them (names, numbers, signatures), never comment text or code, and
none of them is listed in [`../porting.md`](../porting.md#upstreams).
[ADR-0014](../adr/0014-engine-fact-sources.md) records the decision and its legal basis.

## ENGT-1 Sources

**Source:** `crates/qc-engines/sources.toml`, which pins:

| Source | Repository and commit | File | SHA-256 |
| --- | --- | --- | --- |
| `fteqw` | `fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e` | `quakec/menusys/fteextensions.qc` | `21322f15aca52dae3a75b98a46be0491ed9a1f5d136511228bf52725ac81ccb0` |
| `darkplaces` | `DarkPlacesEngine/darkplaces@5d80f03e26871b273da103f3b750820a3718c01a` | `dpdefs/dpextensions.qc` | `73237e430e8d36009bf0518c0105b15fb37d7710cdc47120c67da8e5e85e33e5` |
| `rerelease` | `id-Software/quake-rerelease-qc@634eefab09a77eb7b5f5ca7078ba3d8784a91142` | `quakec/defs.qc` | `7694c006223ec43f05b923c8fe7691a6ed8298141d1562d98d6c86162de6129d` |

The FTEQW and DarkPlaces commits were the tips of each repository's `master` on 2026-10-19.
The vanilla builtins are not extracted: they are id's own table, ported from id's engine in
`qc-builtins` (BUILTIN-5).

### Behaviour

- **ENGT-1.1** The database is built from the three files above, each pinned by repository,
  commit and SHA-256 together with its repository's license file, and from id's vanilla table
  of `qc-builtins` (BUILTIN-5). The generated `crates/qc-engines/src/tables.rs` records every
  pin, so the checked-in tables carry their source checksums.
- **ENGT-1.2** Generation is reproducible: `cargo xtask engines` fetches each pinned file and
  writes the tables, and `cargo xtask engines --check` writes nothing and fails when the
  checked-in tables differ from what the sources give. A fetched file or license file whose
  SHA-256 is not the pinned one is an error, and nothing is written.
- **ENGT-1.3** The `rerelease` pin is the corpus manifest's `quake-rerelease-qc` pin
  (`corpus/manifest.toml`): the same commit and the same SHA-256 for `quakec/defs.qc` and
  `COPYING.txt`. A difference is an error.

## ENGT-2 Extraction

**Source:** this project. The rules read the three files as they are at the pinned commits.

### Behaviour

- **ENGT-2.1** Only facts are kept: for each builtin its key (ENGT-2.4), its QuakeC name, its
  signature and the extensions it belongs to (ENGT-2.7), and for each file its extension names
  (ENGT-2.6). No comment text, no code and no other declaration is kept.
- **ENGT-2.2** A `//` comment runs to the end of its line and a `/* */` comment may span
  lines; a string literal is code, so a `//` inside one starts no comment. Comments are read
  only for the extension names of ENGT-2.6 and ENGT-2.7.
- **ENGT-2.3** `#if`, `#ifdef`, `#ifndef`, `#else` and `#endif` are evaluated, with `#if`
  expressions built from `defined(NAME)`, `defined NAME`, `!`, `&&`, `||`, parentheses and
  decimal numbers, over the macros the source defines before it is read (`NQSSQC` and `SSQC`
  for `fteqw`, choosing its server QuakeC builtins; none for the others) and the `#define` and
  `#undef` lines of the parts that are read. Only those parts are read. `#pragma` is ignored;
  any other directive, an `#if` that cannot be read, an `#else` or `#endif` with no open block,
  or a block still open at the end of the file is an error.
- **ENGT-2.4** A statement `TYPE NAME = #N`, with `N` at least 1, is the builtin with key
  number `N`; `TYPE NAME = #0:ENGINE_NAME` is the builtin the engine looks up by
  `ENGINE_NAME`. `TYPE` is a function type, and its whitespace is collapsed to single spaces;
  the C-like form `RET NAME(PARAMS) = #N` gives the signature `RET(PARAMS)`. A `#0` with no
  name, a number that is not decimal digits, or a builtin whose name or type cannot be read is
  an error. When a file declares one key twice, the first declaration is kept.
- **ENGT-2.5** The deprecation markers `DEP`, `DEP_CSQC`, `FTEDEP(...)` and
  `__deprecated(...)` before a builtin's type are dropped, with their argument.
- **ENGT-2.6** A file's extension names, each kept once in file order, are the names of its
  `#define NAME` lines with no parameters and no body and the comment lines that hold nothing
  but a name, when the name has the shape of an extension: upper-case ASCII letters, digits
  and underscores, starting with a letter, with an underscore, and not ending in one.
- **ENGT-2.7** A builtin's extensions are the names after `Part of ` in the comment on the
  line where its declaration ends, separated by commas, up to the first word that is not
  upper-case letters, digits, underscores and `?`; without one, the last comment-line name of
  ENGT-2.6 above it; without either, none.

## ENGT-3 Engines

**Source:** the tables of ENGT-1 and ENGT-2, and for Ironwail and vkQuake their READMEs:
`andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd`, `README.md`
(SHA-256 `4fa1c4d577d7c19e7320f0ff04e1d6b139968c7dcaccd6dd729e40ea7d27cb66`), which lists
"ability to play the [2021 release content](https://store.steampowered.com/app/2310/QUAKE/)
with zero setup", and `Novum/vkQuake@27789d7333a46674474d871137bb71e056719628`, `readme.md`
(SHA-256 `f1c0767470d6d7183284cf9914bd03d23d4fe4b193be6f6227f2008b93d17e1d`): "vkQuake has
support for playing the 2021 re-release content." Both read on 2026-10-19. The rerelease
QuakeC calls the builtins its `defs.qc` declares, so an engine that plays it has them.

None of the three fact sources describes QuakeSpasm, Ironwail or vkQuake, and none of those
engines' own sources is a fact source here, so their tables hold only what the sources above
establish. An engine may support builtins its table lacks; listing them needs that engine's
own source as a further fact source.

### Behaviour

- **ENGT-3.1** There are seven engines, in this order: Vanilla (id's engine), QuakeSpasm,
  Ironwail, vkQuake, FTEQW, DarkPlaces and KEX (the 2021 rerelease). Each engine's builtin
  table is the union of these layers, the first layer that has a key giving its entry:
  Vanilla and QuakeSpasm, id's table (BUILTIN-5); Ironwail and vkQuake, id's table and then
  the `rerelease` builtins; FTEQW, the `fteqw` builtins; DarkPlaces, id's table and then the
  `darkplaces` builtins; KEX, the `rerelease` builtins alone.
- **ENGT-3.2** KEX's table is exactly what the rerelease `defs.qc` declares, so it has no
  number 23, 24 or 73: the rerelease declares `bprint`, `sprint` and `centerprint` as
  `#0:ex_bprint`, `#0:ex_sprint` and `#0:ex_centerprint` instead, and is silent on the
  numbers.
- **ENGT-3.3** FTEQW's extension names are the `fteqw` file's, DarkPlaces' the `darkplaces`
  file's and KEX's the `rerelease` file's, which declares none (ENGT-2.6); Vanilla,
  QuakeSpasm, Ironwail and vkQuake have none.
- **ENGT-3.4** Each engine has a display name (`Vanilla`, `QuakeSpasm`, `Ironwail`,
  `vkQuake`, `FTEQW`, `DarkPlaces`, `KEX`) and an id (`vanilla`, `quakespasm`, `ironwail`,
  `vkquake`, `fteqw`, `darkplaces`, `kex`); reading any other id gives no engine.
- **ENGT-3.5** A project file's target (PRJ-2.2) selects an engine: `quake` is Vanilla,
  `rerelease` KEX, `fte` FTEQW and `dp` DarkPlaces.

## ENGT-4 Missing builtins

**Source:** this project, for PRIN-8: "warn ... when an engine lacks a builtin".

### Behaviour

- **ENGT-4.1** In a module checked against a target engine, a call `f(...)` whose callee is a
  name that resolves (QC-12.5) to a global function declared with a builtin number `#N`
  (QC-13.3) that the engine's table lacks is warning `W0311` (QCD-7.17) at the callee's name,
  with the message `NAME is builtin #N, which ENGINE does not have`, `ENGINE` being the display
  name. When an engine's table has `#N` with extensions, the first such engine in the order
  of ENGT-3.1 adds them: ` (EXT, EXT)`.
- **ENGT-4.2** There is no `W0311` when the engine has `#N`, for a call of a name that is not
  a builtin, for a use of a builtin that is not a call's callee, or for a builtin that is
  declared and never called.
- **ENGT-4.3** A builtin's number is that of the first `#N` initialiser of its name, in
  compilation order (QC-11.5). A number that is not a decimal literal of at least 1, with an
  optional fraction of zeros, is no number here and gives no `W0311` (the parser reports it,
  QCD-3.9).
- **ENGT-4.4** `W0311` is a warning: it never stops a build or changes the emitted progs
  (QCD-7).

### Test vectors

Recorded with `cargo test -p qc-engines` on 2026-10-19.

| Source, target | Diagnostic | Statements |
| --- | --- | --- |
| `float(float tabsize, float defaulttype) hash_createtab = #287;` called, Ironwail | `W0311` "hash_createtab is builtin #287, which Ironwail does not have (FTE_QC_HASHTABLES)" | ENGT-4.1 |
| the same, FTEQW | none | ENGT-4.2 |
| `float(string s) strlen = #114;` called, QuakeSpasm | `W0311` "strlen is builtin #114, which QuakeSpasm does not have (FRIK_FILE, FTE_STRINGS, ZQ_QC_STRINGS)" | ENGT-4.1 |
| `void(string s) bprint = #23;` called, KEX | `W0311` "bprint is builtin #23, which KEX does not have" | ENGT-3.2, ENGT-4.1 |
