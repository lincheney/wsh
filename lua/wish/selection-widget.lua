local default = require('wish.selection-widget.default')
local fzf = require('wish.selection-widget.fzf')

-- a selection widget needs
-- .new() which returns a plugin that has:
--      .start(source, on_accept)
--      .stop()
--      .add_lines(lines?)
--    and optionally
--      .accept()
--      .up()
--      .down()

local M = default
M.default = default
M.fzf = fzf
return M
