wish.set_keymap('<bs>', function()
    local cursor = wish.cursor
    if cursor > 0 then
        local buffer = wish.buffer
        wish.buffer = buffer:sub(1, cursor - 1) .. buffer:sub(cursor + 1)
        cursor = cursor - 1
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
    else
        io.stderr:write("DEBUG(hull)      ".."wish.buffer"..(" = %q\n"):format(wish.buffer))
    end
end)

local msg = nil

wish.set_keymap('<tab>', function()
    if msg then
        pcall(function() msg:remove() end)
        wish.redraw()
        msg = nil
    end

    local text = {}
    local last = nil

    for chunk in wish.get_completions() do
        for _, cmatch in ipairs(chunk) do
            table.insert(text, tostring(cmatch))
            last = cmatch
        end

        if #text > 0 then
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
            msg:set_options{text = '...' .. #text .. '\n' .. table.concat(text, '\n')}
            wish.redraw()
        end
    end

    if msg then
        msg:set_options{text = 'done ' .. #text .. '\n' .. table.concat(text, '\n')}
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

wish.set_keymap('<up>', function()
    local index = wish.get_history_index()
    local newindex, value = wish.get_prev_history(index)
    if index ~= newindex then
        wish.goto_history(newindex)
        wish.redraw()
    end
end)

wish.set_keymap('<down>', function()
    local index = wish.get_history_index()
    local newindex, value = wish.get_next_history(index)
    if index ~= newindex then
        wish.goto_history(newindex or index + 1)
        wish.redraw()
    end
end)

wish.set_keymap('<c-r>', function()
    if msg then
        pcall(function() msg:remove() end)
        wish.redraw()
        msg = nil
    end

    local index, _, history = wish.get_history()

    msg = wish.show_message{
        align = 'Left',
        height = 'min:10',
        -- italic = true,
        border = {
            fg = 'green',
            type = 'Rounded',
        },
        text = table.concat(history, '\n'),
    }
    wish.redraw()
end)

wish.set_keymap('<f12>', function()
    error("ARGGHH")
    -- local msg = wish.show_message{text="hello world " .. math.random()}
    -- msg:set_text_weight('Bold');
    -- wish.redraw()
end)
