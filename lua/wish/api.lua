setmetatable(wish, {
    __index = function(self, key)
        local fn = rawget(self, '__get_'..key)
        if fn then
            -- if not self.__cache[key] then
                self.__cache[key] = fn()
            -- end
            return self.__cache[key]
        end
    end,
    __newindex = function(self, key, value)
        local fn = rawget(self, '__set_'..key)
        if fn then
            -- wish.__cache[key] = value
            fn(value)
        end
    end,
})
