-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 QuakeC Studio contributors
--
-- quakec-lsp for QuakeC buffers, read by vim.lsp.enable('quakec') (Neovim 0.11 or later,
-- `:help lsp-new-config`). A project with no quakec.toml takes its settings from init_options,
-- with the names quakec.toml uses (apps/lsp/README.md, Settings); override them in
-- after/lsp/quakec.lua or with vim.lsp.config('quakec', { ... }).

return {
  cmd = { 'quakec-lsp' },
  filetypes = { 'quakec' },
  root_markers = { 'quakec.toml', 'progs.src', '.git' },
}
