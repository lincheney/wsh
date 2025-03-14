function wish.repr(val)
    if type(val) == 'table' then
        local text = {}
        for k, v in ipairs(val) do
            table.insert(text, wish.repr(v))
        end
        for k, v in pairs(val) do
            if type(k) == 'string' and not k:find('%W') then
                table.insert(text, k .. ' = ' .. wish.repr(v))
            elseif type(k) ~= 'number' or k > #val then
                table.insert(text, '['..wish.repr(k)..'] = ' .. wish.repr(v))
            end
        end
        return '{' .. table.concat(text, ', ') .. '}'
    elseif type(val) == 'string' then
        local val = string.format('%q', val):gsub('\\\n', '\\n')
        return val
    else
        return tostring(val)
    end
end

function wish.pprint(val)
    wish.log.debug(wish.repr(val))
end

function wish.async.spawn(...)
    local proc, stdin, stdout, stderr = wish.__spawn(...)
    return {
        stdin = stdin,
        stdout = stdout,
        stderr = stderr,
        id = function(self) return proc:id() end,
        wait = function(self) return proc:wait() end,
    }
end

