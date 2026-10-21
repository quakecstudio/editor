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
stays an unknown directive (QC-9.9). In an FTEQCC dialect frame macros also carry from one
source file to the next, and `Preprocessor::restart` begins FTEQCC's second autoprototyping
pass, forgetting the `#define`s and keeping the frames (FTE-22.3, FTE-22.4). An FTEQCC dialect
also reads the full dialect's directives and macros (`dialect-fteqcc.md`, FTEF-14).
`fte_directive.rs` ports FTEQCC's directive reader from `fte-team/fteqw`'s `qcc_pr_lex.c`
(`QCC_PR_Precompiler` and the functions it calls: `QCC_PR_SkipToEndOfLine`,
`QCC_PR_FalsePreProcessorIf`, `QCC_FindBestInclude`, `QCC_PR_NewLine`, `QCC_PR_LexWhitespace`,
`QCC_PR_SimpleGetToken`, `QCC_PR_SimpleGetString`, `QCC_PR_LexInteger`) and `qcc_cmdlib.c`
(`QCC_COM_Parse`) under the same policy: it reads a directive as bytes wherever FTEQCC meets one,
at a line of a file or of a macro's expansion, after a use's `(` and `,`, in an `#includelist`
and after `#pragma DONT_COMPILE_THIS_FILE`; matches its name by its first bytes; ends its line
where `QCC_PR_SkipToEndOfLine` ends it, reporting the rest as "unexpected tokens at end of line";
skips a false side as FTEQCC does, with one count of open sides from file to file; finds an
`#include` beside the file being compiled and then in the include directories, the module's
(FTEQCC's `-I`, FTEF-14.49) first; and runs the pragmas that change what is read, `line`, `file`,
`once`, `includedir`, `noref`, `autoproto`, `warning` (F303) and `DONT_COMPILE_THIS_FILE`
(FTEF-14.33 to FTEF-14.47). Under GMQCC an `#include` is tried beside the file that holds it
first (PRJ-5.12). `#if` and `#elif` use
the project graph's evaluator, in `cond`, so both take the same sides (FTEF-14.33, nesting at most
64 levels, FTEF-14.12 and PRJ-5.10). `#pragma noref` and `#pragma autoproto` are recorded for the
semantic model (FTEF-12.6, FTEF-12.10). FTEQCC's frame numbering and its further `$` commands
(FTEF-18.16, FTEF-18.17), its frame macro diagnostics, "Stale macro used", "Duplicate macro
defined", "Name for frame macro NAME is too long" and "$modelname name too long" (FTEF-18.29;
QCD-2.8, QCD-7.25 to QCD-7.27),
variadic macros and `__VA_ARGS__`, `#` and `##` in macro bodies, one unused extra macro
argument, and the built-in constants such as `__NULL__`. A `#` right before the name of a
macro or built-in constant is that expansion with no `#`, as FTEQCC's lexer reads it
(`dialect-fteqcc-rerelease.md`, FTE-23.10). A token `##` or `#` made, or a built-in's value,
carries its text in `PpToken::spelling`; every other token of an expansion keeps its span in
the macro's body, and `PpToken::touches` says whether it touches the token before it as
FTEQCC's text substitution has them, so a renderer can keep that spacing (QC-9.16). Under
FTEQCC a name is a macro use only where FTEQCC's word ends with it (FTEF-14.13).
`fte_macro.rs` ports FTEQCC's macros from `fte-team/fteqw`'s `qcc_pr_lex.c`
(`QCC_PR_PreProcessor_Define`, `QCC_PR_CheckCompConst`, `QCC_PR_ExpandPreProcessorMacro` and the
functions they call) and `qcc_cmdlib.c` (`QCC_COM_Parse2`) under
[ADR-0017](../../docs/adr/0017-port-fteqcc.md)'s clean-port policy. As FTEQCC does, it keeps a
macro's body as the text the `#define` reader stored, finds a use's arguments by scanning the
text after its `(`, writes the expansion as text, each byte with where it came from, and lexes
that text as a chunk of its own, read until reading goes past its last token: `##` pastes, `#`
strings, `__VA_COUNT__`, built-in constants with FTEQCC's lines and `__FILE__` names, the
chunks' recursion and their argument lists (FTEF-14.13 to FTEF-14.32). Under both dialects the
expansions of one file write at most 2^22 tokens (FTEF-14.28, QC-9.18).
`fte_directive_end` gives, for a reader that runs no directive, where an FTEQCC directive's text
ends, as the preprocessor reads on after it, and `lex_after` the tokens from there; the syntax
tree groups its directive lines with them (`fte_line.rs`, FTEF-14.51).

- **Layer:** L2 syntax. It depends on `qc-lexer`.
- **Spec sections:** QC-8 and QC-9 in [`language.md`](../../docs/spec/language.md); FTEF-14 in
  [`dialect-fteqcc.md`](../../docs/spec/dialect-fteqcc.md); the directive codes QCD-8 of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md); the loaders' case rules
  PRJ-7 in [`project-file.md`](../../docs/spec/project-file.md). A readiness run attributes only
  these sections to the crate, and every active statement of each is cited by a test here.
- **Spec statements:** the `#if` expressions of PRJ-5.5 and PRJ-5.10 (`cond`) in
  [`project-file.md`](../../docs/spec/project-file.md).
- **Also cited here, in sections another crate completes:** QC-27.5 (the `,`/`;` and
  frame-count divergence), QCD-2.6 and QCD-6.4 (the unknown-frame and frame-limit codes
  `qc-lexer` defines), QCD-2.8 and QCD-7.25 to QCD-7.27 (FTEQCC's frame macro diagnostics,
  FTEF-18.29), QCD-7.31 (the W0109 form of F303, FTEF-14.46), and QCD-7.2 (the W0102 "macro
  redefined" warning). QCD-7 is otherwise the semantic model's resolution warnings, so it is
  not declared above.
- **Mutation:** two survivors on the case-fold comparison are equivalent to the original on
  every realisable input and recorded under `[crates.qc-preproc].equivalent_mutants` in
  `ratchets.toml`: the trait's default `stored_name` (its body already returns `None`), and the
  `<` guard that suppresses a lexical diagnostic inside a skipped conditional block, whose end
  is always a `#` directive or end of file where no diagnostic can start.
- **Shipped:** yes.

## Invariants

Each has a property test in `src/tests/mod.rs`:

- Text with no directives and no `$` comes out as the lexer's tokens, each its own origin.
- Every expanded token's span holds its text, and its origin is the macro use in the
  compiled file (QC-9.7).

`cargo xtask lex-corpus` checks id's QuakeC 1.01 in CI: every `.qc` file lexes and the
`progs.src` files preprocess with no errors.
