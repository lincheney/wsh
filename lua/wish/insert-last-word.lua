local M = {}

local state = {
    original_buffer = nil,
    buffer = nil,
    histno = nil,
}

function M.handler(backward)
    local buffer = wish.get_buffer()
    local cursor = wish.str.to_byte_pos(buffer, wish.get_cursor()) or #buffer
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
        wish.set_cursor(wish.str.from_byte_pos(buffer, cursor - prev) or 0)
        wish.set_buffer(value, prev)
        state.buffer = state.original_buffer .. value
    end

end

return M
