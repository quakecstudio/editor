# Syntax

**Crates:** `qc-syntax` (L2). **Specs:** QC-10 to QC-27, QCD-3; FTE-1 to FTE-21, FTEF-1 to FTEF-18.

## Context and design philosophy

`qc-syntax` (L2) parses one QuakeC file as written into a lossless concrete syntax tree. The
semantic model, the engine-target check, and later the LSP server and the editor (P2-14,
P2-18) all read this tree. The design follows rust-analyzer: a green/red tree in the style of
`rowan`, hand-written recursive descent, and recovery that keeps an error local (LANG-2).

Principles visible in the code:

- **Lossless and total.** Every byte is in exactly one token, trivia included, so the tree
  gives back the file unchanged; parsing never fails (lib.rs:4-9).
- **The file as written.** Macros are not expanded and both sides of a conditional are parsed;
  directive lines and `$frame` lines are trivia (lib.rs:13-15). The semantic model parses
  expansions separately.
- **Grammar from the compiler.** The vanilla grammar is id's `qcc` (`PR_ParseDefs` and
  others, parser/mod.rs:5-10), written from `docs/spec/language.md`, not ported. The FTEQCC
  profile decides by token shape what FTEQCC decides by type, "so no program FTEQCC accepts is
  read another way" (fte.rs:4-11), because names are not known at parse time (README.md:47-49).
- **One parser, two profiles** (ADR-0017). Vanilla reports each FTEQCC keyword construct once
  as E0510 and keeps vanilla diagnostics for operator and declaration extensions, so the
  profiles are observably different (lib.rs:88-95).

## Components

### Pipeline

1. `input.rs` lexes the raw file with `lex_with`, classifies the bytes between tokens as
   trivia (comments, `$` lines, unknown bytes, a skipped leaf after a NUL), groups directive
   lines from a line-start `#name` to the next LF (input.rs:72-86, 189-205), and computes
   `column0` and `line_start` for recovery.
2. `parser/mod.rs` walks the input with precedence climbing at qcc's priorities
   (mod.rs:50-64, 126-137; FTEQCC's ternary at the assignment priority, FTE-8.4), building
   through `tree::Builder` with checkpoints.
3. `parser/fte.rs` handles the FTEQCC declarations, types (C type words, pointers, `..T`,
   function-returning-function), classes, accessors, struct variables, casts by look-ahead,
   `[x, y, z]`, `sizeof`, labels and `goto`.
4. `Parse::finish` yields the green tree and diagnostics in source order.

### Tree and typed view

`tree.rs` holds `GreenNode`/`GreenToken` (Arc-shared, token text as bytes, QC-1.1) and
`SyntaxNode`/`SyntaxToken` (an Rc cursor with parent and offset). Node equality is green
pointer plus offset (tree.rs:434-447). `descendants_with_tokens` is iterative; `write_tree`
is recursive (tree.rs:356-366, 399-431).

`ast.rs` wraps nodes with `ast_node!`/`ast_enum!` (ast.rs:108-311). Every accessor returns an
`Option` or an iterator. Positional accessors split around a token (`around()`, ast.rs:86-106).
Scoped names take the last name after `::` (ast.rs:459-490, FTEF-9.4, FTEF-16.4).

### Shared helpers

- `builtin_binum(text, Dialect) -> Option<i32>` (binum.rs) re-lexes a literal and gives the
  builtin number a compiler binds: FTEQCC `i` keeps 32 bits, `u`/`l`/`d` bind nothing
  (FTE-17.6, FTE-23.3); out-of-range floats follow x86 `(int)` (VM-9.2). Used by the parser,
  qc-sema and qc-engines.
- `Builtin::engine_name` (ast.rs:676-709) decodes FTEQCC strings through
  `qc_lexer::fteqcc_string_bytes`, joins adjacent strings, truncates at NUL and caps at 255
  bytes (FTE-23.5, FTE-23.6).
- `recovery::check_deletions[_with]` (recovery.rs) deletes each token of a correct file and
  checks the declarations around it parse as before; public for `xtask parse-corpus`.

## Error model

Two rules (README.md:66-78, mod.rs:12-18, 504-540): a declaration-shaped line at column 0
ends a function body, and after an error tokens are skipped to `;` at depth 0, the closing
`}`, a statement keyword starting a line, or such a declaration line. Errors are muted until
the next item starts, so only the first error per statement or declaration is reported
(mod.rs:401-411, 493-496; QC-24.2); warnings do not mute. Nesting deeper than 256
(`QC_MAX_NESTING`) is E0509 and the tree stays bounded (QC-27.15, QC-27.16, FTEF-18.26). The
label/`goto` check E0308 runs at function end (mod.rs:1086-1098).

## Verification

- Tests in `crates/qc-syntax/src/tests/` per construct and dialect, with property tests for
  the lossless round trip.
- `cargo xtask parse-corpus` round-trips the corpus losslessly and, with `--deletions`, checks
  local recovery by deleting each token of a correct file (`recovery.rs`).
- `crates/qs-tests/tests/it/deep_nesting.rs`: 100000-deep nesting stays bounded (QC-27.15).
- Fuzz target `parser` (QA-2).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Tree library | Hand-written rowan-style tree | `rowan` crate dependency | [inferred] the project builds its foundations itself (cf. qc-sema README.md:14-17 on salsa); no rowan in Cargo.toml |
| Parse input | Raw file, directives as trivia | Parse preprocessor output | The editor view must be the file as written (lib.rs:13-15) |
| Keywords | Parser assigns keyword kinds | Lexer-reserved keywords | QC-3.2, no name is reserved (kind.rs:9-11) |
| Declaration vs expression | By token shape | By resolved names | Names are unknown at parse time (README.md:47-49; fte.rs:4-11) |
| Error reporting | First error per item | Every error | qcc reports one (QC-24.2) |
| Nesting bound | 256, E0509 | Unbounded recursion | QC-27.15, P2-50: no input exhausts the stack |
| FTEQCC profile | Branches per construct via `self.fteqcc()` | Separate parser | ADR-0017, one implementation with profiles |
| Builtin number helper | In qc-syntax, shared upward | In qc-sema | [inferred] the parser needs it too and qc-syntax is the lower crate (binum.rs:3-5) |
| Deletion harness | Public module of the shipped crate | Test-only crate | [inferred] so `xtask parse-corpus --deletions` can run it on the corpus |
| Red tree | `Rc`, `!Send` | `Arc` red nodes | [inferred] speed of a single-threaded cursor |

## Open questions

### Resolved

None recorded.

### Deferred
1. `SourceFile::declarations` returns only `Declaration` nodes (ast.rs:417-419); typedef,
   enum, class, struct and accessor items are unreachable through it, though
   `recovery::items` counts them (recovery.rs:28-43). qc-sema walks the raw children instead
   (qc-sema lower.rs:366-411).
2. Node kinds without a typed wrapper: `ForClause`, `StructType`, `StructField`,
   `Enumerator`, `Qualifier`, `ArraySuffix`, `AccessLabel`, `Error`.
3. `ForStmt::clauses` loses empty-clause positions (ast.rs:1180-1184);
   `AggregateInit::elements` drops nested aggregates (ast.rs:1334-1340); `StructDecl::fields`
   would include nested struct fields (ast.rs:923-925); `TypedefDecl::name` is the last direct
   name token (ast.rs:1342-1347); `ClassDecl::parent` is the first name after `:`
   (ast.rs:881-888).
4. `BinaryExpr::op` returns `Add` when there is no operator token (FTEQCC `a -1`,
   ast.rs:1101-1119). Should the view expose the implicit operator explicitly?
5. `binum.rs` and `Builtin::engine_name` are semantic logic in the syntax crate; for the
   CONV-17 audit (P2-60, #452). `engine_name` cites fteqw (ast.rs:11-14) with no port header;
   under ADR-0017 should it carry one?
6. Directive trivia ends at LF (input.rs:72-80) while the preprocessor follows FTEQCC `\`
   continuations (see [preprocessing](preprocessing.md), Three directive readers).
7. `input.rs` re-derives comment, `$` and unknown-byte boundaries independently of the lexer
   (input.rs:158-187). A second source of truth for trivia.
8. Docs: "vanilla" (Cargo.toml:3, lib.rs:2); `Directive` "treated as trivia" though not in
   `is_trivia` (kind.rs:199-200, 322-331); `Qualifier` documented as const/var/nosave only
   (kind.rs:278-279).
9. Node start/finish split across functions (fte.rs:824-870, 1057-1069 started, finished in
   mod.rs:2073).
10. The two sample round-trip tests and `ast_view.rs` (9 tests) carry no spec citation
    (tests/mod.rs:177-190).
11. The FTEQCC dialect sections (FTE-1 to 21, FTEF-1 to 18 except 14) are declared by no L2
    README, so the per-crate readiness spec score does not see them (docs/readiness/p2-r2.md,
    section 7); the workspace ratchet holds them.
12. GMQCC parsing waits on L-10 (#194) and L-02 (#186).

## References

- Files: crates/qc-syntax/src/{lib.rs, input.rs, kind.rs, tree.rs, ast.rs, binum.rs,
  recovery.rs, parser/mod.rs, parser/fte.rs, tests/*}, README.md, Cargo.toml
- Depends on: qc-lexer
- Used by: qc-sema, qc-engines, xtask (`parse_corpus`, `fteqcc_coverage`, `fteqcc_profile`,
  `oracles/builtins`), fuzz (`parser`, `qc_sema`)
