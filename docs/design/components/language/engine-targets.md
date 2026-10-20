# Engine targets

**Crates:** `qc-engines` (L3); `cargo xtask engines`. **Specs:** ENGT-1 to ENGT-4; QCD-7.17.

## Context and design philosophy

A QuakeC builtin is declared `= #N`, and the number is the only link between a progs and the
engine; engines differ in which numbers they implement (qc-engines README.md:8-11). The
engine-target database answers "does the target engine have this builtin for this module
kind?" and backs the `W0311` warning (QCD-7.17), serving LANG-6 ("diagnostics for unsupported
builtins") and PRIN-8 ("warn ... when an engine lacks a builtin").

Two parts:

- **Generator** (tooling): `cargo xtask engines` reads pinned engine sources and writes
  `crates/qc-engines/src/tables.rs`; `--check` fails CI if the file is stale. It owns ENGT-2.
- **Database and check** (L3, `qc-engines`): the `Engine` enum, the layered tables, and
  `missing_builtins`. It owns ENGT-1, ENGT-3, ENGT-4 (README.md:15-18).

The governing rule is **facts only**: the QuakeC definition files (ADR-0014, superseded by
ADR-0015) and the QuakeSpasm-lineage C tables (ADR-0016) are read for builtin numbers, names,
signatures and extension names, never copied as text or code. ADR-0017 makes FTEQW's compiler
a porting upstream but leaves these fact-only ADRs unchanged (ADR-0017:72-73). Whether
QuakeSpasm, Ironwail and vkQuake become porting upstreams is a separate open decision (P3-30,
#448).

## Components

### Sources and generation

`sources.toml` pins ten sources by repository, commit, path and SHA-256, plus each repository's
license file and its SHA-256: fteqw, darkplaces, darkplaces-client, darkplaces-menu,
rerelease, quakespasm, quakespasm-rerelease, ironwail, vkquake, vkquake-ext (pins recorded
2026-10-19, sources.toml:13). The rerelease pin must match `corpus/manifest.toml`
(engines.rs:290-310).

The generator (engines.rs) fetches each pin and its license file through `Net`, checks both
digests, then reads tables per module kind:

- **QuakeC sources** (ENGT-2.1 to 2.10): a small conditional reader (`#ifdef`, `#ifndef`, `#if`;
  `#elif` unsupported, engines.rs:657-691) with `CSQC`/`MENU` defines per kind, a statement
  splitter, and `name_and_signature` for `#N` and `#0:NAME` builtins and extension names.
- **C sources** (c.rs, ENGT-2.11 to 2.17): a C lexer and macro expander (depth 32, self
  reference blocked, c.rs:17, 285-338) reading array initialisers positionally, by row column,
  or as name patches against an earlier pin's facts (`target_of`, engines.rs:186-192).
  Conditional macros are recorded and an error only when used; `#if` is never evaluated
  (c.rs:199-242, 303-307). Duplicate keys keep the first (c.rs:689-693).

`render` writes `SOURCES` and `<ID>_<KIND>_BUILTINS` / `_EXTENSIONS` constants with a header
citing ADR-0015 and ADR-0016 (engines.rs:838-843), and formats them with `rustfmt`.

### Engines and layers

`Engine::layers(kind)` (lib.rs:283-300) stacks tables; the first layer with a key gives the
entry (ENGT-3.9, 3.11). The vanilla layer comes from `qc-builtins`' table at runtime
(lib.rs:140-147, 175). Examples: QuakeSpasm SSQC is Vanilla, QuakeSpasm, QuakeSpasm-rerelease;
vkQuake SSQC puts its extension table before Vanilla (lib.rs:293-295). FTEQW has SSQC, CSQC and
MenuQC tables; DarkPlaces has all three from its own files; Ironwail and vkQuake have CSQC
(ENGT-2.10, ENGT-3.11). `from_target` maps every `qc_project::Target` (lib.rs:269-279, ENGT-3.5,
3.6).

### The missing-builtin warning

`missing_builtins(engine, kind, parses, index, dialect)` (check.rs:52-100):

1. Returns nothing when the engine has no table for `kind` (ENGT-4.8).
2. Builds keys by definition name from top-level builtin initialisers of the as-written
   parses: a number where the dialect's compiler binds one (`builtin_number`, `(int)` of the
   float value, FTEQCC's hex, octal and `i` literals, `d` giving none; ENGT-4.11 to 4.13,
   4.15), otherwise an engine name (`#0:NAME`, `#"NAME"`, `:NAME`, adjacent strings joined;
   ENGT-4.14, 4.16). Number keys win (check.rs:155-161).
3. For each resolved reference at a call's callee span to a function with a key, warns when
   the engine's table lacks it, naming extensions from other engines that have it
   (check.rs:115-119).
4. Sorts by file and span (check.rs:99).

## Error model

- `missing_builtins` returns diagnostics only (`W0311`), wrapped with their file in a
  `FileDiagnostic`; an engine with no table for the module kind gives none (ENGT-4.8).
- The generator returns `Err` with a message for a failed download, a digest mismatch, a
  rerelease pin that differs from `corpus/manifest.toml`, an unsupported conditional or a C
  macro it cannot expand where it is used.

## Verification

- `crates/qc-engines/src/tests.rs` pins the tables and the warning against ENGT statements.
- `cargo xtask engines --check` runs in CI and fails when `tables.rs` is stale; the
  generator's own tests are `xtask/src/engines/tests.rs` and `xtask/src/engines/c/tests.rs`.
- Fuzz target `qc_sema` exercises the check. No corpus differential reaches it (Open
  questions).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Fact sources | Engines' own QuakeC and C files, facts only | Hand-written tables; copying the files | ADR-0014/0015/0016: facts carry no copyright, pinned by commit and SHA-256 |
| Tables | Generated `tables.rs`, checked in, `--check` in CI | Generate at build time | [inferred] no network at build; reviewable diffs (README.md:50-55) |
| Vanilla table | From `qc-builtins` at runtime | Generate it too | qc-builtins is ported from id's engine (README.md:48-50) |
| Engine tables | Layer stacks per kind, first layer wins | One flat table per engine | ENGT-3.9, ENGT-3.11 |
| Keys | Number, or engine name when no number | Number only | FTEQCC `#0:NAME` builtins (ENGT-4.5, 4.14); ENGT-4.6 records a project choice where FTE-17.4 is silent |
| Check input | As-written parses plus an index resolved from them | `Db::analyze_module` result | [inferred] keeps qc-engines independent of the expansion path; not stated |
| Generator C reader | Own small C lexer and expander | A C preprocessor dependency | [inferred] facts-only reading of array initialisers needs no full preprocessor |
| QuakeSpasm/Ironwail signatures | Empty, C names | Guess signatures | The C sources give none (README.md:83-86, ENGT-2.14) |

## Open questions

### Resolved

None recorded.

### Deferred
1. `missing_builtins` reads as-written parses (check.rs:130-154, 191-208), so macro-made
   declarations and calls and both sides of conditionals are judged as written [inferred].
   Should it take the analysis view?
2. No workspace consumer (only fuzz). Which command surfaces `W0311` first (P2-13 CLI, P2-14
   LSP)?
3. ENGT-4.5 and ENGT-4.6 cite withdrawn ENGT-4.10 (engine-targets.md:280, 287, 309). ENGT-4.11
   is active though ENGT-4.13 refines its number rule (overlap, not contradiction).
4. ADR-0015:98 versus ADR-0016:194 on ENGT-3.8; vkQuake pin `27789d73` (ADR-0016) versus
   `47726d89` (ADR-0012:105); ADR-0016:181-183 "supersedes neither".
5. P3-30 (#448): do QuakeSpasm, Ironwail and vkQuake become porting upstreams?
6. `Engine::builtins` allocates and deduplicates per call (lib.rs:324-334); `Layer::builtins`
   clones generated tables into a `Vec` (lib.rs:190). The `W0311` message is right, but
   `message()` and the `Key::Name` lookup rebuild each engine's merged table per warning
   (check.rs:35-38, 115-119).
7. Generator QuakeC reader: `#elif` unsupported (engines.rs:691; ENGT-2.3 lists none), strings cannot span lines
   (engines.rs:545-546), no character literals (engines.rs:324-366), U+FFFD rejected
   (engines.rs:746, 767). The C reader lexes `0x10` as one number token later refused as
   non-decimal (c.rs:151-180, 489-491) and an unmatched `'` as punctuation (c.rs:169-176).
8. `rustfmt` spawned outside `Exec` (engines.rs:906-913); license text discarded after its
   digest check (engines.rs:180); `eprintln!` status (engines.rs:206-213).
9. xtask engines tests hard-code counts from the live `sources.toml` (engines/tests.rs:140,
   278-297, 325-348). A recursive `#define A B` / `#define B A` pair reads as one builtin, not
   an error (c/tests.rs:225-226).
10. The corpus differential reaches none of qc-engines' code (docs/readiness/p2-r3.md,
    section 7); its vectors are recorded by `cargo test`, not against a running engine
    (engine-targets.md:353-354).
11. Some extension names carry odd prefixes, e.g. `(??MVDSV_BUILTINS)` for #101
    (tests.rs:789), from the source data.

## References

- Files: crates/qc-engines/src/{lib.rs, check.rs, tables.rs, tests.rs}, sources.toml,
  README.md, Cargo.toml; xtask/src/engines.rs, xtask/src/engines/c.rs and their tests
- Depends on: qc-sema (L3), qc-syntax, qc-project, qc-lexer (L2), qc-builtins (L1), qc-diag
  (L0); the generator on serde, toml, `xtask::download`, `oracles::sha256`, `rustfmt`
- Used by: fuzz (`qc_sema` target) only
