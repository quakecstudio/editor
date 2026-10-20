# Diagnostics

**Crates:** `qc-diag` (L0). **Specs:** QCD-1; the codes of QCD-2 to QCD-8.

## Context and design philosophy

`qc-diag` is the diagnostic vocabulary of the QuakeC front end. It sits in L0 so that every
stage above can depend on it (layers.toml:10). The README states the intent: every stage
"reports the same kind of thing: an error or a warning, with a code from
`docs/spec/language-diagnostics.md`, a message with the names filled in, and the byte span it
is about", so "an as-you-type error in the editor is then the same `Diagnostic` a full build
reports (COMP-2)" (README.md:3-14).

The crate holds data only: "no logic that raises diagnostics: each stage keeps its own rules
and cites the spec statements it reports" (README.md:16-19). It owns QCD-1 (QCD-1.1 code
format, QCD-1.2 no reuse, QCD-1.3 diagnostic shape); the codes of QCD-2 to QCD-8 are named
here and cited by the stages that raise them.

## Components

### Types

| Type | Shape | Notes |
|------|-------|-------|
| `Severity` | `Error`, `Warning` | `E` codes are errors, `W` codes warnings; a warning never changes the emitted progs (QCD-7) |
| `Code` | 65 variants; `ALL: [Self; 65]`, `as_str`, `is_error`, `severity` | Each variant's doc gives the message and its QCD statement (code.rs) |
| `Span` | `{start, end}` byte offsets; `new`, `range`, `contains` | No file identity; `new` does not check `start <= end` |
| `Diagnostic` | `{code, message, span}`, public fields | No constructor, built as a struct literal (lib.rs:63-69); `is_error`, `severity` |

Codes follow QCD-1.1: `E` or `W`, four digits, the first two the group (01 lexical, 02
declarations and types, 03 expressions and statements, 04 project, limits and fatal, 05
compiler-added).

### Who uses it

- `qc-lexer` re-exports `Code`, `Diagnostic`, `Severity`, `Span` (qc-lexer lib.rs:26-28), and
  `qc-syntax` re-exports them from the lexer (qc-syntax lib.rs:28).
- `qc-sema` and `qc-engines` depend on `qc-diag` directly and wrap `Diagnostic` with a file in
  two separate `FileDiagnostic` structs (qc-sema resolve.rs:34-40, qc-engines check.rs:44-50).
- `qc-preproc` and `qc-project` use `Code` through `qc_lexer` but define their own diagnostic
  structs: one with a `FileSpan`, one with a file and line.
- `crates/qs-tests/tests/it/diagnostics_unified.rs` checks that the editor path (an
  incremental `Db` after replayed edits) and the build path (fresh parse, lower, resolve)
  give identical diagnostic sets on a small vanilla corpus (QCD-1.3; COMP-2, P2-05). It does
  not go through `analyze_module` or the FTEQCC profile.

## Error model

`qc-diag` raises nothing, so it has no error type: every constructor and accessor is total.
The one unchecked input is `Span::new`, which accepts `start > end` (Open questions).

## Verification

- `crates/qc-diag/src/tests.rs`: every code has the QCD-1.1 shape and a known group, the
  letter decides the severity, no code string is shared (QCD-1.2), and a diagnostic carries
  its code, message and span (QCD-1.3).
- `crates/qs-tests/tests/it/diagnostics_unified.rs`: the editor path and the build path give
  the same diagnostics (COMP-2).
- The stages that raise each code cite its QCD statement in their own tests.

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Layer | L0 | Inside qc-lexer (L2) | Shared by every stage, including L0 to L7 consumers (README.md:21-22) |
| Scope | Vocabulary only, no raising logic | Central diagnostic builder | Each stage keeps its own rules and cites its statements (README.md:16-19) |
| Compatibility | qc-lexer re-exports the types | Move all call sites | Code written against `qc_lexer::{Code, Diagnostic, Span}` keeps compiling (README.md:17-19) |
| Span | Byte range, no file | Span with file id | [inferred] the lexer and parser work one file at a time |
| Construction | Public fields, no constructor | Builder or `new` | [inferred] simplicity; documented at lib.rs:66-67 |
| Code registry | Hand-written enum with four parallel lists | Macro or generated table | [inferred] no generator in the crate (code.rs:25-371) |
| Unassigned codes | Codes not raised yet have no variant | Variants ahead of use | [inferred] tests.rs:53-62 asserts some codes are absent |

## Open questions

### Resolved

None recorded.

### Deferred
1. The one-type intent (lib.rs:63-67) versus three L2 diagnostic structs and two
   `FileDiagnostic`s. Unify with a file-aware wrapper here (CONV-17 audit, P2-60, #452)?
2. tests.rs:53-55 calls E0207 (QCD-3.7) and E0403 to E0405 (QCD-5.3 to 5.5) "withdrawn"; the
   statements are active and baselined (coverage-baseline.tsv:12, 19-21). It also says
   "E0501..=E0509 warnings stay unassigned", but those are live error codes.
3. 22 catalogue codes have no variant (E0207, E0302 to E0307, E0403 to E0405, W0202, W0301 to
   W0310, W0401); they wait on the compiler (P2-06).
4. `ALL` is in declaration order, E040x after E05xx (code.rs:111-120, 203-209); the doc says
   "every code, in order".
5. `Span::new` accepts `start > end`.
6. FTEQCC messages that reuse E0103 and E0209 are not in the catalogue entries QCD-2.3 and
   QCD-3.9.
7. The `span_helpers` test carries no spec citation (tests.rs).

## References

- Files: crates/qc-diag/src/{lib.rs, code.rs, tests.rs}, README.md, Cargo.toml
- Depends on: nothing
- Used by: qc-lexer (re-export), qc-syntax (via qc-lexer), qc-sema, qc-engines; codes only by
  qc-preproc and qc-project
