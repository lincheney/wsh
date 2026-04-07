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

function M.append(first, second)
    for i = 1, #second do
        first[#first + 1] = second[i]
    end
    return first
end

return M
