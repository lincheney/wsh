return wish.plugin(function(wish, opts, plugin)

    local enable_mouse_mode = opts.enable_mouse_mode
    local timeout = opts.timeout
    local min_height = opts.min_height or 3
    local hide_on_stop = opts.hide_on_stop ~= false
    local resize_tty = opts.resize_tty
    local style = opts.style and opts.style.main or {
        border = {
            fg = 'magenta',
            type = 'Rounded',
        }
    }
    local saved_style = opts.style and opts.style.saved or {
        border = {
            fg = 'cyan',
            type = 'Rounded',
        }
    }

    local visible = false
    local active = false
    local msg = wish.set_message(wish.table.deep_merge({
        hidden = true,
        persist = true,
        dim = true,
        min_height = min_height,
    }, style))
    local saved_msg = wish.set_message(wish.table.deep_merge({
        hidden = true,
        persist = true,
        dim = true,
        min_height = min_height,
    }, saved_style))
    local layout_msg = wish.set_message{
        hidden = true,
        persist = true,
        direction = 'horizontal',
        children = {msg, saved_msg},
    }

    local buffer_change_callback = nil
    local message_resize_callback = nil
    local current_preview = nil

    local function debounce(delay, func)
        local next_call = nil
        return function(...)
            local running = next_call
            next_call = wish.time() + delay
            local args = {...}
            if not running then
                wish.schedule(function()
                    local wait = delay
                    while wait > 0 do
                        wish.sleep(wait)
                        wait = next_call - wish.time()
                    end
                    next_call = nil
                    func(unpack(args))
                end)
            end
        end
    end

    local live_preview

    local function preview(command)
        visible = true

        if not command:find('%S') then
            current_preview = nil
            -- hide only main msg in case saved msg is visible
            wish.set_message{id = msg, hidden = false, text = ' ', border = {enabled = false}}
            if message_resize_callback then
                wish.remove_event_callback(message_resize_callback)
                message_resize_callback = nil
            end
            return
        end

        local proc = wish.async.zpty{args = command, height = 1}
        -- kill any old proc
        if current_preview and not current_preview.proc:is_finished() then
            current_preview.proc:term()
        end

        -- become the new preview
        current_preview = {
            command = command,
            need_clear = true,
            buffer = '',
            output = '',
            proc = proc,
            is_current = function(self)
                return self == current_preview
            end,
            read_once = function(self)
                local data = self.proc.stdout:read()
                if not self:is_current() then
                    return false
                end
                if data then
                    self.buffer = self.buffer .. data
                end
                self:flush()
                return data
            end,
            flush = debounce(0.2, function(self)
                if not self:is_current() then
                    return
                end

                local value = self.buffer
                self.buffer = ''

                -- clear old msg
                if self.need_clear then
                    self.need_clear = false
                    self.output = ''
                    wish.clear_message(msg)
                end

                self.output = self.output .. value
                wish.feed_ansi_message(msg, value)
                wish.set_message{
                    id = msg,
                    border = {
                        dim = self.proc:is_finished(),
                        title_top = {text = self.command},
                    },
                    hidden = false,
                }
                wish.set_message{id = layout_msg, hidden = false}
            end),
        }

        -- kill this one after timeout
        if timeout then
            wish.schedule(function()
                wish.sleep(timeout)
                if not proc:is_finished() then
                    proc:term()
                    wish.pprint('killed')
                end
            end)
        end

        if not message_resize_callback and resize_tty then
            message_resize_callback = wish.add_event_callback('message_resize', function(ids)
                if not current_preview then
                    return
                end
                for i = 1, #ids do
                    if ids[i] == msg then
                        wish.schedule(function()
                            local geom = wish.get_message_geometry(msg)
                            wish.pprint(geom.height - 2)
                            current_preview.proc:set_tty_size(geom.height - 2, geom.width)
                        end)
                        break
                    end
                end
            end)
        end

        return current_preview
    end

    live_preview = debounce(0.2, function()
        local command = wish.get_buffer()
        local preview = preview(command)
        while preview and preview:read_once() do
        end
    end)

    wish.add_event_callback('accept_line', function(arg)
        plugin.hide()
    end)

    function plugin.hide()
        if visible then
            wish.set_message{id = layout_msg, hidden = true}
            visible = false
        end
    end

    function plugin.stop()
        if enable_mouse_mode then
            wish.enable_mouse_mode(false)
        end
        wish.remove_event_callback(buffer_change_callback)
        if message_resize_callback then
            wish.remove_event_callback(message_resize_callback)
            message_resize_callback = nil
        end
        if hide_on_stop then
            plugin.hide()
        end
        active = false
    end

    function plugin.start()
        buffer_change_callback = wish.add_event_callback('buffer_change', function(arg)
            live_preview()
        end)

        if enable_mouse_mode then
            wish.enable_mouse_mode(true)
        end
        live_preview()
        active = true
    end

    function plugin.toggle()
        if active then
            plugin.stop()
        else
            plugin.start()
        end
    end

    function plugin.save()
        if not active or not current_preview then
            return
        end

        -- clear saved widget and replay accumulated ANSI data
        wish.clear_message(saved_msg)
        wish.feed_ansi_message(saved_msg, current_preview.output)
        wish.set_message{
            id = saved_msg,
            hidden = false,
            border = {
                dim = true,
                title_top = {text = current_preview.command .. ' (saved)'},
            },
        }
    end

    local function check_is_in_msg(msg, x, y)
        local geom = wish.get_message_geometry(msg)
        if geom.x <= x and x < geom.x + geom.width and geom.y <= y and y < geom.y + geom.height then
            return msg
        end
    end

    wish.set_keymap('<scrolldown>', function(args)
        local scrolled = check_is_in_msg(msg, args.x, args.y) or check_is_in_msg(saved_msg, args.x, args.y)
        if scrolled then
            wish.scroll_message(scrolled, 1)
        end
    end)

    wish.set_keymap('<scrollup>', function(args)
        local scrolled = check_is_in_msg(msg, args.x, args.y) or check_is_in_msg(saved_msg, args.x, args.y)
        if scrolled then
            wish.scroll_message(scrolled, -1)
        end
    end)

end)
