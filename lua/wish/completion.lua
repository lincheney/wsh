local M = {}
local SELECTION = require('wish/selection-widget')
local COMPLETION = {}

function M.complete()
    local all_matches = {}
    SELECTION.show{
        align = 'Left',
        border = {
            fg = 'magenta',
            type = 'Rounded',
            title = {
                text = 'completion ...',
            },
        },
        selected = 1,
        data = COMPLETION,
        callback = function(i)
            if all_matches[i] then
                wish.insert_completion(all_matches[i])
                SELECTION.stop()
            end
        end,
    }

    local comp = wish.get_completions()
    for chunk in comp do
        if SELECTION.get_data() ~= COMPLETION then
            comp:cancel()
            return
        end

        local text = {}
        for _, cmatch in ipairs(chunk) do
            table.insert(all_matches, cmatch)
            table.insert(text, {text = tostring(cmatch) .. '\n'})
        end

        if #text > 0 then
            SELECTION.add_lines(text)
        end
    end

    if #all_matches == 1 then
        wish.insert_completion(all_matches[1])
        SELECTION.stop()
    else
        -- indicate we are finished
        SELECTION.show{border = {title = {text = 'completion' }}}
    end

end

function M.complete()
    local cursor = wish.get_cursor()

    -- start fzf
    local function start_fzf()
        -- go to last line
        wish.set_cursor(wish.str.len(wish.get_buffer()))
        wish.redraw()
        -- then down one
        io.stdout:write('\r\n')
        io.stdout:flush()

        return wish.async.spawn{
            args={
                'fzf',
                '--exit-0',
                '--height=40%',
                '--reverse',
                '--with-nth=2..',
            },
            foreground=true,
            stdin='piped',
            stdout='piped',
        }
    end

    local proc = nil
    -- get completions and feed to fzf
    local comp = wish.get_completions()
    local matches = {}
    local ok, err = pcall(function()
        for chunk in comp do

            local str = {}
            for i = 1, #chunk do
                if tostring(chunk[i]) then
                    table.insert(matches, chunk[i])
                    table.insert(str, string.format('%i\t%s\n', #matches, chunk[i]))
                end
            end

            if #str then
                -- start fzf
                proc = proc or start_fzf()
                proc.stdin:write(table.concat(str, ''))
            end
        end
    end)

    if proc then
        proc.stdin:close()
        local code = proc:wait()
        local num = tonumber(proc.stdout:read_all():match('^(%d+)\t'))
        -- go back up
        io.stdout:write('\x1b[A')
        io.stdout:flush()

        wish.redraw{all=true}
        wish.set_cursor(cursor)
        wish.redraw()

        if not ok then
            error(err)
        elseif code == 0 and matches[num] then
            wish.insert_completion(matches[num])
            wish.redraw()
        end

    else
        if not ok then
            error(err)
        end
        wish.show_message{text='No completion matches', fg='lightred'}
        wish.redraw()

    end


end

return M
