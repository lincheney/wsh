local SELECTION = require('wish.selection-widget')
local LS_COLORS = require('wish.ls_colors')

return wish.plugin(function(wish, opts, plugin)

    local loading_msg = wish.set_message{hidden = true, persist = true}

    local selector = SELECTION.new().enable{
        style = {
            border = {
                fg = 'blue',
                type = 'rounded',
            }
        }
    }

    local selector_keybinds = opts.selector_keybinds or {
        ['<enter>'] = 'accept',
        ['<up>'] = 'up',
        ['<down>'] = 'down',
    }

    function plugin.start()
        selector.stop()

        local cancelled = false
        local keymap_layer = wish.add_keymap_layer()

        wish.set_keymap('<esc>', function()
            -- TODO
            cancelled = true
        end, keymap_layer)

        local all_matches = nil
        local result = nil
        local zsh_finished = false
        local selector_finished = false
        local buffer, cursor = wish.get_buffer()

        local function finish()
            if zsh_finished and selector_finished then
                wish.del_keymap_layer(keymap_layer)
                if result then
                    wish.set_message{id = loading_msg, hidden = true}
                    wish.set_buffer(buffer, cursor)
                    wish.insert_completion(all_matches[result])
                elseif cancelled then
                    wish.set_message{id = loading_msg, hidden = true}
                elseif not all_matches or #all_matches == 0 then
                    wish.set_message{id = loading_msg, hidden = false, contents = 'No completion matches', fg = 'red'}
                end
            end
        end

        wish.schedule(function()
            wish.get_completions(string.sub(buffer, 1, wish.str.to_byte_pos(buffer, cursor)), function(matches)
                if cancelled or selector_finished then
                    return
                end
                wish.set_message{id = loading_msg, hidden = true}

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

                if #filtered > 0 then
                    selector.add_lines(filtered)
                end
            end)
            zsh_finished = true
            if #all_matches <= 1 then
                selector.accept()
            else
                finish()
            end
            selector.add_lines()
        end)

        -- loading message
        wish.set_message{id = loading_msg, hidden = false, contents = 'Loading matches ...', fg = 'grey'}

        local opts = {
            keybinds = selector_keybinds
        }
        selector.start(opts, nil, function(r)
            result = r
            selector_finished = true
            finish()
        end)

    end

    wish.add_event_callback('accept_line', function()
        wish.set_message{id = loading_msg, hidden = true}
    end)

end)
