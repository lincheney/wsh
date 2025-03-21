local M = {}
local SELECTION = require('wish/selection-widget')

function M.complete()
    local matches = nil

    local result = SELECTION.start{
        source = function()
            local comp = wish.get_completions()
            matches = {}
            return function()
                while true do
                    local chunk = comp()
                    if not chunk then
                        return
                    end

                    local filtered_chunk = {}
                    for i = 1, #chunk do
                        local text = tostring(chunk[i])
                        if text then
                            table.insert(matches, chunk[i])
                            table.insert(filtered_chunk, {text = text})
                        end
                    end

                    if #filtered_chunk > 0 then
                        return filtered_chunk
                    end
                end
            end
        end
    }


    if result then
        wish.insert_completion(matches[result])
        wish.redraw()
    elseif #matches == 0 then
        wish.set_message{text='No completion matches', fg='lightred'}
        wish.redraw()
    end

end

return M
