local M = {}

local SIZE = 9
local selection_widget = nil
local state = nil
local match_fg = 'yellow'

function M.stop()
    if M.is_active() then
        wish.remove_event_callback(state.event_id)
        wish.del_keymap_layer(state.keymap_layer)
        state = nil

        if selection_widget and wish.check_message(selection_widget) then
            wish.set_message{id = selection_widget, hidden = true}
            wish.redraw()
        end
    end
end

-- poor mans fuzzy find
-- it basically just looks for contiguous matches of chars in needle in haystack
-- does not find optimal match
local function score(haystack, needle)
    local last = 1
    local start = 0
    local text = {}
    for i = 1, wish.str.len(needle) do
        local ix = string.find(haystack, wish.str.get(needle, i-1), last, true)
        if not ix then
            return
        elseif ix > last then
            if last > 1 then
                table.insert(text, {text = haystack:sub(start, last-1), fg = match_fg})
            end
            table.insert(text, {text = haystack:sub(last, ix-1)})
            start = ix
        end
        last = ix + 1
    end
    table.insert(text, {text = haystack:sub(start, last-1), fg = match_fg})
    if last <= #haystack then
        table.insert(text, {text = haystack:sub(last)})
    end
    return text
end

local function recalc_filter()
    if not state then
        return
    end

    local buffer = wish.get_buffer()
    local filter = buffer:sub(#state.buffer + 1)

    if state.filter and (buffer:sub(1, #state.buffer) ~= state.buffer or filter:find('%s$')) then
        state.resume()
        return
    end

    if #filter == 0 or not state.filter then
        -- no filtering
        state.filter_text = nil
        state.filtered = state.lines

    elseif filter ~= state.filter_text then
        -- text filtering
        state.filter_text = filter
        state.filtered = {}
        for i = 1, #state.lines do
            local s = score(state.lines[i].text, filter)
            if s then
                table.insert(state.filtered, {s, i})
            end
        end
        -- sort by score, otherwise index (stable sort)
        table.sort(state.filtered, function(a, b)
            if #a[1] ~= #b[1] then
                return #a[1] < #b[1]
            else
                return a[2] < b[2]
            end
        end)
    end

    -- clamp it
    state.selected = math.max(0, math.min(state.selected, #state.filtered + 1))

    -- center around the selected item
    local bottom = math.min(#state.filtered, state.selected + math.ceil(SIZE / 2) - 1)
    local top = math.max(1, bottom - SIZE + 1)
    bottom = math.min(#state.filtered, top + SIZE - 1)
    local step = 1

    if state.reverse then
        top, bottom = bottom, top
        step = -1
    end

    local text = {}
    for i = top, bottom, step do
        local bg = i == state.selected and 'darkgrey' or nil

        if state.filtered[i].text then
            -- unhighlighted text
            table.insert(text, state.filtered[i])
            state.filtered[i].bg = bg
            if i == state.selected then
                state.real_selected = i
            end

        else
            -- highlighted text
            for j = 1, #state.filtered[i][1] do
                table.insert(text, state.filtered[i][1][j])
                state.filtered[i][1][j].bg = bg
            end
            if i == state.selected then
                state.real_selected = state.filtered[i][2]
            end
        end

        table.insert(text, {text = '\n'})
    end

    if selection_widget and wish.check_message(selection_widget) then
        wish.set_message{id = selection_widget, text = #text > 0 and text or ''}
        wish.redraw()
    end
end

-- opts:
--      filter: bool: whether to do text filtering
--      reverse: bool: show in reverse
--      reload_callback: function: function to call on reload
--      selected: int: selected index
--      lines: string[]: lines fot text to select
--      keymaps: bool: set keymaps
--      data: any
function M.start(opts)

    if not M.is_active() or opts.data ~= state.data then
        state = {
            data = opts.data,
            buffer = wish.get_buffer(),
            filter = true,
            lines = {},
            event_id = wish.add_event_callback('buffer_change', function()
                if state and state.filter then
                    recalc_filter()
                end
            end),
            keymap_layer = wish.add_keymap_layer(),
            selected = 1,
            real_selected = nil,
        }

        if not opts.no_keymaps then
            wish.set_keymap('<up>', M.up, state.keymap_layer)
            wish.set_keymap('<down>', M.down, state.keymap_layer)
            wish.set_keymap('<tab>', M.accept, state.keymap_layer)
            wish.set_keymap('<esc>', M.cancel, state.keymap_layer)
            wish.set_keymap('<c-r>', M.reload, state.keymap_layer)
        end
    end

    state.reload_callback = opts.reload_callback or state.reload_callback
    opts.reload_callback = nil

    if opts.filter ~= nil then
        state.filter = opts.filter
    end
    if opts.reverse ~= nil then
        state.reverse = opts.reverse
    end
    state.selected = opts.selected or state.selected

    opts.height = 'max:'..(SIZE + 2)
    opts.hidden = false

    if selection_widget and not wish.check_message(selection_widget) then
        selection_widget = nil
    end
    opts.id = selection_widget

    local source = opts.source
    opts.source = nil
    selection_widget = wish.set_message(opts)

    if type(source) == 'function' then
        wish.schedule(function()
            for lines in source() do
                M.add_lines(lines)
                if not M.is_active() then
                    break
                end
            end
            if M.is_active() and #state.lines == 0 then
                state.resume()
            end
        end)
    elseif type(source) == 'table' then
        M.add_lines(source)
    end

    recalc_filter()

    local resume, yield = wish.async.promise()
    state.resume = resume
    local result = yield()
    wish.pprint(result)
    M.stop()
    return result
end

function M.add_lines(lines)
    if M.is_active() and lines then
        for i = 1, #lines do
            table.insert(state.lines, lines[i])
        end
        recalc_filter()
    end
end

function M.clear()
    if M.is_active() then
        for i = #state.lines, 1, -1 do
            state.lines[i] = nil
        end
        recalc_filter()
    end
end

function M.accept()
    if M.is_active() then
        state.resume(state.real_selected)
    end
end

function M.cancel()
    if M.is_active() then
        state.resume()
    end
end

function M.reload()
    if M.is_active() then
        local callback = state.reload_callback
        M.stop()
        if callback then
            callback()
        end
    end
end

function M.up()
    if M.is_active() then
        state.selected = math.max(1, state.selected - 1)
        recalc_filter()
    end
end

function M.down()
    if M.is_active() then
        state.selected = state.selected + 1
        recalc_filter()
    end
end

function M.is_active()
    return state ~= nil
end

wish.add_event_callback('accept_line', function()
    if M.is_active() then
        M.stop()
    end
end)

return M
