local M = {}

local state = nil

function M.stop()
    if state then
        state.proc.term()
        state.proc.wait()
        state = nil
    end
end

function M.start(opts)
    if state and opts.data == state.data then
        state.accept_callback = opts.accept_callback or state.accept_callback
        return
    end
    M.stop()

    -- go to last line
    wish.set_cursor(wish.str.len(wish.get_buffer()))
    wish.redraw()
    -- then down one
    io.stdout:write('\r\n')
    io.stdout:flush()

    state = {
        data = opts.data,
        count = 0,
        proc = wish.async.spawn{
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
        },
        accept_callback = opts.accept_callback,
        cursor = wish.get_cursor(),
    }

    if opts.lines then
        M.add_lines(opts.lines)
    end

    -- and wait for the proc to finish
    wish.schedule(function()
        if state then
            local code = state.proc:wait()
            local num = tonumber(state.proc.stdout:read_all():match('^(%d+)\t'))
            -- go back up
            io.stdout:write('\x1b[A')
            io.stdout:flush()

            wish.redraw{buffer=true, messages=true}
            wish.set_cursor(state.cursor)
            wish.redraw()

            if state.accept_callback and code == 0 and num then
                state.accept_callback(num)
            end
            state = nil
        end
    end)

end

function M.add_lines(lines)
    if lines and #lines > 0 then
        local str = {}
        for i = 1, #lines do
            table.insert(str, string.format('%i\t%s\0', state.count + i, lines[i].text))
        end
        state.count = state.count + #lines
        state.proc.stdin:write(table.concat(str, ''))
    else
        -- close stdin
        state.proc.stdin:close()
    end
end

function M.is_active()
    return state ~= nil
end

return M
