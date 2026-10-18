# qc-preproc

The QuakeC preprocessor. It turns each source file of a `progs.src` into the tokens the parser
reads: `#include`, `#define` (object-like and function-like), `#undef`, `#ifdef`, `#ifndef`,
`#else` and `#endif`, macro expansion with rescanning, and frame macro references resolved
against each file's `$frame` list. Every expanded token records the macro use it came from
and where its text is in the definition, so a diagnostic can point at either; `origin` gives
the place in the compiled file.

id's `qcc` has no preprocessor. The directives are this project's own definition, named after
FTEQCC's; for vanilla code, where no line starts with `#` and a name, the only change is frame
macro resolution. `Preprocessor::with_dialect` selects the dialect: in an FTEQCC dialect the
FTEQCC `#pragma` directive is accepted and produces no tokens (`dialect-fteqcc-rerelease.md`,
FTE-16), including the rerelease's `#pragma warning disable/enable`; in the vanilla dialect it
stays an unknown directive (QC-9.9).

- **Layer:** L2 syntax. It depends on `qc-lexer`.
- **Spec sections:** QC-8 and QC-9 in [`language.md`](../../docs/spec/language.md); the
  directive codes QCD-8 of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md); the loaders' case rules
  PRJ-7 in [`project-file.md`](../../docs/spec/project-file.md). A readiness run attributes only
  these sections to the crate, and every active statement of each is cited by a test here.
- **Also cited here, in sections another crate completes:** QC-27.5 (the `,`/`;` and
  frame-count divergence), QCD-2.6 and QCD-6.4 (the unknown-frame and frame-limit codes
  `qc-lexer` defines), and QCD-7.2 (the W0102 "macro redefined" warning). QCD-7 is otherwise
  the semantic model's resolution warnings, so it is not declared above.
- **Mutation:** two survivors on the case-fold comparison are equivalent to the original on
  every realisable input and recorded under `[crates.qc-preproc].equivalent_mutants` in
  `ratchets.toml`: the trait's default `stored_name` (its body already returns `None`), and the
  `<` guard that suppresses a lexical diagnostic inside a skipped conditional block, whose end
  is always a `#` directive or end of file where no diagnostic can start.
- **Shipped:** yes.

## Invariants

Each has a property test in `src/tests.rs`:

- Text with no directives and no `$` comes out as the lexer's tokens, each its own origin.
- Every expanded token's span holds its text, and its origin is the macro use in the
  compiled file (QC-9.7).

`cargo xtask lex-corpus` checks id's QuakeC 1.01 in CI: every `.qc` file lexes and the
`progs.src` files preprocess with no errors.
