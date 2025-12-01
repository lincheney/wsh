local M = {}

local active = false
local msg = wish.set_ansi_message{
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
                    wish.async.sleep(wait)
                    wait = next_call - wish.time()
                end
                next_call = nil
                func(unpack(args))
            end)
        end
    end
end

local function preview(buffer)

    if not buffer:find('%S') then
        current_preview = nil
        wish.set_ansi_message{id = msg, hidden = true}
        wish.redraw()
        return
    end

    -- -- this ought to be using something like zpty
    -- local proc = wish.async.spawn{
        -- args = {'bash', '-c', 'exec 2>&1; ' .. buffer},
        -- stdin = 'null',
        -- stdout = 'piped',
        -- stderr = 'null',
    -- }
    local proc = wish.async.zpty(buffer)
    -- kill any old proc
    if current_preview then
        current_preview.proc:term()
    end

    -- become the new preview
    current_preview = {
        buffer = buffer,
        need_clear = true,
        stdout = '',
        proc = proc,
        is_current = function(self) return self == current_preview end,
        read_once = function(self)
            local data = self.proc.pty:read()
            if not self:is_current() then
                return false
            end
            if data then
                self.stdout = self.stdout .. data
            end
            self:flush()
            return data
        end,
        flush = debounce(0.2, function(self)
            if not self:is_current() then
                return
            end

            local value = self.stdout
            self.stdout = ''

            -- clear old msg
            if self.need_clear then
                self.need_clear = false
                wish.clear_ansi_message(msg)
            end

            wish.feed_ansi_message(msg, value)
            wish.set_ansi_message{
                id = msg,
                hidden = false,
                border = {
                    dim = self.proc:is_finished(),
                    title = {text = self.buffer},
                },
            }
            wish.redraw()
        end),
    }
    return current_preview
end

local live_preview = debounce(0.2, function()
    local buffer = wish.get_buffer()
    local preview = preview(buffer)
    while preview and preview:read_once() do
    end
end)

local function stop()
    wish.remove_event_callback(buffer_change_callback)
    wish.remove_event_callback(accept_line_callback)
    wish.set_ansi_message{id = msg, hidden = true}
    active = false
    wish.redraw()
end

local function start()
    wish.set_message{text = 'live'}

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

return M
