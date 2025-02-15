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
wish.set_keymap('<c-w>', function()
    if wish.cursor > 0 then
        local cursor = wish.buffer:sub(1, wish.cursor):find('%S+%s*$')
        wish.buffer = wish.buffer:sub(1, cursor - 1) .. wish.buffer:sub(wish.cursor + 1)
        wish.cursor = (cursor or 1) - 1
    end
end)

-- there ought to be a better way of doing this
wish.set_keymap('<c-d>', function()
    if not wish.buffer:find('%S') then
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
    local last = nil

    for chunk in wish.get_completions() do
        for _, cmatch in ipairs(chunk) do
            table.insert(text, tostring(cmatch))
            last = cmatch
        end

        if #text > 1 then
            msg = msg or wish.show_message{
                align = 'Left',
                fg = 'blue',
                height = 'max:10',
                -- italic = true,
                border = {
                    fg = 'white',
                    type = 'Rounded',
                },
            }
            msg:set_options{text = '...\n' .. table.concat(text, '\n')}
            wish.redraw()
        end
    end

    if msg then
        msg:set_options{text = 'done\n' .. table.concat(text, '\n')}
        wish.redraw()
    end

    if #text == 1 then
        wish.insert_completion(last)
        if msg then
            pcall(function() msg:remove() end)
            msg = nil
        end
        wish.redraw()
    end

end)

wish.set_keymap('<f12>', function()
    error("ARGGHH")
    -- local msg = wish.show_message{text="hello world " .. math.random()}
    -- msg:set_text_weight('Bold');
    -- wish.redraw()
end)
