wish.set_keymap('<bs>', function()
    if wish.cursor > 0 then
        wish.buffer = wish.buffer:sub(1, wish.cursor) .. wish.buffer:sub(wish.cursor + 1)
        wish.cursor = wish.cursor - 1
    end
end)
