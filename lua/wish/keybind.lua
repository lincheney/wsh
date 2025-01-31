wish.set_keymap('<bs>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        local buffer = wish.get_buffer()
        wish.set_buffer(buffer:sub(1, cursor - 1) .. buffer:sub(cursor + 1))
        wish.set_cursor(cursor - 1)
    end
end)

wish.set_keymap('<c-u>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        local buffer = wish.get_buffer()
        wish.set_buffer(buffer:sub(cursor + 1))
        wish.set_cursor(0)
    end
end)

wish.set_keymap('<c-k>', function()
    local cursor = wish.get_cursor()
    local buffer = wish.get_buffer():sub(1, cursor)
    wish.set_buffer(buffer)
    wish.set_cursor(#buffer)
end)

wish.set_keymap('<c-a>', function() wish.set_cursor(0) end)
wish.set_keymap('<home>', function() wish.set_cursor(0) end)
wish.set_keymap('<c-e>', function() wish.set_cursor(#wish.get_buffer()) end)
wish.set_keymap('<end>', function() wish.set_cursor(#wish.get_buffer()) end)
wish.set_keymap('<left>', function() local cursor = wish.get_cursor(); wish.set_cursor(math.max(0, cursor-1)) end)
wish.set_keymap('<right>', function() local cursor = wish.get_cursor(); wish.set_cursor(cursor+1) end)
