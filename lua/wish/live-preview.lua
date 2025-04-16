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
local epoch = 0
local drawing = false

local function live_preview()
    wish.schedule(function()
        local buffer = wish.get_buffer()
        local this_epoch = epoch + 1
        epoch = this_epoch

        if #buffer == 0 then
            wish.set_ansi_message{id = msg, hidden = true}
            wish.redraw()
            return
        end

        local proc = wish.async.spawn{
            args = {'bash', '-c', 'exec 2>&1; ' .. buffer},
            stdin = 'null',
            stdout = 'piped',
            stderr = 'null',
        }
        local cleared = false
        local stdout = ''
        while true do
            local data = proc.stdout:read()
            if epoch ~= this_epoch then
                break
            end

            if data then
                stdout = stdout .. data
            end
            if not data or #stdout == #data then
                wish.schedule(function()
                    wish.async.sleep(100)
                    if epoch ~= this_epoch then
                        return
                    end
                    local value = stdout
                    stdout = ''
                    if not cleared then
                        cleared = true
                        wish.clear_ansi_message(msg)
                    end
                    wish.feed_ansi_message(msg, value)
                    wish.set_ansi_message{
                        id = msg,
                        hidden = false,
                        border = {
                            dim = not not data,
                            title = {text = buffer},
                        },
                    }
                    wish.redraw()
                end)
            end
            if not data then
                break
            end
        end

        proc.wait()
    end)
end

local function stop()
    wish.remove_event_callback(buffer_change_callback)
    wish.remove_event_callback(accept_line_callback)
    epoch = epoch + 1
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
