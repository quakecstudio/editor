# QuakeC for Neovim

Neovim's own LSP client starts [`quakec-lsp`](../../apps/lsp/README.md), and
[nvim-treesitter](https://github.com/nvim-treesitter/nvim-treesitter) installs
[tree-sitter-quakec](https://github.com/quakecstudio/tree-sitter-quakec) for highlighting.
This directory is a runtime directory with the two files the setup needs:

- `lsp/quakec.lua`: the server's config, which `vim.lsp.enable('quakec')` reads (Neovim 0.11 or
  later, `:help lsp-new-config`);
- `plugin/quakec.lua`: `.qc` and `.qh` are the `quakec` filetype. Neovim reads `.qc` as C
  otherwise.

## The language server

Put `quakec-lsp` on `PATH` (`cargo build --release -p quakec-lsp`). Then add this directory to
the runtime path and enable the server, in `init.lua`:

```lua
vim.opt.runtimepath:prepend('/path/to/editor/editors/neovim')
vim.lsp.enable('quakec')
```

With a plugin manager, the directory is a local plugin instead, for example lazy.nvim's
`{ dir = '/path/to/editor/editors/neovim' }`. For a project with no `quakec.toml`, such as the
rerelease, pass its settings:

```lua
vim.lsp.config('quakec', {
  init_options = { dialect = 'fteqcc', target = 'rerelease' },
})
```

The server reads `init_options` only when it starts; after changing them, run
`:lsp restart quakec`. Go-to-definition is then Neovim's default: `CTRL-]`, or `vim.lsp.buf.definition()`.

## Highlighting

nvim-treesitter's `main` branch (Neovim 0.12 or later) installs a parser that is not in its
list from a `User TSUpdate` autocommand, with the queries from the grammar's `queries/`
directory:

```lua
vim.api.nvim_create_autocmd('User', {
  pattern = 'TSUpdate',
  callback = function()
    require('nvim-treesitter.parsers').quakec = {
      install_info = {
        url = 'https://github.com/quakecstudio/tree-sitter-quakec',
        revision = '77949595b4238c24eaf7641fc1ce156f307df1d6',
        queries = 'queries',
      },
    }
  end,
})

vim.api.nvim_create_autocmd('FileType', {
  pattern = 'quakec',
  callback = function()
    vim.treesitter.start()
  end,
})
```

Then run `:TSInstall quakec`. The parser and the filetype share the name `quakec`, so no
`vim.treesitter.language.register` is needed.

## Recorded test

```sh
nvim --headless --clean -l editors/neovim/test/definition.lua <rerelease dir>
```

with `quakec-lsp` on `PATH` and the rerelease from `cargo xtask corpus fetch rerelease`. It
loads this directory, opens `weapons.qc`, runs `vim.lsp.buf.definition()` on the `T_Damage`
call, and prints and checks `definition T_Damage weapons.qc:71:3: defs.qc:806:68`.
