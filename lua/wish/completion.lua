local M = {}
local SELECTION = require('wish/selection-widget')
local msg = wish.set_message{hidden = true, persist = true}

function M.complete()
    local matches = nil
    local comp = wish.get_completions()
    local loaded = false
    local cancelled = false
    local keymap_layer = wish.add_keymap_layer()

    wish.set_keymap('<esc>', function()
        comp:cancel()
        cancelled = true
    end, keymap_layer)

    -- loading spinner thing
    wish.schedule(function()
        local dots = 0
        wish.async.sleep(0.05)
        while not loaded do
            wish.set_message{id = msg, hidden = false, text = 'Loading matches ' .. string.rep('.', dots), fg = 'grey'}
            wish.redraw()
            dots = (dots + 1) % 4
            wish.async.sleep(0.2)
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
        loaded = true
        wish.del_keymap_layer(keymap_layer)
        comp:cancel()

        if result then
            wish.set_message{id = msg, hidden = true}
            wish.insert_completion(comp, matches[result])
            wish.redraw()
        elseif cancelled then
            wish.set_message{id = msg, hidden = true}
            wish.redraw()
        elseif not matches or #matches == 0 then
            wish.set_message{id = msg, hidden = false, text='No completion matches', fg='lightred'}
            wish.redraw()
        end
    end)

end

wish.add_event_callback('accept_line', function()
    wish.set_message{id = msg, hidden = true}
end)

return M
