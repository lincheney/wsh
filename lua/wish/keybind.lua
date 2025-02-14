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

wish.set_keymap('<c-left>', function()
    if wish.cursor > 0 then
        local cursor = wish.buffer:sub(1, wish.cursor):find('%S+%s*$')
        wish.cursor = (cursor or 1) - 1
    end
end)
wish.set_keymap('<c-right>', function()
    local cursor = wish.buffer:find('%f[%s]', wish.cursor + 2)
    wish.cursor = (cursor or #wish.buffer + 1) - 1
end)

-- there ought to be a better way of doing this
wish.set_keymap('<c-d>', function()
    if #wish.buffer == 0 then
        wish.buffer = 'exit'
        wish.accept_line()
    end
end)

local messages = {}
wish.set_keymap('<tab>', function()
    if msg then
        msg:remove()
        msg = nil
    end

    local text = {}

    for chunk in wish.get_completions() do
        for _, cmatch in ipairs(chunk) do
            table.insert(text, tostring(cmatch))
        end

        msg = msg or wish.show_message{
            align = 'Right',
            fg = 'blue',
            -- italic = true,
            border = {
                fg = 'white',
                type = 'Rounded',
            },
        }
        msg:set_options{text = table.concat(text, '\n')}
        wish.redraw()
    end

end)

wish.set_keymap('<f12>', function()
    local msg = wish.show_message{text="hello world " .. math.random()}
    -- msg:set_text_weight('Bold');
    wish.redraw()
end)
