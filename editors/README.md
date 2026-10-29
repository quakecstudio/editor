# Editor integrations

QuakeC support for VS Code, Zed and Neovim (P2-16; LANG-8, LANG-9). Each one starts
[`quakec-lsp`](../apps/lsp/README.md), the language server built from the same crates as
QuakeC Studio, so go-to-definition, references, rename, hover, completion, diagnostics,
semantic tokens and inlay hints are the server's in every editor. Highlighting in Zed and
Neovim is the tree-sitter grammar,
[quakecstudio/tree-sitter-quakec](https://github.com/quakecstudio/tree-sitter-quakec), pinned
at `77949595b4238c24eaf7641fc1ce156f307df1d6`; VS Code has no tree-sitter, so it gets a small
TextMate grammar for the lexical tokens and the server's semantic tokens for names.

| Editor | Directory | What it is |
| --- | --- | --- |
| VS Code | [`vscode/`](vscode/README.md) | An extension in TypeScript on `vscode-languageclient` |
| Zed | [`zed/`](zed/README.md) | A Zed extension: the grammar, its queries, and a Rust module that starts the server |
| Neovim | [`neovim/`](neovim/README.md) | A runtime directory with the LSP config and the filetype, and the setup documented |

**Not published.** None of them is on a marketplace, the VS Code Marketplace, Open VSX or
Zed's extension registry, until the legal review (P4-22) signs off; each is installed from
this directory.

## The server

Build it and put it on `PATH`, or point the editor's setting at it:

```sh
cargo build --release -p quakec-lsp    # target/release/quakec-lsp
```

A project with a `quakec.toml` gives each module its dialect and target engine. One without,
such as the rerelease's QuakeC, is vanilla unless the editor passes the options below; each
editor's README shows where. For the rerelease:

```json
{ "dialect": "fteqcc", "target": "rerelease" }
```

## Recorded tests

Each editor's README has a go-to-definition test on the rerelease
(`cargo xtask corpus fetch rerelease`): the `T_Damage` call at `weapons.qc:71:3` must land
on its declaration at `defs.qc:806:68`, the first probe of
[`docs/spec/vectors/lsp/rerelease.txt`](../docs/spec/vectors/lsp/rerelease.txt). VS Code's
and Neovim's are scripts; Zed has no headless mode, so its test is a manual checklist.
