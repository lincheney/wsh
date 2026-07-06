local M = {}

local function extract_text(line)
    if type(line) == 'string' then
        return line
    elseif type(line.text) == 'string' then
        return line.text
    else
        local parts = {}
        for i = 1, #line do
            table.insert(parts, line[i].text)
        end
        return table.concat(parts)
    end
end

-- poor mans fuzzy find
-- it basically just looks for contiguous matches of chars in needle in haystack
-- does not find optimal match
local function score(haystack, needle)
    local last = 0
    local start = 0
    local text = {}
    for i = 1, #needle do
        local ix = string.find(haystack, needle[i], last + 1, true)
        if not ix then
            return
        elseif ix > last + 1 then
            if last > 0 then
                table.insert(text, start)
                table.insert(text, last)
            end
            start = ix - 1
        end
        last = ix
    end
    if last > 0 then
        table.insert(text, start)
        table.insert(text, last)
    end
    return text
end

local function clamp_cursor(plugin)
    plugin.selected = math.max(0, math.min(plugin.selected, #plugin.filtered))
end

local function redraw_cursor(plugin)
    wish.scroll_message_to(plugin.widget, math.max(0, plugin.selected - 1))
end

local function move_cursor(plugin, new)
    local old = plugin.selected
    plugin.selected = new
    clamp_cursor(plugin)
    if plugin.selected ~= old then
        wish.redraw_message(plugin.widget)
        redraw_cursor(plugin)
    end
end

local function recalc_filter(plugin)
    local matches = nil

    if plugin.menu_only then
        plugin.filtered = plugin.lines
    else
        local buffer = wish.get_buffer()
        local filter = string.sub(buffer, #plugin.starting_text + 1)

        if not wish.str.startswith(buffer, plugin.starting_text) or string.find(filter, '%s$') then
            plugin.stop()
            return
        end

        if filter ~= plugin.filter_text then
            wish.table.clear(plugin.match_ranges)

            -- text filtering
            plugin.filter_text = filter
            if filter == '' then
                -- no filter
                plugin.filtered = plugin.lines
            else
                local needle = wish.str.graphemes(filter)

                plugin.filtered = {}
                for i = 1, #plugin.lines do
                    local s = score(plugin.text[i], needle)
                    if s then
                        table.insert(plugin.filtered, {s, i})
                    end
                end
                -- sort by score, otherwise index (stable sort)
                table.sort(plugin.filtered, function(a, b)
                    if #a[1] ~= #b[1] then
                        return #a[1] < #b[1]
                    else
                        return a[2] < b[2]
                    end
                end)
                for i = 1, #plugin.filtered do
                    plugin.match_ranges[i] = plugin.filtered[i][1]
                    plugin.filtered[i] = plugin.lines[plugin.filtered[i][2]]
                end
            end
        end

        clamp_cursor(plugin)
    end

    wish.clear_message(plugin.widget)
    wish.set_message{id = plugin.widget, hidden = false, contents = #plugin.filtered > 0 and plugin.filtered or ''}
    redraw_cursor(plugin)
end

local function add_lines(plugin, lines)
    if plugin.inner.is_enabled() and lines then
        for i = 1, #lines do
            table.insert(plugin.lines, lines[i])
            table.insert(plugin.text, extract_text(lines[i]))
        end
        recalc_filter(plugin)
    end
end

function M.new()
    return wish.plugin(function(wish, opts, plugin)

        local style = opts.style or {
            border = {
                fg = 'green',
                type = 'rounded',
            }
        }

        plugin.match_style = opts.match_style or {
            fg = 'yellow',
            underline = true,
        }

        plugin.cursor_style = opts.cursor_style or {
            bg = 'dark_grey'
        }

        plugin.menu_only = opts.menu_only

        plugin.widget = wish.set_message(wish.table.deep_merge({
            hidden = true,
            persist = true,
            max_height = 11,
        }, style))

        wish.add_render_callback(function(widget, lineno)
            if widget == plugin.widget and (plugin.selected == lineno or plugin.match_ranges[lineno]) then
                local tbl = {}
                local ranges = plugin.match_ranges[lineno]
                if ranges then
                    for i = 1, #ranges, 2 do
                        local hl = wish.table.copy(plugin.match_style)
                        hl.start_column = ranges[i]
                        hl.end_column = ranges[i+1]
                        table.insert(tbl, hl)
                    end
                end
                if plugin.selected == lineno then
                    local hl = wish.table.copy(plugin.cursor_style)
                    hl.start_column = 0
                    hl.end_column = wish.MAXNUM
                    table.insert(tbl, hl)
                end
                return tbl
            end
        end)

        plugin.inner = wish.plugin(function(wish, opts, inner)
            plugin.selected = 0
            plugin.lines = {}
            plugin.text = {}
            plugin.filtered = {}
            plugin.match_ranges = {}
            plugin.starting_text = not opts.menu_only and wish.get_buffer()
            wish.clear_message(plugin.widget)

            if not plugin.menu_only then
                wish.add_event_callback('buffer_change', function()
                    recalc_filter(plugin)
                end)
            end
        end)

        function plugin.start(source, on_accept)
            plugin.on_accept = opts.on_accept
            plugin.inner.enable()
            if type(source) == 'table' then
                add_lines(plugin, source)
            elseif type(source) == 'function' then
                wish.schedule(function()
                    for lines in source() do
                        if not plugin.inner.is_enabled() then
                            break
                        end
                        add_lines(plugin, lines)
                    end
                end)
            elseif source then
                error('expected source to be array of lines or function, got: ' .. type(source))
            end
        end

        function plugin.stop()
            plugin.inner.disable()
            wish.set_message{id = plugin.widget, hidden = true}
        end

        -- function plugin.clear()
            -- if plugin.inner.is_enabled() then
                -- for i = #state.lines, 1, -1 do
                    -- state.lines[i] = nil
                -- end
                -- recalc_filter()
            -- end
        -- end

        function plugin.accept()
            if plugin.inner.is_enabled() then
                if plugin.on_accept and plugin.selected > 0 then
                    local selected = plugin.filtered[plugin.selected]
                    if type(selected) == 'table' then
                        selected = table.concat(wish.table.map(selected, function(x) return x.text end))
                    end
                    plugin.on_accept(selected)
                end
                plugin.stop()
            end
        end

        -- function M.reload()
            -- if M.is_enabled() then
                -- local callback = state.reload_callback
                -- M.stop()
                -- if callback then
                    -- callback()
                -- end
            -- end
        -- end

        function plugin.up()
            move_cursor(plugin, plugin.selected - 1)
        end

        function plugin.down()
            move_cursor(plugin, plugin.selected + 1)
        end

        wish.set_keymap('<up>', function()
            plugin.up()
        end)

        wish.set_keymap('<down>', function()
            plugin.down()
        end)

        wish.add_event_callback('accept_line', function()
            plugin.stop()
        end)

    end)
end

return M
