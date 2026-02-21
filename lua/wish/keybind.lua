local utf8 = require('wish/utf8')

local CUT_CONTENTS = nil

wish.create_dynamic_var('CLIPBOARD', 'string', function()
    return wish.async.spawn{args={'wl-paste'}, foreground=false, stdout='piped'}.stdout:read()
end)

wish.set_keymap('<bs>', function()
    local buffer, cursor = wish.get_buffer()
    if cursor > 0 then
        buffer = utf8.sub(buffer, 1, cursor-2) .. utf8.sub(buffer, cursor)
        wish.set_buffer(buffer, cursor-1)
    end
end)

wish.set_keymap('<delete>', function()
    wish.splice_buffer('', 1)
end)

wish.set_keymap('<c-u>', function()
    local buffer, cursor = wish.get_buffer()
    if cursor > 0 then
        CUT_CONTENTS = utf8.sub(buffer, 1, cursor)
        wish.set_buffer(utf8.sub(buffer, cursor+1), 0)
    end
end)

wish.set_keymap('<c-k>', function()
    local buffer, cursor = wish.get_buffer()
    CUT_CONTENTS = utf8.sub(buffer, cursor+1)
    wish.set_buffer(utf8.sub(buffer, 1, cursor))
end)

wish.set_keymap('<c-y>', function()
    if CUT_CONTENTS then
        wish.splice_buffer(CUT_CONTENTS, 0)
    end
end)

wish.set_keymap('<c-a>',   function() wish.set_cursor(0) end)
-- wish.set_keymap('<home>',  function() wish.set_cursor(0) end)
-- wish.set_keymap('<c-e>',   function() wish.__laggy() end)
wish.set_keymap('<c-f>',   function()
    local x = wish.cmd{args=[[ echo ls; ls -l /dev/fd/; exec ls ]], subshell=true}
    -- local x = wish.async.spawn{args={'cat'}, stdout='null'}
    io.stderr:write("DEBUG(blamer)    ".."x.result:wait()"..(" = %q\n"):format(x:wait()))
    io.stderr:flush()
end)
wish.set_keymap('<end>',   function()
    local buffer, cursor = wish.get_buffer()
    local buflen = utf8.len(buffer) + 1
    if cursor == buflen then
        require('wish/autosuggestions').accept_suggestion()
    else
        wish.set_cursor(buflen)
    end
end)
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
            CUT_CONTENTS = utf8.sub(buffer, start, cursor - 1)
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
            CUT_CONTENTS = utf8.sub(buffer, start, cursor)
            buffer = utf8.sub(buffer, 1, start - 1) .. utf8.sub(buffer, cursor)
            wish.set_buffer(buffer, start)
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
    wish.splice_buffer('\n')
end)

-- there ought to be a better way of doing this
wish.set_keymap('<c-d>', function()
    wish.set_message{text='hello '..wish.get_buffer()}
    wish.redraw()
    if not wish.get_buffer():find('%S') then
        -- wish.exit()
        wish.exit()
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
    local old_buffer, old_cursor = wish.get_buffer()
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
        wish.set_buffer(buffer, cursor)
    end
end)

wish.set_keymap('<a-.>', function()
    require('wish.insert-last-word').handler(true)
end)

wish.set_keymap('<a-,>', function()
    require('wish.insert-last-word').handler(false)
end)

wish.set_keymap('<f12>', function()
    local id = wish.set_message{
        dim = true,
        -- border = {
            -- fg = 'blue',
            -- dim = false,
            -- type = 'Rounded',
            -- title = {
                -- text = ' running ... ',
            -- },
        -- },
    }
    wish.schedule(function()
        local proc = wish.async.spawn{
            -- args = {'bash', '-c', 'for i in {1..10}; do printf "\\rhello world %i\x1b[3%im" $i $i; sleep 0.3; done; echo; echo done'},
            args = {'script', '-fqc', 'top', '/dev/null'},
            -- args = {'echo', '\x1b[7;39;49maslkdjalskjdaklsd'},
            stdout = 'piped',
            stdin = 'null',
            stderr = 'null',
        }
        while true do
            local stdout = proc.stdout:read()
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


wish.set_keymap('<a-a>', function()
    -- run in the background
    local buffer = wish.get_buffer()
    wish.append_history(buffer)
    wish.trigger_event_callback('accept_line')
    wish.call_hook_func{'preexec', buffer}
    wish.set_buffer('', 0)
    require('wish/background-job').run_in_background(buffer)
end)

wish.set_keymap('<c-`>', function()
    require('wish/background-job').focus_next_job{key = "`", control = true}
end)

wish.set_keymap('<c-f>', function()
    require('wish/ft').activate()
end)

local PASTE_NS = wish.add_buf_highlight_namespace()
local clear_paste = nil
wish.add_event_callback('paste', function(data)
    -- insert this into the buffer
    if #data > 0 then
        local id = math.random()
        clear_paste = id
        local buffer, cursor = wish.get_buffer()

        -- paste
        wish.splice_buffer(data, 0)

        -- flash blue for a bit
        wish.add_buf_highlight{
            namespace = PASTE_NS,
            fg = 'blue',
            start = cursor,
            finish = cursor + utf8.len(data) - 1,
        }
        wish.redraw()

        wish.schedule(function()
            wish.sleep(0.5)
            if clear_paste == id then
                wish.clear_buf_highlights(PASTE_NS)
                wish.redraw()
            end
        end)
    end
end)
wish.add_event_callback('accept_line', function()
    wish.clear_buf_highlights(PASTE_NS)
    wish.redraw()
end)

wish.set_status_bar{
    text = 'asd',
    align = 'Center',
    bg = 'darkgrey',
}
wish.redraw()
