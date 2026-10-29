-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 QuakeC Studio contributors
--
-- Neovim reads `.qc` as C by default; these are QuakeC.

vim.filetype.add({
  extension = {
    qc = 'quakec',
    qh = 'quakec',
  },
})
