local utf8 = require('wish.utf8')

wish.set_keymap('<bs>', function()
    local buffer, cursor = wish.get_buffer()
    if cursor > 1 then
        buffer = utf8.sub(buffer, 1, cursor-2) .. utf8.sub(buffer, cursor)
        wish.set_buffer(buffer, cursor-1)
    end
end)

wish.set_keymap('<delete>', function()
    wish.delete_at_cursor(1)
end)

wish.set_keymap('<c-u>', function()
    local buffer, cursor = wish.get_buffer()
    if cursor > 0 then
        require('wish.killring').push(utf8.sub(buffer, 1, cursor - 1))
        wish.set_buffer(utf8.sub(buffer, cursor), 0)
    end
end)

wish.set_keymap('<c-k>', function()
    local buffer, cursor = wish.get_buffer()
    require('wish.killring').push(utf8.sub(buffer, cursor))
    wish.set_buffer(utf8.sub(buffer, 1, cursor-1))
end)

wish.set_keymap('<c-a>',   function() wish.set_cursor(0) end)
wish.set_keymap('<c-e>',   function() wish.set_cursor(wish.MAXNUM) end)
wish.set_keymap('<home>',  function() wish.set_cursor(0) end)
wish.set_keymap('<end>',   function() wish.set_cursor(wish.MAXNUM) end)
wish.set_keymap('<left>',  function() wish.set_cursor(math.max(0, wish.get_cursor() - 1)) end)
wish.set_keymap('<right>', function() wish.set_cursor(wish.get_cursor() + 1) end)

wish.set_keymap('<c-left>', function()
    local buffer, cursor = wish.get_buffer()
    if cursor > 1 then
        cursor = utf8.sub(buffer, 1, cursor - 1):find('%S+%s*$')
        if cursor then
            wish.set_cursor(cursor)
        end
    end
end)

wish.set_keymap('<c-right>', function()
    local buffer, cursor = wish.get_buffer()
    cursor = utf8.find(buffer, '%f[%s]', cursor+1) or #buffer + 1
    wish.set_cursor(cursor)
end)

wish.set_keymap('<c-w>', function()
    local buffer, cursor = wish.get_buffer()
    if cursor > 1 then
        local start = utf8.sub(buffer, 1, cursor - 1):find('%S+%s*$')
        if start then
            require('wish.killring').push(utf8.sub(buffer, start, cursor - 1))
            buffer = utf8.sub(buffer, 1, start - 1) .. utf8.sub(buffer, cursor)
            wish.set_buffer(buffer, start)
        end
    end
end)

wish.set_keymap('<a-bs>', function()
    local buffer, cursor = wish.get_buffer()
    if cursor > 1 then
        local start = utf8.sub(buffer, 1, cursor - 1):find('[^/%s]+[/%s]*$')
        if start then
            require('wish.killring').push(utf8.sub(buffer, start, cursor))
            buffer = utf8.sub(buffer, 1, start - 1) .. utf8.sub(buffer, cursor)
            wish.set_buffer(buffer, start)
        end
    end
end)

wish.set_keymap('<a-_>', function()
    wish.redo_buffer()
end)

wish.set_keymap('<a-cr>', function()
    wish.insert_at_cursor('\n')
end)

-- wish.set_keymap('<tab>', require('wish/completion').complete)
-- wish.set_keymap('<up>', require('wish/history').history_up)
-- wish.set_keymap('<down>', require('wish/history').history_down)
-- wish.set_keymap('<c-p>', require('wish/history').history_up)
-- wish.set_keymap('<c-n>', require('wish/history').history_down)
-- wish.set_keymap('<c-r>', require('wish/history').history_search)

wish.set_keymap('<a-a>', function()
    -- run in the background
    local buffer = wish.get_buffer()
    wish.append_history(buffer)
    wish.trigger_event_callback('accept_line')
    wish.call_hook_func{'preexec', buffer}
    wish.set_buffer('', 0)
    require('wish.background-job').run_in_background(buffer)
end)

wish.set_keymap('<c-`>', function()
    require('wish.background-job').focus_next_job{key = "`", control = true}
end)
