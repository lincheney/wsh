wish.async = {
    spawn = wish.__async_spawn,
    sleep = wish.__async_sleep,
}

wish.repr = function(val)
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
        return '{' .. table.concat(text, ',') .. '}'
    elseif type(val) == 'string' then
        return string.format('%q', val)
    else
        return tostring(val)
    end
end

wish.pprint = function(val)
    wish.log.debug(wish.repr(val))
end
