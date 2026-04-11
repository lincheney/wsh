local M = {}

function M.extract(command, keyword)
    command = wish.str.lstrip(command)
    command = wish.str.removeprefix(command, keyword)
    if command and command:find('^%s+%S') then
        return wish.str.lstrip(command)
    end
end

local init = false
function M.register(wish, opts)
    wish.add_event_callback('init', function()
        if not init then
            init = true
            wish.silent_cmd[[setopt interactivecomments]]:wait()
        end
    end)

    local keyword = opts.keyword
    local callback = opts.callback

    local accept_line
    accept_line = wish.add_event_callback('accept_line', function()
        local alias_cmd = 'alias ' .. keyword .. '=" # "'
        wish.silent_cmd(alias_cmd):wait()
        wish.remove_event_callback(accept_line)
    end)

    wish.add_event_callback('precmd', function(buffer)
        local content = buffer and M.extract(buffer, keyword)
        if content then
            callback(content)
        end
    end)
end

return M
