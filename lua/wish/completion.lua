local M = {}
local SELECTION = require('wish/selection-widget')
local LS_COLORS = require('wish/ls_colors')
local msg = wish.set_message{hidden = true, persist = true}

function M.complete()
    local matches = nil
    -- local comp = wish.get_completions()
    local loaded = false
    local cancelled = false
    local keymap_layer = wish.add_keymap_layer()

    wish.set_keymap('<esc>', function()
        cancelled = true
    end, keymap_layer)

    local all_matches = nil

    wish.schedule(function()
        wish.get_completions(nil, function(matches)
            loaded = true
            if cancelled then
                return
            end
            wish.set_message{id = msg, hidden = true}

            all_matches = all_matches or {}
            local filtered = {}
            for i = 1, #matches do
                local text = tostring(matches[i])
                if text then
                    table.insert(all_matches, matches[i])
                    local sgr = LS_COLORS.sgr_for(text, matches[i]:mode())
                    local props = sgr and wish.sgr_to_style(sgr) or {}
                    props.text = text
                    table.insert(filtered, props)
                end
            end

            if #filtered> 0 then
                SELECTION.add_lines(filtered)
            end
            -- wish.sleep(2)
        end)
        SELECTION.add_lines()
    end)

    -- loading message
    wish.set_message{id = msg, hidden = false, contents = 'Loading matches ...', fg = 'grey'}

    local result = SELECTION.start()
    loaded = true
    cancelled = true
    wish.del_keymap_layer(keymap_layer)
    -- comp:cancel()

    if result then
        wish.set_message{id = msg, hidden = true}
        wish.insert_completion(all_matches[result])
    elseif cancelled then
        wish.set_message{id = msg, hidden = true}
    elseif not all_matches or #all_matches == 0 then
        wish.set_message{id = msg, hidden = false, contents='No completion matches', fg='lightred'}
    end

end

wish.add_event_callback('accept_line', function()
    wish.set_message{id = msg, hidden = true}
end)

return M
