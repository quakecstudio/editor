# Semantics

**Crates:** `qc-sema` (L3). **Specs:** QC-10, QC-12, QC-23, QCD-1.4; FTE-22, FTEF-15, FTEF-17.

## Context and design philosophy

`qc-sema` (L3) is the semantic model behind go-to-definition, find-references and the
resolution diagnostics (GOAL-1, LANG-4). It reads the trees of `qc-syntax` and the file graph
of `qc-project`, runs `qc-preproc` itself for the compiler's view, and gives a `ModuleIndex`
of definitions, references and diagnostics. `qc-engines` and xtask's corpus and oracle checks
consume it; the LSP server and editor (P2-14, P2-18) will.

Principles visible in the code and README:

- **Incremental in the style of salsa, without salsa.** A hand-written query database with
  revisions and memos (db.rs:26-27), single-threaded through `Rc` trees (db.rs:107-108). "It
  is small, and the project builds its foundations itself" (README.md:14-17).
- **Resolution as id's `qcc` does it**, not as other tools describe it (README.md:45-53): two
  scopes, one definition list in declaration order, a local cannot shadow an earlier global
  (QC-12.3), names resolve at their position in compilation order (QC-12.5, QC-23.3).
- **FTEQCC's rules under the FTEQCC profile** (FTE-22, FTEF-17): locals shadow globals,
  classes and members, scoped names, builtin reuse (FTE-23.7), autoprototyping.
- **Never fail on bad syntax** (QCD-1.4): lowering walks whatever the parser built.
- **Every location in a source file.** Whatever the expansion, a definition, reference or
  diagnostic points at bytes of a real file (QC-9.7, QC-9.2).

## Components

### Query database (`db.rs`)

Inputs: `set_file_text(file, bytes)` and `set_file(file, path, bytes)`; `set_dialect` (clears
every cache and restamps every file, db.rs:151-166); `set_autoproto` (clears only the module
and analysis memos, db.rs:171-177). Every `set_file_text` advances the clock, even for equal
bytes, by design (db.rs:184-201).

Queries:

| Query | Memo | Recomputes when |
|-------|------|-----------------|
| `parse(file)` | per file | that file's revision advances |
| `lower(file)` | per file | its parse changes |
| `module_index(files)` | one (file list, index) | the file list differs or any listed file changed |
| `analyze_module(input)` | one (input, analysis) | the input differs or any file with a path changed (db.rs:337-342) |
| expanded parse/lower | per file, on rendered text | the rendered expansion text differs (early cutoff, db.rs:407-424) |

`QueryCounts` (parse, lower, module_index, preprocess) saturate and make an edit's effect
observable; the incremental tests assert on them.

### Expansion view (`expand.rs`)

`ModuleInput::from_graph` splits a `FileGraph` into prelude (source-form root) and compiled
files, plus the graph's autoproto flag (expand.rs:50-64). `preprocess` runs qc-preproc over
the files in order, twice with `restart` when autoprototyping is in force, appending the
late-pragma W0108 once (expand.rs:165-185). An unreadable listed file is E0402 at 0..0
(expand.rs:214-227, QCD-5.2, QC-23.5). `render` writes each compiled file's tokens as text,
with a newline where the origin line changes, a leading space for indented tokens (column-0
recovery), frame references as numbers, and FTEQCC's touching rule (`adjacent`,
expand.rs:299-358, QC-9.16). `OriginMap::locate` maps a rendered span back to a source
location, falling back to the first token's origin when the end lies elsewhere
(expand.rs:80-93).

### Lowering and resolution

`lower_file` (lower.rs) walks the top-level kinds `Declaration`, `EnumDecl`, `TypedefDecl`,
`ClassDecl`, `StructDecl`, `AccessorDecl` (lower.rs:366-411) and records `RawDecl`s (floats as
bit patterns), class members, and uses with their kind (`Value`, `Member`) and member base.
Accessor bodies get synthetic `this`/`value` params and are named `Owner::get_field` /
`set_field` (lower.rs:627-637).

`resolve_module` / `resolve_mapped` (resolve.rs) join files in order. The first global
definition of a name wins; vector components `_x/_y/_z` are declared recursively
(resolve.rs:329-344); autoproto defers every body until all top-level declarations exist
(resolve.rs:128-165); `#pragma noref` state carries across files (resolve.rs:132-152); class
parent walks are bounded at 64 (resolve.rs:216, 663); `spawnfunc_Name` is synthesised per class
(resolve.rs:571-594). Diagnostics: E0205, E0206, E0208, E0211, E0301, W0201, W0206.

### Types (`ty.rs`)

`Type` is `Unknown`, `Simple`, `Field`, `Function`, `Named`, `Pointer`, with `Rc` sharing and
qcc's structural equality (QC-10.6). A `(void)` list with an unnamed void parameter has no
parameters (FTEF-15.3); a missing return type is void. Under FTEQCC `Named`, `Pointer` and
`Unknown`-containing types are compatible with anything (FTEF-17.6). Lowering nested function
types is linear (P2-53; `FROM_AST_CALLS` test counter).

## Error model

Analysis never fails: whatever the parser built is lowered and resolved (QCD-1.4), and every
problem is a diagnostic in the `ModuleIndex`, located in a real source file. An unreadable
listed file is E0402 at 0..0 (expand.rs:214-227). There is no `Result` in the query API.

## Verification

- Tests in `crates/qc-sema/src/tests/` for resolution, navigation, types, expansion,
  incremental recomputation (asserting on `QueryCounts`) and each FTEQCC rule.
- `cargo xtask sema-corpus` compares each corpus module's index with its golden file in
  `docs/spec/vectors/sema/`, and with the definitions in its progs.dat where one is built.
- `cargo xtask fteqcc-profile` analyses every FTEQCC module without error.
- `crates/qs-tests/tests/it/diagnostics_unified.rs` and the fuzz target `qc_sema`.

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Incremental engine | Hand-written query db | `salsa` crate | Small, and the project builds its foundations itself (README.md:14-17) |
| Threading | Single-threaded, `Rc` | `Arc`, parallel queries | [inferred] follows qc-syntax's `Rc` red tree (db.rs:107-108) |
| Compiler view | Preprocess, render to text, re-parse, map back | Parse token streams directly | [inferred] reuses the one parser; spacing kept by QC-9.16 rendering (P2-74) |
| Editing view | `Db::parse` stays the file as written | Only the expanded view | The editor needs the lossless tree (README.md:27-28) |
| Memo granularity | One module memo, one analysis memo | Per-module memo map | [inferred] one module at a time in P2 |
| Same-bytes edit | Still a change | Byte comparison inside `Db` | Callers compare if they care (db.rs:184-190) |
| Resolution model | qcc's two scopes, first definition wins | Block scopes as in C | QC-12.1 to QC-12.5 |
| FTEQCC opaque types | Compatible with anything | Full FTEQCC type rules | FTEF-17.6; expression typing is not built yet (README.md:97-104) |
| Undefined `noref` functions | Reported when referred to only, `noref` skip not followed | Follow FTEQCC's skip | [inferred] noted as "not followed" (resolve.rs:995-997) |

## Open questions

### Resolved

None recorded.

### Deferred
1. Single `module` and `analysis` memos (db.rs:124, 130): how will several modules (SSQC,
   CSQC, MenuQC, L-09) share a `Db`?
2. `analyze_module` freshness covers only files with paths (db.rs:337-342). Intended?
3. `lookup_local` is linear per lookup (resolve.rs:957-965); quadratic for one function's
   locals (docs/readiness/p2-r3.md, section 7). Budget is P2-18's (#74).
4. FTEQCC expression typing, member access on non-name bases, `enumflags` and constant values
   (README.md:97-104). Where do these land: qc-sema or the compiler (P2-06, P2-07)?
5. Manifest defines do not reach the preprocessor here (expand.rs:50-64; see [preprocessing](preprocessing.md), Open questions).
6. FTEQCC's skip of unreferenced `noref` functions is not followed (resolve.rs:995-997).
7. `is_member_name` treats `x`, `y`, `z`, `length` as non-definitions (resolve.rs:870-872);
   `is_intrinsic` hard-codes FTEQCC intrinsic names (resolve.rs:239-256). Source of truth?
8. Class access labels are detected by `ends_with("virtual")`/`"nonvirtual"` on the first word
   (lower.rs:495-499); under FTEQCC a non-literal initialiser is compared by token text
   joined with spaces (lower.rs:215-217, 437-446).
9. `qc_engines::check::FileDiagnostic` duplicates `FileDiagnostic` (CONV-17 audit, P2-60).
10. README lists more QCD statements than lib.rs:52-53; lib.rs:46 passes 100 columns.
11. Tests: string factories rather than spec structs (CONV-11); `incremental.rs` uncited with
    unchecked subtraction; `types.rs::malformed_type_is_unknown` asserts on either shape;
    `expansion.rs::the_files_as_written_do_not_resolve` asserts a disjunction
    (expansion.rs:88-95).
12. `sema-corpus` and `fteqcc-profile` pick the dialect and autoprototyping by directory name
    (xtask/src/sema_corpus.rs:98-103; xtask/src/fteqcc_profile.rs:78-81).
13. Rename (P2-14) and CSQC/MenuQC symbol tables (L-03, L-09) are not built.

## References

- Files: crates/qc-sema/src/{lib.rs, db.rs, expand.rs, lower.rs, resolve.rs, symbol.rs, ty.rs,
  nav.rs, tests/*}, README.md, Cargo.toml
- Depends on: qc-syntax, qc-lexer, qc-preproc, qc-project (L2); qc-diag (L0)
- Used by: qc-engines, qs-tests, xtask (`sema_corpus`, `fteqcc_profile`, `oracles/builtins`),
  fuzz
