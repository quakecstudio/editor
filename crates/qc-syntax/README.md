# qc-syntax

The vanilla QuakeC parser. It turns one source file into a lossless concrete syntax tree:
every byte of the file is in exactly one token, comments, whitespace, directive lines,
`$` command lines and bytes the lexer skipped included, so the tree gives back the file byte
for byte. The parser is hand-written recursive descent over the grammar of id's `qcc`, and it
never fails: an error becomes a diagnostic and an error node, and parsing resumes at the next
statement or declaration. The tree has the two layers of rust-analyzer's `rowan` (an
immutable green tree and a red cursor over it, with byte tokens since QuakeC source has no
encoding), and `ast` gives a typed view of it.

The tree is of the file as written: macros are not expanded and both sides of a conditional
are parsed. Directive lines and `$frame` lines are trivia to the parser; frame macro
references are literals, resolved later.

Written from [`docs/spec/language.md`](../../docs/spec/language.md), not ported; the spec's
test vectors were recorded against id's `qcc` at the pinned commit, and the tests here check
the syntactic ones.

- **Layer:** L2 syntax. It depends on `qc-lexer`.
- **Spec sections:** the syntax of QC-10, QC-11, QC-13, QC-14 and QC-15 in
  [`language.md`](../../docs/spec/language.md), with the divergences QC-27.3, QC-27.7,
  QC-27.8, QC-27.9, QC-27.10 and QC-27.15; the syntax errors of QCD-3 and QCD-6 and the
  recovery of QCD-1.4 in [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md).
- **Shipped:** yes.

## Error recovery

Two rules, neither of which changes how a correct program parses:

- A declaration-shaped line at column 0 (a type word followed by a name, or by a parameter
  list and a name, or a `.`) cannot start a statement, so inside a function body it ends the
  body.
- After an error, tokens are skipped to the end of the statement or declaration (`;` at brace
  depth 0), to the `}` that closes the block, to a statement keyword starting a line, or to
  such a declaration line.

Only the first error of each statement or declaration is reported, as `qcc` reports one and
abandons the declaration (QC-24.2). Nesting is bounded (QC-27.15), so no input exhausts the
stack.

## Invariants

Each has a test in `src/tests/`:

- Parsing any bytes terminates and the tree's tokens tile the input exactly (property tests
  in `lossless.rs`).
- Deleting any single token of a correct file leaves every declaration before it, and every
  declaration from two after it on, parsed as before, with every error between them
  (`recovery::check_deletions`, run on `src/tests/sample.qc`, on generated programs, and by
  `cargo xtask parse-corpus --deletions` on id's QuakeC 1.01 in CI).
- Every file of the corpus, FTEQCC codebases included, round-trips
  (`cargo xtask parse-corpus`).
- Parsing never panics or overflows the stack and round-trips every input, under the
  grammar-aware fuzz target `fuzz/fuzz_targets/parser.rs` (`cargo xtask fuzz parser`), which
  mutates whole tokens and nests brackets past the nesting limit.
