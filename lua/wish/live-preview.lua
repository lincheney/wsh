local M = {}

local active = false
local msg = wish.set_message{
    hidden = true,
    persist = true,
    dim = true,
    height = 'min:3',
    border = {
        fg = 'magenta',
        dim = false,
        type = 'Rounded',
    },
}
local saved_msg = wish.set_message{
    hidden = true,
    persist = true,
    dim = true,
    height = 'min:3',
    border = {
        fg = 'cyan',
        dim = false,
        type = 'Rounded',
    },
}
local layout_msg = wish.set_message{
    hidden = true,
    persist = true,
    direction = 'horizontal',
    children = {msg, saved_msg},
}
local buffer_change_callback = nil
local accept_line_callback = nil
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

local function preview(command)

    if not command:find('%S') then
        current_preview = nil
        wish.set_message{id = layout_msg, hidden = true}
        wish.redraw()
        return
    end

    local proc = wish.async.zpty(command)
    -- kill any old proc
    if current_preview then
        current_preview.proc:term()
    end

    -- become the new preview
    current_preview = {
        command = command,
        need_clear = true,
        buffer = '',
        output = '',
        proc = proc,
        is_current = function(self) return self == current_preview end,
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
                    title = {text = self.command},
                },
                hidden = false,
            }
            wish.set_message{id = layout_msg, hidden = false}
            wish.redraw()
        end),
    }
    return current_preview
end

local live_preview = debounce(0.2, function()
    local command = wish.get_buffer()
    local preview = preview(command)
    while preview and preview:read_once() do
    end
end)

local function stop()
    wish.remove_event_callback(buffer_change_callback)
    wish.remove_event_callback(accept_line_callback)
    wish.set_message{id = layout_msg, hidden = true}
    wish.set_message{id = saved_msg, hidden = true}
    active = false
    wish.redraw()
end

local function start()
    buffer_change_callback = wish.add_event_callback('buffer_change', function(arg)
        live_preview()
    end)

    accept_line_callback = wish.add_event_callback('accept_line', function(arg)
        stop()
    end)

    live_preview()
    active = true
end

wish.set_keymap('<a-p>', function()
    if active then
        stop()
    else
        start()
    end
end)

wish.set_keymap('<a-s>', function()
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
            title = {text = current_preview.command .. ' (saved)'},
        },
    }
    wish.redraw()
end)

return M
