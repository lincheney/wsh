wish.set_keymap('<bs>', function()
    if wish.cursor > 0 then
        wish.buffer = wish.buffer:sub(1, wish.cursor - 1) .. wish.buffer:sub(wish.cursor + 1)
        wish.cursor = wish.cursor - 1
    end
end)

wish.set_keymap('<c-u>', function()
    if wish.cursor > 0 then
        wish.buffer = wish.buffer:sub(wish.cursor + 1)
        wish.cursor = 0
    end
end)

wish.set_keymap('<c-k>', function()
    wish.buffer = wish.buffer:sub(1, wish.cursor)
    wish.cursor = #wish.buffer
end)

wish.set_keymap('<c-a>', function() wish.cursor = 0 end)
wish.set_keymap('<home>', function() wish.cursor = 0 end)
wish.set_keymap('<c-e>', function() wish.cursor = #wish.buffer end)
wish.set_keymap('<end>', function() wish.cursor = #wish.buffer end)
wish.set_keymap('<left>', function() wish.cursor = math.max(0, wish.cursor - 1) end)
wish.set_keymap('<right>', function() wish.cursor = wish.cursor + 1 end)

-- there ought to be a better way of doing this
wish.set_keymap('<c-d>', function()
    if #wish.buffer == 0 then
        wish.buffer = 'exit'
        wish.accept_line()
    end
end)

wish.set_keymap('<tab>', function()
    for cmatch in wish.get_completions() do
        io.stderr:write("DEBUG(entrap)    ".."cmatch"..(" = %q\r\n"):format(cmatch))
    end
end)
