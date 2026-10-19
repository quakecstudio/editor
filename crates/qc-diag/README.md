# qc-diag

The one QuakeC diagnostic vocabulary. Every stage of the QuakeC front end reports the same
kind of thing: an error or a warning, with a code from
[`docs/spec/language-diagnostics.md`](../../docs/spec/language-diagnostics.md), a message with
the names filled in, and the byte span it is about. This crate owns that one `Diagnostic`
type, the `Code` enum and its `Severity`, and the `Span` byte range, so the lexer
([`qc-lexer`](../qc-lexer/README.md)), the preprocessor
([`qc-preproc`](../qc-preproc/README.md)), the parser
([`qc-syntax`](../qc-syntax/README.md)), the project graph
([`qc-project`](../qc-project/README.md)), the semantic model
([`qc-sema`](../qc-sema/README.md)) and, above them, the compiler, the editor, the language
server and the command line all produce and consume the same value. An as-you-type error in
the editor is then the same `Diagnostic` a full build reports (COMP-2).

The crate deliberately holds only the diagnostic vocabulary and no logic that raises
diagnostics: each stage keeps its own rules and cites the spec statements it reports. `Span`
and `Code` live here, and `qc-lexer` re-exports them, so code written against
`qc_lexer::{Code, Diagnostic, Span}` keeps compiling unchanged.

- **Layer:** L0 formats. It depends on no other crate; the lexer, preprocessor, parser,
  project graph, semantic model and compiler build on it.
- **Spec sections:** [QCD-1](../../docs/spec/language-diagnostics.md#qcd-1-codes) (the code
  format QCD-1.1, the no-reuse rule QCD-1.2 and the diagnostic shape QCD-1.3) of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md). Written from the spec,
  not ported.
- **Not this crate's:** the codes of QCD-2 to QCD-8, which this crate names and the stages that
  raise them cite.
- **Shipped:** yes.

## Public shape

- `Code` and `Severity` (`code`): the diagnostic codes of `docs/spec/language-diagnostics.md`
  and whether each is an error or a warning. `Code::ALL` lists them in order; `Code::as_str`
  gives the written form (`E0101`); `Code::is_error` and `Code::severity` give the severity.
- `Span`: a byte range `start..end` in one source file, with `range` and `contains`.
- `Diagnostic`: a code, a message and a primary span (QCD-1.3), with `is_error` and
  `severity`.
