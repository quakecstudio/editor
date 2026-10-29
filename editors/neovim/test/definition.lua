-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 QuakeC Studio contributors
--
-- The recorded go-to-definition test for Neovim, on the rerelease:
--
--   nvim --headless --clean -l editors/neovim/test/definition.lua <rerelease dir>
--
-- with quakec-lsp on PATH and the rerelease from `cargo xtask corpus fetch rerelease`. It loads
-- this directory as a plugin, opens weapons.qc, puts the cursor on the `T_Damage` call and runs
-- vim.lsp.buf.definition(), the command `gd` and CTRL-] map to, and checks that it lands on the
-- declaration in defs.qc: the first probe of docs/spec/vectors/lsp/rerelease.txt
-- (`definition T_Damage weapons.qc:71:3: defs.qc:806:68`, 1-based).

local here = vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(vim.fs.abspath(arg[0]))))
local rerelease = arg[1]
if rerelease == nil then
  io.stderr:write('usage: nvim --headless --clean -l definition.lua <rerelease dir>\n')
  os.exit(2)
end

vim.opt.runtimepath:prepend(here)
vim.cmd.runtime('plugin/quakec.lua')
vim.lsp.config('quakec', { init_options = { dialect = 'fteqcc', target = 'rerelease' } })
vim.lsp.enable('quakec')

local function fail(message)
  io.stderr:write(message .. '\n')
  os.exit(1)
end

vim.cmd.edit(vim.fs.joinpath(rerelease, 'weapons.qc'))
if vim.bo.filetype ~= 'quakec' then
  fail('filetype is ' .. vim.bo.filetype .. ', not quakec')
end
if not vim.wait(30000, function()
  local client = vim.lsp.get_clients({ bufnr = 0, name = 'quakec' })[1]
  return client ~= nil and client.initialized == true
end) then
  fail('quakec-lsp did not start')
end

local found = vim.fn.search('T_Damage (trace_ent', 'cw')
if found == 0 then
  fail('weapons.qc has no `T_Damage (trace_ent`')
end
local from = vim.api.nvim_win_get_cursor(0)
local from_text = string.format('weapons.qc:%d:%d', from[1], from[2] + 1)

vim.lsp.buf.definition()
if not vim.wait(30000, function()
  return vim.fs.basename(vim.api.nvim_buf_get_name(0)) == 'defs.qc'
end) then
  fail('go-to-definition did not leave ' .. from_text)
end
local to = vim.api.nvim_win_get_cursor(0)
local to_text = string.format('defs.qc:%d:%d', to[1], to[2] + 1)
print(string.format('definition T_Damage %s: %s', from_text, to_text))
if from_text ~= 'weapons.qc:71:3' or to_text ~= 'defs.qc:806:68' then
  fail('expected weapons.qc:71:3 -> defs.qc:806:68')
end
os.exit(0)
