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
    wish.show_message{text='hello '..wish.buffer}
    wish.redraw()
    if not wish.buffer:find('%S') then
        wish.buffer = 'exit'
        wish.accept_line()
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

local function show_history(size)
    local index, histnums, history = wish.get_history()

    msg = msg and msg:exists() and msg or wish.show_message{
        align = 'Left',
        -- italic = true,
        border = {
            fg = 'green',
            type = 'Rounded',
        },
    }
    size = size or 10

    local ix = 0
    for i = 1, #histnums do
        if histnums[i] == index then
            ix = i
            break
        end
    end

    local start = math.max(1, ix - math.ceil(size / 2) + 1)
    local text = {}
    -- reverse
    for i = math.min(#history, start + size), start, -1 do
        table.insert(text, {text = history[i] .. '\n'})
        if i == ix then
            text[#text].bg = 'darkgrey'
        end
    end
    if #text == 0 then
        msg:remove()
    else
        msg:set_options{
            height = 'max:'..(size + 2),
            text = text,
        }
    end
    wish.redraw()
end

wish.set_keymap('<up>', function()
    local index = wish.get_history_index()
    local newindex, value = wish.get_prev_history(index)
    if index ~= newindex and newindex then
        wish.goto_history(newindex)
        show_history(5)
        wish.redraw()
    end
end)

wish.set_keymap('<down>', function()
    local index = wish.get_history_index()
    local newindex, value = wish.get_next_history(index)
    if index ~= newindex then
        wish.goto_history(newindex or index + 1)
        show_history(5)
        wish.redraw()
    end
end)

wish.set_keymap('<c-r>', function()
    if msg then
        pcall(function() msg:remove() end)
        wish.redraw()
        msg = nil
    end
    show_history(3)
end)

wish.set_keymap('<f12>', function()
    error("ARGGHH")
    -- local msg = wish.show_message{text="hello world " .. math.random()}
    -- msg:set_text_weight('Bold');
    -- wish.redraw()
end)

wish.add_event_callback('key', function(arg)
    error("got a key " .. arg.key)
end)
