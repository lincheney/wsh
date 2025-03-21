local M = {}
local SELECTION = require('wish/selection-widget')

function M.complete()
    local comp = wish.get_completions()
    local matches = {}

    for chunk in comp do

        local filtered_chunk = {}
        for i = 1, #chunk do
            local text = tostring(chunk[i])
            if text then
                table.insert(matches, chunk[i])
                table.insert(filtered_chunk, {text = text})
            end
        end

        if #filtered_chunk > 0 then
            if not SELECTION.is_active() then
                SELECTION.start{
                    accept_callback = function(i)
                        wish.insert_completion(matches[i])
                        wish.redraw()
                    end,
                }
            end
            SELECTION.add_lines(filtered_chunk)
        end

    end

    if #matches == 0 then
        wish.set_message{text='No completion matches', fg='lightred'}
        wish.redraw()
    else
        SELECTION.add_lines(nil)
    end

end

return M
