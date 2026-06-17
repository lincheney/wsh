return wish.plugin(function(wish, opts, plugin)

    local NAMESPACE = wish.add_buf_highlight_namespace()
    local PRIORITY = 10000
    local clear_paste = nil

    local flash_style = opts.flash_style or {
        fg = 'blue'
    }
    local flash_timeout = opts.flash_timeout or 0.5

    wish.add_event_callback('paste', function(data)
        -- insert this into the buffer
        if #data > 0 then
            local id = math.random()
            clear_paste = id
            local _buffer, cursor = wish.get_buffer()

            -- paste
            wish.insert_at_cursor(data)

            if not next(flash_style) then
                -- no styling
                return
            end

            -- flash for a bit
            wish.clear_buf_highlights(NAMESPACE)
            local hl = wish.table.merge(wish.table.copy(flash_style), {
                namespace = NAMESPACE,
                start = cursor,
                finish = cursor + wish.utf8.len(data) - 1,
                priority = PRIORITY,
            })
            wish.add_buf_highlight(hl)

            if flash_timeout > 0 then
                wish.schedule(function()
                    wish.sleep(flash_timeout)
                    if clear_paste == id then
                        wish.clear_buf_highlights(NAMESPACE)
                    end
                end)
            end

        end
    end)

    wish.add_event_callback('accept_line', function()
        wish.clear_buf_highlights(NAMESPACE)
    end)

end)
