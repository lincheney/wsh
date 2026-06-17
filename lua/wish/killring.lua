local M = {}

function M.push(value)
    wish.in_zle_param_scope(function()
        local killring = wish.get_var('killring')
        killring[#killring] = nil
        table.insert(killring, 1, wish.get_var('CUTBUFFER') or '')
        wish.set_var('CUTBUFFER', value)
        wish.set_var('killring', killring)
    end)
end

function M.get()
    return wish.in_zle_param_scope(function()
        local killring = wish.get_var('killring')
        table.insert(killring, 1, wish.get_var('CUTBUFFER'))
        wish.table.filter(killring, function(x) return x ~= '' end)
        return killring
    end)
end

return M
