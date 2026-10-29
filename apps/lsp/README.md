# quakec-lsp

The QuakeC language server (LANG-7, LANG-9): QuakeC Studio's editor features over the Language
Server Protocol 3.18, built from the same crates as the IDE, so VS Code, Zed, Neovim and any
other LSP client get the answers QuakeC Studio gives. The binary is `quakec-lsp`
([ADR-0006](../../docs/adr/0006-product-name.md)).

- **Layer:** apps. It depends on `qc-ide` (L3), which holds every feature, and `qc-project`
  (L2) for the file loader and the project settings.
- **Specification:** LSP 3.18, cached in [`specs/lsp/3.18/`](../../specs/lsp/3.18/); every
  protocol rule the code and the tests follow is quoted from there.
- **Spec sections:** none of `docs/spec/`; the engine behaviour behind each answer is
  `qc-ide`'s and `qc-sema`'s.
- **Shipped:** yes.

## Model

rust-analyzer splits its server in two: the `ide` crate answers questions about code in its own
terms, and the `rust-analyzer` binary owns the protocol, the main loop and the conversion of
offsets to the client's lines and columns. This server follows that split: the analysis is
`qc_ide::AnalysisHost`, and this crate only frames messages, keeps the lifecycle, and converts
byte offsets and paths to positions and URIs. It reads one message at a time and answers it
before the next, so every answer is computed on the state the messages before it left.

## Features

| Request or notification | Answer |
| --- | --- |
| `initialize` | The capabilities below; UTF-8 positions when the client offers them, else UTF-16 |
| `textDocument/didOpen`, `didChange` (full and incremental), `didClose` | The open text replaces the file on disk in every module that compiles it |
| `textDocument/didSave` | Reads the project again, for a changed `#include`, keeping each unchanged module's analysis |
| `textDocument/publishDiagnostics` | Every problem of each file the build would report, `W0311` for the target engine's missing builtins included; an empty list clears a file |
| `textDocument/definition` | The first declaration of the name at the cursor |
| `textDocument/references` | Every use, and with `includeDeclaration` every declaration, in every module |
| `textDocument/prepareRename`, `textDocument/rename` | The edits of every declaration and use; refused for a name the engine binds, through a macro, or when the new name would change what the code means; the edits of a function or field a map may name each need the modder's confirmation, through change annotations, and are refused to a client that cannot confirm them (ADR-0019) |
| `textDocument/hover` | The declaration as written, its comment, and for a builtin which engines have it |
| `textDocument/completion` | The names in scope; fields after `.` |
| `textDocument/semanticTokens/full` | Functions, fields, globals, parameters and locals, with declaration, constant and builtin modifiers |
| `textDocument/inlayHint` | The parameter name before each argument of a call |

## Settings

A project with a `quakec.toml` gives each module its dialect and target engine. A project with
none, such as the rerelease's QuakeC, is vanilla unless the client says otherwise in
`initializationOptions`, with the names `quakec.toml` uses:

```json
{ "dialect": "fteqcc", "target": "rerelease", "autoproto": false }
```

`autoproto` is FTEQCC's `-Fautoproto` (FTE-22.3), which `rerelease-mg3` is built with.

## Tests

`cargo test -p quakec-lsp` drives whole sessions through the base protocol's framing in memory,
one module per area in `src/tests/`, each test quoting the sentence of LSP 3.18 it pins. `cargo
xtask lsp-corpus <rerelease dir>` drives every request against the rerelease codebase and
compares the answers with [`docs/spec/vectors/lsp/rerelease.txt`](../../docs/spec/vectors/lsp/rerelease.txt);
the corpus CI job runs it.
