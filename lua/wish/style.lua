local M = {}

M.normal = {
    fg = 'reset',
    bg = 'reset',
    bold = false,
    dim = false,
    italic = false,
    underline = false,
    strikethrough = false,
    reversed = false,
    blink = false,
    blend = false,
}
M.flag = {fg = '#ffaaaa'}
M.flag_value = {fg = '#ffdddd'}
M.escape = {fg = '#ffaaaa'}
M.escape_space = wish.table.merge({}, M.escape, {bg = '#442222'})
M.string = {fg = '#ffffaa', bg='#333300'}
M.heredoc_tag = {fg = 'lightblue', bold = true}
M.variable = {fg = 'lightmagenta'}
M.command = {fg = '#aaffaa', bold = true}
M.func = {fg = 'yellow'}
M.keyword = {fg = 'red'}
M.symbol = {fg = 'cyan'}
M.comment = {fg = 'grey'}
M.env_var_key = {fg = '#aa77ff'}
M.env_var_value = {fg = '#77aaff'}
M.error = {bg = 'red'}

return M
