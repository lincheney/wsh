local default = require('wish/selection-widget/default')
local fzf = require('wish/selection-widget/fzf')

local M = fzf
M.default = default
M.fzf = fzf
return M
