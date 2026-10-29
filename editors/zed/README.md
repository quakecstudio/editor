# QuakeC for Zed

A Zed extension: the QuakeC language for `.qc` and `.qh` files, highlighted by
[tree-sitter-quakec](https://github.com/quakecstudio/tree-sitter-quakec) at the commit pinned
in `extension.toml`, and [`quakec-lsp`](../../apps/lsp/README.md) as its language server. Not
published to Zed's extension registry until the legal review (P4-22) signs off.

- `languages/quakec/highlights.scm` and `injections.scm` are the grammar's own queries at the
  pinned commit, unchanged; copy them again when the commit moves. Their capture names are
  the tree-sitter CLI's standard set; `@variable.builtin` and `@function.builtin` are not in
  Zed's documented list, and how a theme colours them is the theme's.
- `languages/quakec/brackets.scm` is Zed's own: the pairs it matches and colours.
- `src/lib.rs` finds the server and passes Zed's settings to it.

## Install from this directory

Zed builds the extension itself; it needs Rust through rustup (it adds the `wasm32-wasip2`
target) and downloads the grammar from the pinned commit.

1. Put `quakec-lsp` on `PATH` (`cargo build --release -p quakec-lsp`), or set its path below.
2. In Zed, run `zed: install dev extension` and choose this directory.

## Settings

In Zed's `settings.json`, for a project with no `quakec.toml` such as the rerelease:

```json
{
  "lsp": {
    "quakec-lsp": {
      "binary": { "path": "/path/to/quakec-lsp" },
      "initialization_options": { "dialect": "fteqcc", "target": "rerelease" }
    }
  }
}
```

`binary` is optional; without it the server is `quakec-lsp` on `PATH`, run in the project's
shell environment. The server reads `initialization_options` only when it starts, so restart
it after changing them. To colour names by the
server's semantic tokens as well, add `"semantic_tokens": "combined"`.

## Checks

```sh
cargo clippy --all-targets -- -D warnings
cargo clippy --target wasm32-wasip2 -- -D warnings
cargo build --release --target wasm32-wasip2
```

The package is its own workspace, outside the editor's, since Zed builds it here for
WebAssembly.

## Recorded test

Zed has no headless mode, so this one is by hand, on the rerelease
(`cargo xtask corpus fetch rerelease`):

1. Install the dev extension as above, with the settings above.
2. Open the rerelease directory as a project (`zed <rerelease dir>`) and open `weapons.qc`.
3. Check the status bar shows QuakeC and the file is highlighted.
4. Go to line 71, column 3, the `T_Damage` of `T_Damage (trace_ent, ...)`, and run
   `editor: go to definition` (F12).
5. It must open `defs.qc` at line 806, column 68, the `T_Damage` of its declaration.
