# Lexing

**Crates:** `qc-lexer` (L2). **Specs:** QC-1 to QC-8, QCD-2; FTEF-13, FTE-23.

## Context and design philosophy

`qc-lexer` is the first stage of the QuakeC front end (L2 syntax, `layers.toml`). It splits
one file's bytes into tokens with byte spans and reports lexical errors as `qc_diag`
diagnostics. The preprocessor (`qc-preproc`), the parser (`qc-syntax`, which lexes the raw file
itself through `input.rs`), the project graph's code helpers and several xtask corpus checks
all call it, so the tokens it gives are the tokens every stage agrees on.

Two principles show in the code:

- **The lexer follows the compiler, not a grammar.** Vanilla behaviour is written from
  `docs/spec/language.md` (QC-1 to QC-8), whose vectors were recorded against id's `qcc`
  (lib.rs:13-14). The FTEQCC profile follows FTEQCC's `QCC_PR_Lex` as
  `docs/spec/dialect-fteqcc.md` records it. The spec files are the authority; this document does
  not restate them.
- **Never fail.** `lex` "never fails: an error is reported and lexing carries on after it"
  (lib.rs:383-384). There is no `Result` in the public API.

## Components

### Public shape

| Item | Role |
|------|------|
| `lex(&[u8]) -> Lexed` | Vanilla lexing; equal to `lex_with(source, Dialect::Vanilla)` (lib.rs:380-387) |
| `lex_with(&[u8], Dialect) -> Lexed` | Dialect-aware lexing (lib.rs:389-402) |
| `Lexed {tokens, diagnostics}` | Output; diagnostics are `qc_diag::Diagnostic` |
| `Token {kind, span, line_start}` | One token; `span` is a byte range |
| `TokenKind` | `Name`, `Float`, `Vector`, `String`, `Punct`, `FrameRef`, `Frames`, `FrameCommand` |
| `Punct` (`ALL[27]`, `FTEQCC[25]`) | Punctuators; FTEQCC ones tried first (lib.rs:1083-1094) |
| `Dialect` | `Vanilla`, `Fteqcc`, `Gmqcc`; `has_fteqcc_extensions()` (lib.rs:40-70) |
| `QC_MAX_TOKEN` (2048), `QC_MAX_FRAME_NAME` (15) | id's limits (lib.rs:30-39) |
| `atof`, `LineIndex`, `fteqcc_string_bytes`, `is_whitespace` | Helpers used by higher stages |
| Re-exports `Code`, `Diagnostic`, `Severity`, `Span` | From `qc-diag`, for older call sites (lib.rs:26-28) |

### Vanilla and FTEQCC profiles

One lexer serves both profiles. A runtime check of `dialect.has_fteqcc_extensions()` gates
each FTEQCC path inside the shared routines (`run`, number, vector, name, `$` and punctuation;
lib.rs:534-563, 694, 799, 883, 980). FTEQCC-only paths:

- numbers with hex, octal and suffixes (`fteqcc_number`, lib.rs:707-771) and hex floats
  (lib.rs:430-446, FTE-23.2);
- character constants (lib.rs:827-872, FTEF-13.3, 13.13, 13.14);
- `\` line continuation (lib.rs:583-585, FTEF-13.2);
- raw message text after `#warning`, `#error`, `#message` (lib.rs:883-911, FTEF-14.3);
- `L"` string joins, which look back for a directive line (lib.rs:512-533);
- the FTEQCC `$` commands (lib.rs:977-1032, FTEF-18.13, 18.14, 18.16);
- string prefixes, raw strings, escapes and output encodings (`fte_string.rs`, FTEF-13.7 to
  13.14).

`Dialect::Gmqcc` returns true from `has_fteqcc_extensions`, so it lexes as FTEQCC
(lib.rs:46-47, 67-69).

## Error model

Every lexical problem is a `Diagnostic` with a code from `docs/spec/language-diagnostics.md`:
E0101 to E0105, E0107, E0501 to E0503, W0101. Lexing resumes after each. The text is read up
to the first NUL byte (lib.rs:403-407); the parser keeps the bytes after it as one skipped leaf
(qc-syntax input.rs:134-139).

## Verification

Held by property tests in `src/tests.rs` (README.md:50-57):

- lexing any bytes terminates; token spans are in bounds, increasing and non-overlapping;
- a token's own text lexes to the same token;
- the line of an offset is one more than the LF bytes before it (QC-1.2).

Unit tests in the same file cite the QC, QCD and FTEF statements they pin. `cargo xtask
lex-corpus` lexes the whole corpus and fails on any error, the FTEQCC string and escape rules
are checked against the pinned FTEQCC by `cargo xtask oracles builtins`, and the `parser` and
`qc_preproc` fuzz targets reach the lexer through its callers.

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Keywords | Every word is `TokenKind::Name`; the parser assigns keyword kinds | Reserved keyword tokens | QC-3.2: qcc reserves no name (lib.rs:6-7) |
| Dialect branching | One lexer, runtime `has_fteqcc_extensions()` checks | Separate FTEQCC lexer | ADR-0017: "one implementation with dialect profiles, never parallel compilers" |
| Provenance (vanilla) | Written from the spec, not ported | Port `qcc/pr_lex.c` | [inferred] the spec vectors recorded against qcc are the check (lib.rs:13-14) |
| Provenance (FTEQCC strings) | Written from fteqw `qcc_pr_lex.c`, no port header | Port header per ADR-0017 | [inferred] predates ADR-0017 (fte_string.rs:6-8) |
| Diagnostic types | Re-export `qc_diag` types | Own types | Compatibility for `qc_lexer::{Code, Diagnostic, Span}` call sites (lib.rs:26-27) |
| `Dialect` enum | Own enum, mirrored by `qc_project::Dialect` | One shared enum | Keeps serde out of the lexer (qc-project lib.rs:57-60) |
| GMQCC | Lexed as FTEQCC | Own profile | COMP-3 orders GMQCC last; "treated as Fteqcc for now" (lib.rs:46-47) |
| Very large hex floats | Approximate above 2^53 | Exact big-number conversion | [inferred] documented at lib.rs:428-429 |
| FTEQCC `utf8()` for codes >= 0x80000000 | Writes 7 bytes | Loop forever as FTEQCC does | A documented divergence so lexing terminates (fte_string.rs:390-393) |

## Open questions

### Resolved

None recorded.

### Deferred
1. `fte_string.rs` says fteqw "is not a porting upstream" (fte_string.rs:6-8), which ADR-0017
   overturned. Should it take the port header that `fte_macro.rs` has?
2. README.md:33 says "depends on no other crate"; Cargo.toml:12 depends on `qc-diag`.
3. Cargo.toml:3 and lib.rs:3 call it "the vanilla QuakeC lexer"; it also lexes FTEQCC.
4. QC-8 is declared in both this crate's and qc-preproc's README spec-sections line. Which one
   owns it for readiness attribution?
5. The lexer makes directive-line decisions (`on_directive_line`, lib.rs:512-520; raw message
   text, lib.rs:883-911) that are otherwise the preprocessor's concern. Intended, or a
   placement to revisit in the CONV-17 audit (P2-60, #452)?
6. Two `Dialect` enums (`qc_lexer`, `qc_project`). Fold together once serde can sit behind a
   feature?
7. GMQCC lexing waits on its spec (L-10, #194) and implementation (L-02, #186).
8. The test file repeats the comment `// spec QCD-1.1` on tests.rs:559-560, and the ratchet
   counts QCD-1.1/1.2 (qc-diag's statements) from here.

## References

- Files: crates/qc-lexer/src/{lib.rs, fte_string.rs, atof.rs, lines.rs, tests.rs},
  crates/qc-lexer/README.md, crates/qc-lexer/Cargo.toml
- Depends on: `qc-diag` (diagnostics)
- Used by: qc-preproc, qc-syntax, qc-project, qc-sema, qc-engines, xtask (`lex_corpus`,
  `parse_corpus`, `fteqcc_coverage`, `fteqcc_full_coverage`, `oracles/builtins`), fuzz
