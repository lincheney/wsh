local cut_contents = nil
local function cut_buffer(len)
    local buffer = wish.get_buffer()
    local cursor = wish.str.to_byte_pos(buffer, wish.get_cursor()) or #buffer
    cut_contents = buffer:sub(cursor + 1, len and cursor + len)
    wish.set_buffer('', len)
end

wish.set_keymap('<bs>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        wish.set_cursor(cursor-1)
        wish.set_buffer('', 1)
    end
end)

wish.set_keymap('<delete>', function()
    wish.set_buffer('', 1)
end)

wish.set_keymap('<c-u>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        wish.set_cursor(0)
        cut_buffer(cursor)
    end
end)

wish.set_keymap('<c-k>', function()
    cut_buffer(nil)
end)

wish.set_keymap('<c-y>', function()
    if cut_contents then
        wish.set_buffer(cut_contents, 0)
    end
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
        cursor = wish.str.to_byte_pos(buffer, cursor) or #buffer
        local start = buffer:sub(1, cursor):find('%S+%s*$')
        if start then
            start = wish.str.from_byte_pos(buffer, start - 1)
            wish.set_cursor(start)
            cut_buffer(cursor - start)
        end
    end
end)

wish.set_keymap('<a-bs>', function()
    local cursor = wish.get_cursor()
    if cursor > 0 then
        local buffer = wish.get_buffer()
        cursor = wish.str.to_byte_pos(buffer, cursor) or #buffer
        local start = buffer:sub(1, cursor):find('[^/%s]+[/%s]*$')
        if start then
            start = wish.str.from_byte_pos(buffer, start - 1)
            wish.set_cursor(start)
            cut_buffer(cursor - start)
        end
    end
end)

wish.set_keymap('<c-7>', function() -- same as <c-s-_>
    wish.undo_buffer()
end)
wish.set_keymap('<a-_>', function()
    wish.redo_buffer()
end)

wish.set_keymap('<a-cr>', function()
    wish.set_buffer('\n')
end)

-- there ought to be a better way of doing this
wish.set_keymap('<c-d>', function()
    wish.set_message{text='hello '..wish.get_buffer()}
    wish.redraw()
    if not wish.get_buffer():find('%S') then
        -- wish.exit()
        wish.set_buffer('exit')
        wish.accept_line()
    end
end)

local msg = nil

wish.set_keymap('<tab>', require('wish/completion').complete)
wish.set_keymap('<up>', require('wish/history').history_up)
wish.set_keymap('<down>', require('wish/history').history_down)
wish.set_keymap('<c-p>', require('wish/history').history_up)
wish.set_keymap('<c-n>', require('wish/history').history_down)
wish.set_keymap('<c-r>', require('wish/history').history_search)

wish.set_keymap('<a-v>', function()
    local buffer
    local cursor
    local old_buffer = wish.get_buffer()
    local old_cursor = wish.get_cursor()
    wish.in_param_scope(function()
        for k, v in pairs{
            REGION_ACTIVE = 1,
            BUFFER = old_buffer,
            CURSOR = old_cursor,
            LBUFFER = old_buffer:sub(1, old_cursor),
        } do
            wish.unset_var(k)
            wish.set_var(k, v)
        end

        wish.cmd[[autoload -Uz edit-command-line; edit-command-line]].wait()
        buffer = wish.get_var('BUFFER') or old_buffer
        cursor = wish.get_var('CURSOR') or old_cursor
    end)
    if buffer ~= old_buffer and cursor ~= old_cursor then
        wish.set_cursor(0)
        wish.set_buffer(buffer)
        wish.set_cursor(cursor)
    end
end)

wish.set_keymap('<a-.>', function()
    require('wish.insert-last-word').handler(true)
end)

wish.set_keymap('<a-,>', function()
    require('wish.insert-last-word').handler(false)
end)

wish.set_keymap('<f12>', function()
    local id = wish.set_ansi_message{
        dim = true,
        border = {
            fg = 'blue',
            dim = false,
            type = 'Rounded',
            title = {
                text = ' running ... ',
            },
        },
    }
    wish.schedule(function()
        local proc = wish.async.spawn{
            args = {'bash', '-c', 'for i in {1..10}; do printf "\\rhello world %i\x1b[3%im" $i $i; sleep 0.3; done; echo; echo done'},
            stdout = 'piped',
        }
        while true do
            local stdout = proc.stdout:read()
            wish.pprint("DEBUG(pile)      ".."stdout"..(" = %q\n"):format(stdout))
            if not stdout then
                break
            end
            wish.feed_ansi_message(id, stdout)
            wish.redraw()
        end
        proc:wait()
    end)
    -- local code, stdout = wish.eval[[ls -l --color=always /tmp/]]

    -- wish.set_var("path[${#path[@]}+1]", "hello")
    -- wish.pprint(wish.get_var("path"))
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

local PASTE_NS = wish.add_buf_highlight_namespace()
local clear_paste = nil
wish.add_event_callback('paste', function(data)
    -- insert this into the buffer
    if #data > 0 then
        local id = math.random()
        clear_paste = id
        local cursor = wish.get_cursor()
        local buffer = wish.get_buffer()
        local len = wish.str.len(data)
        local buflen = wish.str.len(buffer)

        local _, prefix = wish.str.to_byte_pos(buffer, cursor)
        prefix = prefix or #buffer
        wish.set_buffer(data, 0)

        -- flash blue for a bit
        wish.add_buf_highlight{namespace = PASTE_NS, fg = 'blue', start = prefix, finish = prefix + len}
        wish.redraw{buffer = true}

        wish.schedule(function()
            wish.async.sleep(500)
            if clear_paste == id then
                wish.clear_buf_highlights(PASTE_NS)
                wish.redraw{buffer = true}
            end
        end)
    end
end)
wish.add_event_callback('accept_line', function()
    wish.clear_buf_highlights(PASTE_NS)
    wish.redraw{buffer = true}
end)

wish.set_status_bar{
    text = 'asd',
    align = 'Center',
    bg = 'darkgrey',
}
wish.redraw()
