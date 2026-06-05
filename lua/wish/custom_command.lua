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

    local keyword = opts.keyword
    local callback = opts.callback

    if not init then
        wish.add_event_callback('init', function()
            if not init then
                init = true
                wish.silent_cmd[[setopt interactivecomments]]
            end
        end)
    end

    wish.add_event_callback('init', function()
        wish.silent_cmd('alias ' .. keyword .. '=" # "')
    end)

    wish.add_event_callback('precmd', function(buffer)
        local content = buffer and M.extract(buffer, keyword)
        if content then
            callback(content)
        end
    end)
end

return M
