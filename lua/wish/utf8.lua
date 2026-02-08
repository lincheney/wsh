local M = {}

function M.sub(str, start, stop)
    local x
    start = wish.str.to_byte_pos(str, start) or #str + 1
    if stop and stop > 0 then
        x, stop = wish.str.to_byte_pos(str, stop)
    end
    return string.sub(str, start, stop)
end

function M.find(str, pat, init, plain)
    if init then
        init = wish.str.to_byte_pos(str, init) or #str
    end
    local s, e = string.find(str, pat, init, plain)
    if s then
        s = wish.str.from_byte_pos(str, s)
        e = wish.str.from_byte_pos(str, e)
        return s, e
    end
end

function M.len(str)
    return wish.str.len(str)
end

return M
