return wish.plugin(function(wish, opts, plugin)

    local NAMESPACE = wish.add_buf_highlight_namespace()
    local chars = opts.chars or 'fjdkslarueiwoqpvn'
    local style = opts.style or {
        fg = 'cyan',
        underline = true,
        bold = true,
        no_blend = true,
    }
    local dim_style = opts.dim_style or {
        dim = true,
        no_blend = true,
    }

    local key_event_id = nil
    local keymap_layer = nil

    function plugin.deactivate()
        if keymap_layer then
            wish.del_keymap_layer(keymap_layer)
            keymap_layer = nil
        end
        if key_event_id then
            wish.remove_event_callback(key_event_id)
            key_event_id = nil
        end
        wish.clear_buf_highlights(NAMESPACE)
        wish.redraw()
    end

    local deactivate_now = plugin.deactivate
    local function deactivate_later()
        wish.schedule(deactivate_now)
    end

    function plugin.activate()
        local positions = nil
        key_event_id = wish.add_event_callback('key', function(key, data)
            wish.try{
                try = function()
                    -- we have highlighted keys and waiting for one of them
                    if positions then
                        if positions[data] then
                            wish.set_cursor(positions[data] + 1)
                            deactivate_later()
                        else
                            deactivate_now()
                        end
                        return
                    end

                    -- i pressed something else
                    if not data:find('^[%w%s%p]$') then
                        deactivate_now()
                        return
                    end

                    local buffer = wish.get_buffer()
                    local cursor = wish.get_cursor()
                    cursor = wish.str.to_byte_pos(buffer, cursor) or #buffer - 1
                    local matches = {}

                    local pat
                    if data:find('^[a-z]$') then
                        pat = '()['..data..data:upper()..']'
                    else
                        pat = '()['..data..']'
                    end
                    for s in buffer:gmatch(pat) do
                        s = wish.str.from_byte_pos(buffer, s) - 1
                        if s ~= cursor then
                            table.insert(matches, s)
                        end
                    end

                    -- nothing matched
                    if #matches == 0 then
                        deactivate_later()
                        return
                    end

                    -- jump directly to the only match
                    if #matches == 1 then
                        wish.set_cursor(matches[1] + 1)
                        deactivate_later()
                        return
                    end

                    -- sort by distance from cursor
                    wish.table.sort_by(matches, function(m) return math.abs(m - cursor) end)

                    -- highlight the keys
                    positions = {}
                    for i, m in ipairs(matches) do
                        local c = chars:sub(i, i)
                        local hl = wish.table.merge(wish.table.copy(style), {
                            start = m + 1,
                            finish = m + 1,
                            namespace = NAMESPACE,
                            virtual_text = c,
                            conceal = true,
                        })
                        wish.add_buf_highlight(hl)
                        positions[c] = m
                    end
                    wish.redraw()
                end,

                finally = function(err)
                    if err then
                        -- deactivate on any error so we don't get stuck
                        deactivate_now()
                    end
                end
            }
        end)
        keymap_layer = wish.add_keymap_layer(true)
        -- dim everything
        local hl = wish.table.merge(wish.table.copy(dim_style), {
            start = 0,
            finish = math.pow(2, 32) - 1,
            namespace = NAMESPACE,
        })
        wish.add_buf_highlight(hl)
        wish.redraw()
    end

end)
