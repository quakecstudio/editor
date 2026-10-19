# qc-lexer

The vanilla QuakeC lexer. It splits one source file into tokens with byte spans, as id's
`qcc` splits it: names (keywords are names, since none is reserved), float, vector and string
literals, punctuation in id's order, and `$` commands (`$frame` lists and frame macro
references). Every lexical error is a diagnostic with its code, and lexing always runs to the
end of the file. The diagnostic vocabulary itself, `Code`, `Diagnostic` and `Span`, lives in
[`qc-diag`](../qc-diag/README.md) and is re-exported here, so code written against
`qc_lexer::{Code, Diagnostic, Span}` keeps compiling. A line index is also provided.

`lex` lexes the vanilla dialect; `lex_with(source, dialect)` lexes a chosen `Dialect`. An
FTEQCC dialect also lexes the FTEQCC punctuators `++`, `--`, `+=`, `-=`, `*=`, `/=`, `%`, `?`
and `:` (`docs/spec/dialect-fteqcc-rerelease.md`, FTE-5 to FTE-8) and `->`, `~`, `^`, `<<`,
`>>`, `&=`, `|=`, `^=`, `&~=`, `<<=`, `>>=`, `(+)` and `(-)` (FTE-19.1, FTE-19.2), tried before
the vanilla operators they extend so the longer operator wins the longest match (QC-7.1); a
number that starts with `.` or ends with an `f` suffix (FTE-20.1, FTE-20.2); and FTEQCC's
one-byte string escapes (FTE-20.4). It also lexes the full dialect's tokens
(`docs/spec/dialect-fteqcc.md`): `::`, a `\` that joins two lines, character constants and
two-component vectors, FTEQCC's other escapes (FTEF-13), and its hex, octal and suffixed
numbers, each kept as a float of its value (FTEF-3, FTEF-13.5). Every other rule, and the
vanilla output byte for byte, is unchanged.

Written from [`docs/spec/language.md`](../../docs/spec/language.md), not ported; the spec's
test vectors were recorded against id's `qcc` at the pinned commit, and the tests here check
them.

- **Layer:** L2 syntax. It depends on no other crate; `qc-preproc` and `qc-syntax` build on
  it.
- **Spec sections:** QC-2, QC-4, QC-5, QC-6 and QC-8 in
  [`language.md`](../../docs/spec/language.md); the codes QCD-2 and QCD-6 of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md). The lexer implements
  QC-2, QC-4, QC-5, QC-6, QC-8 and QCD-2 in full; QCD-6 holds the lexer's own divergence
  codes, and every one of them is cited by a test. The code format and the diagnostic shape
  (QCD-1) and the `Code`, `Diagnostic` and `Span` types live in
  [`qc-diag`](../qc-diag/README.md). A readiness run attributes only these sections to the
  crate, and every active statement of each is cited here or in a crate that builds on it.
- **Also cited here, in sections a higher layer completes:** the lexical statements QC-1.1 to
  QC-1.4 (line index), QC-3.1 and QC-3.5 (names and keywords), QC-7.1, QC-7.2 and QC-7.4
  (punctuation), the lexer's own divergences QC-27.1, QC-27.2, QC-27.4, QC-27.5, QC-27.6 and
  QC-27.14, and the parser error codes QCD-3.1 to QCD-3.4 it defines. The rest of QC-1, QC-3,
  QC-7, QC-27 and QCD-3 is the parser's, the semantic model's or code generation's, so those
  sections are not declared above.
- **Shipped:** yes.

## Invariants

Each has a property test in `src/tests.rs`:

- Lexing any bytes terminates, and token spans are in bounds, increasing and do not overlap.
- A token's own text lexes to the same token.
- The line of an offset is one more than the LF bytes before it (QC-1.2).
