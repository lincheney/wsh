local M = {}

local NAMESPACE = wish.add_buf_highlight_namespace()
local suggestion = ''
local history = nil

wish.add_event_callback('accept_line', function()
    history = nil
    wish.clear_buf_highlights(NAMESPACE)
end)

wish.add_event_callback('buffer_change', function()
    local buffer = wish.get_buffer()

    -- check if prev text still matches
    if buffer == '' then
        suggestion = nil
    elseif not suggestion or suggestion:sub(1, #buffer) ~= buffer then
        suggestion = nil
        -- refetch history
        history = history or ({wish.get_history()})[2]
        -- find a new one
        for i = 1, #history do
            if history[i].text:sub(1, #buffer) == buffer then
                -- got one
                suggestion = history[i].text
                break
            end
        end
    end

    wish.clear_buf_highlights(NAMESPACE)
    local suffix = suggestion and suggestion:sub(#buffer + 1)
    if suffix and suffix ~= ''  then
        wish.add_buf_highlight{
            start = math.pow(2, 32) - 1,
            finish = math.pow(2, 32) - 1,
            fg = 'grey',
            virtual_text = suffix,
            namespace = NAMESPACE,
        }
    end
    wish.redraw()

end)

function M.accept_suggestion()
    if suggestion and suggestion ~= '' then
        wish.set_cursor(0)
        wish.set_buffer(suggestion)
    end
end

return M
