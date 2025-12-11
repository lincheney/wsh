local M = {}
local ITER_META = {}

setmetatable(M, {
    __call = function(self, x)
        return setmetatable(x, self)
    end,
})

M.__index = M

function M.copy(self, deep)
    local new = M{}
    for k, v in self do
        if type(k) == 'number' and k % 1 == 0 and k >= 1 and new[k] ~= nil then
            k = #new + 1
        end

        if type(v) == 'table' and deep then
            new[k] = M.copy(M(v), deep)
        else
            new[k] = v
        end
    end
    return new
end

M.collect = M.copy

function M.deepcopy(self)
    return M.copy(self, true)
end

function M.__call(self, state, var)
    return next(self, var)
end

local function make_iter(func)
    return setmetatable({func}, ITER_META)
end

function M.map(self, func)
    return make_iter(function(...)
        local k, v = self(...)
        return k, k and func(k, v)
    end)
end

function M.filter(self, func)
    return make_iter(function(state, k)
        local v
        while true do
            k, v = self(state, k)
            if not k or func(k, v) then
                return k, v
            end
        end
    end)
end

function M.filter_map(self, func)
    return M.map(self, func):filter(function(k, v) return v end)
end

function M.find(self, func)
    return M.filter(self, func)()
end

function M.any(self, func)
    return not not M.find(self, func)
end

function M.all(self, func)
    return not M.any(self, function(...) return not func(...) end)
end

function M.take_while(self, func)
    return make_iter(function(...)
        local k, v = self(...)
        if k and func(k, v) then
            return k, v
        end
    end)
end

function M.skip_while(self, func)
    local drop = true
    return M.filter(self, function(k, v)
        drop = drop and func(k, v)
        return not drop
    end)
end

function M.enumerate(self, func)
    local k, v
    local i = 0
    return make_iter(function()
        k, v = self(nil, k)
        i = i + 1
        return k and i, {k, v}
    end)
end

function M.count(self)
    local count = 0
    for _ in self do
        count = count + 1
    end
    return count
end

function M.chain(self, other)
    if type(other) == 'table' then
        other = M(other)
    end
    local iter = self
    return make_iter(function(state, index)
        local x = {iter(state, index)}
        if x[1] == nil and iter ~= other then
            iter = other
            x = {iter(nil, nil)}
        end
        return unpack(x)
    end)
end

function M.inspect(self, func)
    return make_iter(function(...)
        local k, v = self(...)
        if k then
            func(k, v)
        end
        return k, v
    end)
end

for k, v in pairs(M) do
    ITER_META[k] = v
end
ITER_META.__call = function(self, ...)
    return self[1](...)
end

return M
