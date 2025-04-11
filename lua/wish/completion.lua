local M = {}
local SELECTION = require('wish/selection-widget')
local msg = wish.set_message{hidden = true}

function M.complete()
    local matches = nil
    local comp = wish.get_completions()
    local loaded = false

    -- loading spinner thing
    wish.schedule(function()
        local dots = 0
        wish.async.sleep(50)
        while not loaded do
            wish.set_message{id = msg, hidden = false, text = 'Loading matches ' .. string.rep('.', dots), fg = 'grey'}
            wish.redraw()
            dots = (dots + 1) % 4
            wish.async.sleep(200)
        end
    end)

    wish.schedule(function()

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
        loaded = true

        if result then
            wish.set_message{id = msg, hidden = true}
            wish.insert_completion(comp, matches[result])
            wish.redraw()
        elseif not matches or #matches == 0 then
            wish.set_message{id = msg, hidden = false, text='No completion matches', fg='lightred'}
            wish.redraw()
        end
    end)

end

return M
