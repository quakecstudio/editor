# qc-lexer

The vanilla QuakeC lexer. It splits one source file into tokens with byte spans, as id's
`qcc` splits it: names (keywords are names, since none is reserved), float, vector and string
literals, punctuation in id's order, and `$` commands (`$frame` lists and frame macro
references). Every lexical error is a diagnostic with its code, and lexing always runs to the
end of the file. It also has the diagnostic codes the preprocessor shares, and a line index.

Written from [`docs/spec/language.md`](../../docs/spec/language.md), not ported; the spec's
test vectors were recorded against id's `qcc` at the pinned commit, and the tests here check
them.

- **Layer:** L2 syntax. It depends on no other crate; `qc-preproc` and the parser build on
  it.
- **Spec sections:** QC-1 to QC-7 and the lexical part of QC-8 in
  [`language.md`](../../docs/spec/language.md), with the divergences QC-27.1, QC-27.2,
  QC-27.4, QC-27.5 (the `,` and `;` part), QC-27.6 and QC-27.14; the codes in QCD-1, QCD-2,
  QCD-6, QCD-7 and QCD-8 of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md).
- **Shipped:** yes.

## Invariants

Each has a property test in `src/tests.rs`:

- Lexing any bytes terminates, and token spans are in bounds, increasing and do not overlap.
- A token's own text lexes to the same token.
- The line of an offset is one more than the LF bytes before it (QC-1.2).
