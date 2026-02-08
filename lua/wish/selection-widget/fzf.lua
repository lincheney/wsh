local M = {}

local utf8 = require('wish/utf8')

local state = nil

function M.stop()
    if state and state.proc then
        state.proc.term()
        state.proc.wait()
        state = nil
    end
end

local function start_proc()
    state.cursor = wish.get_cursor()
    -- go to last line
    wish.set_cursor(utf8.len(wish.get_buffer()) + 1)
    wish.redraw()

    state.proc = wish.async.spawn{
        args = {
            'fzf',
            '--read0',
            '--exit-0',
            '--height=40%',
            '--reverse',
            '--with-nth=2..',
        },
        foreground = true,
        stdin = 'piped',
        stdout = 'piped',
    }
    state.resume()
end

function M.start(opts)
    if state and opts.data == state.data then
        state.accept_callback = opts.accept_callback or state.accept_callback
        return
    end
    M.stop()

    state = {
        data = opts.data,
        count = 0,
        proc = nil,
        no_more_input = false,
    }

    local resume, yield = wish.async.promise()
    state.resume = resume

    if type(opts.source) == 'function' then
        wish.schedule(function()
            wish.try(
                function()
                    for lines in opts.source() do
                        M.add_lines(lines)
                    end
                end,
                nil,
                function(err)
                    M.add_lines(nil)
                    if state then
                        state.resume()
                    end
                end
            )
        end)
    elseif type(opts.source) == 'table' then
        M.add_lines(opts.source)
        M.add_lines(nil)
    end

    yield()

    local result = nil
    if state.proc then
        -- and wait for the proc to finish
        local code = state.proc:wait()
        if code == 0 then
            result = tonumber(state.proc.stdout:read_all():match('^(%d+)\t'))
        end

        -- go back up
        io.stdout:write('\x1b[A')
        io.stdout:flush()

        wish.redraw{buffer=true, messages=true}
        wish.set_cursor(state.cursor)
        wish.redraw{all = true}
    end

    state = nil

    return result
end

function M.add_lines(lines)
    if not state then
        return
    end

    if lines and #lines > 0 then
        local str = {}
        for i = 1, #lines do
            table.insert(str, string.format('%i\t%s\0', state.count + i, lines[i].text))
        end
        state.count = state.count + #lines

        if not state.proc then
            start_proc()
        end
        state.proc.stdin:write(table.concat(str, ''))

    else
        state.no_more_input = true
        if state.proc then
            -- close stdin
            state.proc.stdin:close()
        else
            state.resume()
        end
    end
end

function M.is_active()
    return state ~= nil
end

return M
