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
