local M = {}
local utf8 = require('wish/utf8')

local state = {
    original_buffer = nil,
    buffer = nil,
    histno = nil,
}

function M.handler(backward)
    local buffer, cursor = wish.get_buffer()
    local cursor = wish.str.to_byte_pos(buffer, cursor) or #buffer
    local prefix = buffer:sub(1, cursor)

    if not state.histno or state.buffer ~= prefix then
        state.histno = wish.get_history_index()
        state.original_buffer = prefix
    end

    local histno, value
    if backward then
        histno, value = wish.get_prev_history(state.histno)
    else
        histno, value = wish.get_next_history(state.histno)
        if not histno or not value then
            histno = wish.get_history_index()
            value = ''
        end
    end

    if histno and value then
        state.histno = histno
        value = wish.shell_split(value)
        value = value[#value] or ''
        local prev = #prefix - #state.original_buffer
        buffer = buffer:sub(1, cursor - prev - 1) .. value .. buffer:sub(cursor)
        wish.set_buffer(value)
        state.buffer = state.original_buffer .. value
    end

end

return M
