# qc-preproc

The QuakeC preprocessor. It turns each source file of a `progs.src` into the tokens the parser
reads: `#include`, `#define` (object-like and function-like), `#undef`, `#ifdef`, `#ifndef`,
`#else` and `#endif`, macro expansion with rescanning, and frame macro references resolved
against each file's `$frame` list. Every expanded token records the macro use it came from
and where its text is in the definition, so a diagnostic can point at either; `origin` gives
the place in the compiled file.

id's `qcc` has no preprocessor. The directives are this project's own definition, named after
FTEQCC's; for vanilla code, where no line starts with `#` and a name, the only change is frame
macro resolution. FTEQCC's pragmas come with the dialect profiles (P2-03).

- **Layer:** L2 syntax. It depends on `qc-lexer`.
- **Spec sections:** QC-9 and the frame macro rules of QC-8 in
  [`language.md`](../../docs/spec/language.md), with QC-27.5; the codes in QCD-2.6, QCD-6.4,
  QCD-7.2 and QCD-8 of [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md);
  the loaders' case rules of PRJ-7.1 and PRJ-7.2 in
  [`project-file.md`](../../docs/spec/project-file.md).
- **Shipped:** yes.

## Invariants

Each has a property test in `src/tests.rs`:

- Text with no directives and no `$` comes out as the lexer's tokens, each its own origin.
- Every expanded token's span holds its text, and its origin is the macro use in the
  compiled file (QC-9.7).

`cargo xtask lex-corpus` checks id's QuakeC 1.01 in CI: every `.qc` file lexes and the
`progs.src` files preprocess with no errors.
