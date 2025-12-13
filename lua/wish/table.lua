local M = {}

function M.copy(tbl, deep)
    local new = {}
    for k, v in pairs(tbl) do
        if deep and type(v) == 'table' then
            v = M.copy(v, deep)
        end
        new[k] = v
    end
    return new
end

function M.merge(tbl, ...)
    for _, other in ipairs{...} do
        for k, v in pairs(other) do
            tbl[k] = v
        end
    end
    return tbl
end

function M.deep_merge(tbl, ...)
    for _, other in ipairs{...} do
        for k, v in pairs(other) do
            if type(tbl[k]) == 'table' and type(v) == 'table' then
                v = M.deep_merge(tbl[k], v)
            end
            tbl[k] = v
        end
    end
    return tbl
end

function M.extend(tbl, other)
    local ITER = require('wish/iter')
    if type(other) == 'table' then
        other = ITER(other)
    end

    for k, v in other do
        if v == nil then
            v = k
            k = #tbl + 1
        end

        if type(k) == 'number' and k % 1 == 0 and k >= 1 and tbl[k] ~= nil then
            k = #tbl + 1
        end
        tbl[k] = v
    end
    return tbl
end

function M.sort_by(tbl, key)
    local func = key
    if type(key) == 'function' then
        func = function(a, b) return key(a) < key(b) end
    else
        func = function(a, b) return a[key] < b[key] end
    end
    table.sort(tbl, func)
    return tbl
end

return M
