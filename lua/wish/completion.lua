local M = {}
local SELECTION = require('wish/selection-widget')

function M.complete()
    local matches = nil
    local comp = wish.get_completions()
    local loaded = false
    local msg = wish.set_message{hidden = true}

    -- loading spinner thing
    wish.schedule(function()
        local dots = 1
        while not loaded do
            wish.set_message{id = msg, hidden = false, text = 'Loading matches ' .. string.rep('.', dots)}
            wish.redraw()
            dots = (dots + 1) % 4
            wish.async.sleep(200)
        end
    end)

    local result = SELECTION.start{
        source = function()
            matches = {}
            return function()
                while true do
                    local chunk = comp()
                    loaded = true
                    if not chunk then
                        return
                    end
                    wish.set_message{id = msg, hidden = true}

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
    comp:cancel()

    if result then
        wish.insert_completion(matches[result])
        wish.redraw()
    elseif #matches == 0 then
        wish.set_message{id = msg, hidden = false, text='No completion matches', fg='lightred'}
        wish.redraw()
    end

end

return M
