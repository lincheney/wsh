wish.set_keymap('<bs>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        wish.set_buffer(wish.str.set(wish.get_buffer(), nil, cursor-1, cursor))
        wish.set_cursor(cursor-1)
    end
end)

wish.set_keymap('<delete>', function()
    local cursor = wish.get_cursor()
    local buffer = wish.get_buffer()
    buffer = wish.str.set(buffer, nil, cursor, cursor+1)
    wish.set_buffer(buffer)
end)

wish.set_keymap('<c-u>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        wish.set_buffer(wish.str.set(wish.get_buffer(), nil, 0, cursor))
        wish.set_cursor(0)
    end
end)

wish.set_keymap('<c-k>', function()
    local cursor = wish.get_cursor()
    local buffer = wish.get_buffer()
    buffer = wish.str.set(buffer, nil, cursor, #buffer)
    wish.set_buffer(buffer)
    wish.set_cursor(wish.str.len(buffer))
end)

wish.set_keymap('<c-a>',   function() wish.set_cursor(0) end)
wish.set_keymap('<home>',  function() wish.set_cursor(0) end)
wish.set_keymap('<c-e>',   function() wish.set_cursor(wish.str.len(wish.get_buffer())) end)
wish.set_keymap('<end>',   function() wish.set_cursor(wish.str.len(wish.get_buffer())) end)
wish.set_keymap('<left>',  function() wish.set_cursor(math.max(0, wish.get_cursor() - 1)) end)
wish.set_keymap('<right>', function() wish.set_cursor(wish.get_cursor() + 1) end)

wish.set_keymap('<c-left>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        local buffer = wish.get_buffer()
        cursor = wish.str.to_byte_pos(buffer, cursor)
        cursor = buffer:sub(1, cursor):find('%S+%s*$')
        wish.set_cursor(wish.str.from_byte_pos(buffer, (cursor or 1) - 1))
    end
end)

wish.set_keymap('<c-right>', function()
    local buffer = wish.get_buffer()
    local cursor = wish.str.to_byte_pos(buffer, wish.get_cursor()) or #buffer
    cursor = buffer:find('%f[%s]', cursor + 2)
    wish.set_cursor(wish.str.from_byte_pos(buffer, (cursor or #buffer + 1) - 1) or #buffer)
end)

wish.set_keymap('<c-w>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        local buffer = wish.get_buffer()
        local start = buffer:sub(1, cursor):find('%S+%s*$')
        if start then
            start = wish.str.to_byte_pos(buffer, start - 1)
            wish.set_buffer(wish.str.set(buffer, nil, start, cursor))
            wish.set_cursor(start)
        end
    end
end)

wish.set_keymap('<a-bs>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        local buffer = wish.get_buffer()
        local start = buffer:sub(1, cursor):find('[^/%s]+[/%s]*$')
        if start then
            start = wish.str.to_byte_pos(buffer, start - 1)
            wish.set_buffer(wish.str.set(buffer, nil, start, cursor))
            wish.set_cursor(start)
        end
    end
end)

-- there ought to be a better way of doing this
wish.set_keymap('<c-d>', function()
    wish.set_message{text='hello '..wish.get_buffer()}
    wish.redraw()
    if not wish.get_buffer():find('%S') then
        wish.set_buffer('exit')
        wish.accept_line()
    end
end)

local msg = nil

wish.set_keymap('<tab>', function()
    if require('wish/selection-widget').is_active() then
        require('wish/selection-widget').trigger()
    else
        require('wish/completion').complete()
    end
end)
wish.set_keymap('<up>', require('wish/history').history_up)
wish.set_keymap('<down>', require('wish/history').history_down)
wish.set_keymap('<c-p>', require('wish/history').history_up)
wish.set_keymap('<c-n>', require('wish/history').history_down)
wish.set_keymap('<c-r>', require('wish/history').history_search)

wish.set_keymap('<f12>', function()
    wish.set_var("path[${#path[@]}+1]", "hello")
    wish.pprint(wish.get_var("path"))
    -- local complete, starts, ends, kinds = wish.parse(wish.get_buffer())
    -- wish.pprint(complete)
    -- wish.pprint(strings)
    -- wish.pprint(kinds)
    -- wish.cmd[[x=3]]:wait()
    -- error("ARGGHH")
    -- local msg = wish.set_message{text="hello world " .. math.random()}
    -- msg:set_text_weight('Bold');
    -- wish.redraw()
end)

wish.add_event_callback('key', function(arg)
    -- error("got a key " .. arg.key)
end)

wish.add_event_callback('paste', function(data)
    -- insert this into the buffer
    if #data > 0 then
        local cursor = wish.get_cursor()
        local buffer = wish.get_buffer()
        local len = wish.str.len(data)
        local buflen = wish.str.len(buffer)
        wish.set_buffer((wish.str.get(buffer, 0, cursor) or '') .. data .. (wish.str.get(buffer, cursor, buflen) or ''))
        wish.set_cursor(cursor + len)
        wish.redraw()
    end
end)
