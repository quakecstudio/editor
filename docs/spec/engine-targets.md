# Engine targets

Prefix: `ENGT` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

The engine-target database (LANG-6, P2-04): which builtins and extensions each Quake engine
a project can target supports, where those facts come from, and the diagnostic for a call to
a builtin the target engine lacks (PRIN-8). Implemented by `qc-engines`, with the generator
`cargo xtask engines` (`xtask/src/engines.rs`).

The five files the tables are generated from are **fact sources, not porting upstreams**:
only facts are taken from them (names, numbers, signatures), never comment text or code, and
none of them is listed in [`../porting.md`](../porting.md#upstreams).
[ADR-0014](../adr/0014-engine-fact-sources.md) records the decision for the first three and
its legal basis, and [ADR-0015](../adr/0015-darkplaces-client-menu-fact-sources.md), which
supersedes it, extends it to DarkPlaces' client and menu QuakeC definitions.

## ENGT-1 Sources

**Source:** `crates/qc-engines/sources.toml`, which pins:

| Source | Repository and commit | File | SHA-256 |
| --- | --- | --- | --- |
| `fteqw` | `fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e` | `quakec/menusys/fteextensions.qc` | `21322f15aca52dae3a75b98a46be0491ed9a1f5d136511228bf52725ac81ccb0` |
| `darkplaces` | `DarkPlacesEngine/darkplaces@5d80f03e26871b273da103f3b750820a3718c01a` | `dpdefs/dpextensions.qc` | `73237e430e8d36009bf0518c0105b15fb37d7710cdc47120c67da8e5e85e33e5` |
| `darkplaces-client` | `DarkPlacesEngine/darkplaces@5d80f03e26871b273da103f3b750820a3718c01a` | `dpdefs/csprogsdefs.qc` | `f9b3562ccc1298cc4794b44fb07071b5458e9919b6e674f911d666e3c54b76ab` |
| `darkplaces-menu` | `DarkPlacesEngine/darkplaces@5d80f03e26871b273da103f3b750820a3718c01a` | `dpdefs/menudefs.qc` | `1bfe701c54ffece1ba6a362920c9f9ec6738e45aae31413e0ab14ff8fa6f96bd` |
| `rerelease` | `id-Software/quake-rerelease-qc@634eefab09a77eb7b5f5ca7078ba3d8784a91142` | `quakec/defs.qc` | `7694c006223ec43f05b923c8fe7691a6ed8298141d1562d98d6c86162de6129d` |

The FTEQW and DarkPlaces commits were the tips of each repository's `master` on 2026-10-19;
the three DarkPlaces files are read at the same commit and share its license file, `COPYING`.
The vanilla builtins are not extracted: they are id's own table, ported from id's engine in
`qc-builtins` (BUILTIN-5).

### Behaviour

- **ENGT-1.1** **Withdrawn** 2026-10-19: it built the database from three files, but
  ADR-0015 added DarkPlaces' `csprogsdefs.qc` and `menudefs.qc`; replaced by ENGT-1.4.
- **ENGT-1.2** Generation is reproducible: `cargo xtask engines` fetches each pinned file and
  writes the tables, and `cargo xtask engines --check` writes nothing and fails when the
  checked-in tables differ from what the sources give. A fetched file or license file whose
  SHA-256 is not the pinned one is an error, and nothing is written.
- **ENGT-1.3** The `rerelease` pin is the corpus manifest's `quake-rerelease-qc` pin
  (`corpus/manifest.toml`): the same commit and the same SHA-256 for `quakec/defs.qc` and
  `COPYING.txt`. A difference is an error.
- **ENGT-1.4** The database is built from the five files above, each pinned by repository,
  commit and SHA-256 together with its repository's license file, and from id's vanilla table
  of `qc-builtins` (BUILTIN-5). The generated `crates/qc-engines/src/tables.rs` records every
  pin, so the checked-in tables carry their source checksums.

## ENGT-2 Extraction

**Source:** this project. The rules read the files of ENGT-1 as they are at the pinned
commits.

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
- **ENGT-2.8** **Withdrawn** 2026-10-19: it read every source for server QuakeC, but the
  DarkPlaces client and menu sources of ADR-0015 give only a client or a menu table; replaced
  by ENGT-2.10.
- **ENGT-2.9** A statement ends at a `;` outside a string literal, or at a `}` outside
  parentheses that closes the statement's outermost `{`, so a function body with no `;` after
  it ends at its closing brace; a `{` or `}` inside parentheses, as in FTEQW's
  `enumflags:float{...}` parameter types, ends nothing. A comment line is read for an
  extension name (ENGT-2.6) only outside a statement: when no code of an unfinished statement
  precedes it and no brace or parenthesis is open.
- **ENGT-2.10** Each source is read once per module kind (PRJ-3.1) it gives a table for, each
  reading by ENGT-2.1 to ENGT-2.7 and ENGT-2.9 with its own predefined macros, giving its own
  builtins and extension names. `fteqw` is read for server QuakeC (`ssqc`) with the macros of
  ENGT-2.3, for client QuakeC (`csqc`) with `CSQC` alone defined and for menu QuakeC
  (`menuqc`) with `MENU` alone defined, the macros its own `#if` lines test for those kinds;
  `darkplaces` and `rerelease` are read for server QuakeC, `darkplaces-client` for client
  QuakeC and `darkplaces-menu` for menu QuakeC, each with no macro defined. `sources.toml`
  lists each source's readings as `tables`; a source with no table, a table of any other
  kind, or two tables of one kind is an error, and nothing is written.

## ENGT-3 Engines

**Source:** the tables of ENGT-1 and ENGT-2, and for Ironwail and vkQuake their READMEs:
`andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd`, `README.md`
(SHA-256 `4fa1c4d577d7c19e7320f0ff04e1d6b139968c7dcaccd6dd729e40ea7d27cb66`), which lists
"ability to play the [2021 release content](https://store.steampowered.com/app/2310/QUAKE/)
with zero setup", and `Novum/vkQuake@27789d7333a46674474d871137bb71e056719628`, `readme.md`
(SHA-256 `f1c0767470d6d7183284cf9914bd03d23d4fe4b193be6f6227f2008b93d17e1d`): "vkQuake has
support for playing the 2021 re-release content." Both read on 2026-10-19. The rerelease
QuakeC calls the builtins its `defs.qc` declares, so an engine that plays it has them.

None of the fact sources describes QuakeSpasm, Ironwail or vkQuake, and none of those
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
- **ENGT-3.5** A project file's target (PRJ-2.9) selects an engine: `quake` is Vanilla,
  `rerelease` KEX, `fte` FTEQW and `dp` DarkPlaces.
- **ENGT-3.6** The targets `quakespasm`, `ironwail` and `vkquake` (PRJ-2.9) select QuakeSpasm,
  Ironwail and vkQuake, so every engine of ENGT-3.1 is the engine of some target.
- **ENGT-3.7** **Withdrawn** 2026-10-19: it gave only FTEQW client and menu QuakeC tables, but
  ADR-0015 made DarkPlaces' `csprogsdefs.qc` and `menudefs.qc` fact sources; replaced by
  ENGT-3.8.
- **ENGT-3.8** The tables and extension names of ENGT-3.1 to ENGT-3.3 are those for server
  QuakeC. FTEQW also has a client QuakeC table, the `fteqw` builtins of its `csqc` reading
  (ENGT-2.10), and a menu QuakeC table, those of its `menuqc` reading. DarkPlaces also has a
  client QuakeC table, the `darkplaces-client` builtins, and a menu QuakeC table, the
  `darkplaces-menu` builtins, with no layer of id's table under either, since each file
  declares the builtins of its kind from `#1`. Each table has its reading's extension names.
  No other engine has a client or menu QuakeC table, since no fact source gives one. A
  builtin number can mean a different builtin in each kind: in `fteqw`, `#5` is
  `lightstylestatic` for server and `bprint` for menu QuakeC, and in DarkPlaces `#1` is
  `makevectors` for client and `checkextension` for menu QuakeC.

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
- **ENGT-4.3** **Withdrawn** 2026-10-19: it read every builtin number as a decimal literal,
  but under an FTEQCC dialect FTEQCC reads `#0x10` as 16 and `#010` as 8; replaced by
  ENGT-4.10 and ENGT-4.11.
- **ENGT-4.4** `W0311` is a warning: it never stops a build or changes the emitted progs
  (QCD-7).
- **ENGT-4.5** Under an FTEQCC dialect a builtin may be declared `= #0:NAME` (FTE-17.4), which
  the engine looks up by `NAME` (ENGT-2.4). A global function whose name has no number by
  ENGT-4.10 takes as its key the `NAME` of its first such initialiser in compilation order,
  `0` being any literal whose value is 0 (ENGT-4.9). A call of it, as in ENGT-4.1, whose engine
  table has no builtin keyed `#0:NAME`, names compared exactly, is `W0311` with the message
  `FUNC is builtin #0:NAME, which ENGINE does not have`, `FUNC` being the function's name,
  and the extensions of the first engine, in the order of ENGT-3.1, whose table has `NAME`
  with some. ENGT-4.2 holds for these keys as for numbers.
- **ENGT-4.6** An initialiser `#N:NAME` with `N` at least 1 is keyed by its number alone, as
  in ENGT-4.10, and its name is not read. FTE-17.4 says FTEQCC accepts the form and is silent
  on which key an engine uses for it; keying by number is this project's choice, since a
  number is what a progs carries for every builtin with one.
- **ENGT-4.7** Under the vanilla dialect, whose `qcc` takes a builtin number greater than 0
  and nothing after it (QC-13.3), `#0:NAME` gives no key, so a call of such a declaration is
  no `W0311`; the parser reports the declaration instead.
- **ENGT-4.8** A module is checked against its target engine's table for the module's kind
  (PRJ-3.1, ENGT-3.8), and the extensions of ENGT-4.1 and ENGT-4.5 come from the engines'
  tables for that kind. A module of a kind its engine has no table for is not checked and
  has no `W0311`.
- **ENGT-4.9** The `0` of ENGT-4.5 is any number literal that FTEQCC reads as the value 0, in
  any of its number forms (FTEF-3.1): `0`, `00`, `.0`, `0.00`, the hex `0x0` and the
  integer `0i`. FTEQCC's `QCC_PR_ParseInitializerType` (`qcc_pr_comp.c`,
  fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`) keys a builtin by the value
  `QCC_PR_LexNumber` gives, not by its spelling: `if (pr_token_type == tt_immediate &&
  pr_immediate_type == type_float && pr_immediate._float == (int)pr_immediate._float) binum =
  (int)pr_immediate._float; else if (pr_token_type == tt_immediate && pr_immediate_type ==
  type_integer) binum = pr_immediate._int;`, then `if (!*fname && QCC_PR_CheckToken (":"))`
  reads the name; and `QCC_PR_LexNumber` (`qcc_pr_lex.c`) reads `0x` as base 16
  (`if (pr_file_p[0] == '0' && (pr_file_p[1] == 'x' || pr_file_p[1] == 'X')) { ... base =
  16;`), so `#0x0:NAME` is `binum` 0 with the name `NAME`, the same as `#0:NAME`. A literal
  whose value is not 0, such as `0.5` or `0x1`, is no `0` here.
- **ENGT-4.10** A builtin's number is that of the first `#N` initialiser of its name, in
  compilation order (QC-11.5), `N` being the value of its number literal: under the vanilla
  dialect a decimal literal of at least 1, with an optional fraction of zeros, and under an
  FTEQCC dialect the value of ENGT-4.11. A literal that gives no number by these rules is no
  number here and gives no `W0311` (the parser reports it, QCD-3.9).
- **ENGT-4.11** Under an FTEQCC dialect a builtin number is the value FTEQCC reads from the
  literal, in any of its number forms (FTEF-3.1), when that value is a whole number of at least
  1: `#16`, `#16.0`, the hex `#0x10` and the integer `#16i` are all 16, and `#010`, which
  FTEQCC reads in base 8, is 8, where the vanilla dialect reads `#010` as 10. FTEQCC's
  `QCC_PR_ParseInitializerType` (`qcc_pr_comp.c`,
  fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`) takes `binum` from that value, as
  ENGT-4.9 quotes, and `QCC_PR_LexNumber` (`qcc_pr_lex.c`) reads a leading `0` in base 8: `else
  if (pr_file_p[0] == '0') { pr_file_p++; if (*pr_file_p >= '0' && *pr_file_p <= '9')
  QCC_PR_ParseWarning(WARN_OCTAL_IMMEDIATE, "A leading 0 is interpreted as base-8."); base =
  8;`. A value that is not whole, such as `1.5`, is FTEQCC's "Bad builtin immediate" and no
  number here.

### Test vectors

Recorded with `cargo test -p qc-engines` on 2026-10-19.

| Source, target | Diagnostic | Statements |
| --- | --- | --- |
| `float(float tabsize, float defaulttype) hash_createtab = #287;` called, Ironwail | `W0311` "hash_createtab is builtin #287, which Ironwail does not have (FTE_QC_HASHTABLES)" | ENGT-4.1 |
| the same, FTEQW | none | ENGT-4.2 |
| `float(string s) strlen = #114;` called, QuakeSpasm | `W0311` "strlen is builtin #114, which QuakeSpasm does not have (FRIK_FILE, FTE_STRINGS, ZQ_QC_STRINGS)" | ENGT-4.1 |
| `void(string s) bprint = #23;` called, KEX | `W0311` "bprint is builtin #23, which KEX does not have" | ENGT-3.2, ENGT-4.1 |
| `void(string s, ...) bprint = #0:ex_bprint;` called, FTEQCC dialect, FTEQW | `W0311` "bprint is builtin #0:ex_bprint, which FTEQW does not have" | ENGT-4.5 |
| the same, KEX, Ironwail or vkQuake | none | ENGT-4.5 |
| `stuffcmdflags = #0:stuffcmdflags` called, FTEQCC dialect, KEX | `W0311` "stuffcmdflags is builtin #0:stuffcmdflags, which KEX does not have (FTE_QC_STUFFCMDFLAGS)" | ENGT-4.5 |
| `void() f = #.0:nothing;` called, FTEQCC dialect, KEX (and the same with `#00`, `#0.00`, `#0x0`, `#0X00` or `#0i`) | `W0311` "f is builtin #0:nothing, which KEX does not have" | ENGT-4.5, ENGT-4.9 |
| `void() f = #0.5:nothing;` or `#0x1:nothing` called, FTEQCC dialect, KEX | none: neither is zero | ENGT-4.9 |
| `float(float tabsize, float defaulttype) hash_createtab = #0x11F;` called, FTEQCC dialect, Ironwail | `W0311` "hash_createtab is builtin #287, which Ironwail does not have (FTE_QC_HASHTABLES)" | ENGT-4.10, ENGT-4.11 |
| `void() f = #0101;` called, FTEQCC dialect, Vanilla | `W0311` "f is builtin #65, which Vanilla does not have (DP_QC_ETOS)" | ENGT-4.11 |
| the same, vanilla dialect, Vanilla | `W0311` "f is builtin #101, which Vanilla does not have (??MVDSV_BUILTINS)" | ENGT-4.10 |
| `void() clearscene = #300;` called, FTEQW, client module | none | ENGT-4.8 |
| the same, FTEQW, server module | `W0311` "clearscene is builtin #300, which FTEQW does not have" | ENGT-4.8 |
| `void(entity e) setspawnparms = #78;` called, FTEQW, client module | `W0311` "setspawnparms is builtin #78, which FTEQW does not have" | ENGT-4.8 |
| `float(float tabsize, float defaulttype) hash_createtab = #287;` called, Ironwail, client module | none: Ironwail has no client table | ENGT-4.8 |
| the same, DarkPlaces, client module | `W0311` "hash_createtab is builtin #287, which DarkPlaces does not have (FTE_QC_HASHTABLES)" | ENGT-3.8, ENGT-4.8 |
| `void() clearscene = #300;` called, DarkPlaces, client module | none | ENGT-3.8, ENGT-4.8 |
| `void(entity e) setspawnparms = #78;` called, DarkPlaces, client module | `W0311` "setspawnparms is builtin #78, which DarkPlaces does not have" | ENGT-3.8, ENGT-4.8 |
| `float(float max_themes) initparticlespawner = #522;` called, FTEQW, client module | `W0311` "initparticlespawner is builtin #522, which FTEQW does not have (DP_CSQC_SPAWNPARTICLE)" | ENGT-4.8 |
| `vector() getmousepos = #66;` called, DarkPlaces, menu module | none | ENGT-3.8, ENGT-4.8 |
| `string(float ascii) chr = #78;` called, DarkPlaces, menu module | none | ENGT-3.8, ENGT-4.8 |
| the same, FTEQW, menu module | `W0311` "chr is builtin #78, which FTEQW does not have" | ENGT-4.8 |
