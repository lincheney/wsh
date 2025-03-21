local M = {}

local SIZE = 9
local selection_widget = nil
local state = nil
local match_fg = 'yellow'

function M.stop()
    wish.remove_event_callback(state.event_id)
    wish.del_keymap_layer(state.keymap_layer)
    state = nil

    if selection_widget and wish.check_message(selection_widget) then
        wish.set_message{id = selection_widget, hidden = true}
        wish.redraw()
    end
end

-- poor mans fuzzy find
-- it basically just looks for contiguous matches of chars in needle in haystack
-- does not find optimal match
local function score(haystack, needle)
    local last = 1
    local start = 0
    local text = {}
    for i = 1, wish.str.len(y) do
        local ix = string.find(x, wish.str.get(y, i-1), last, true)
        if not ix then
            return
        elseif ix > last then
            if last > 1 then
                table.insert(text, {text = x:sub(start, last-1), fg = match_fg})
            end
            table.insert(text, {text = x:sub(last, ix-1)})
            start = ix
        end
        last = ix + 1
    end
    table.insert(text, {text = x:sub(start, last-1), fg = match_fg})
    if last <= #x then
        table.insert(text, {text = x:sub(last)})
    end
    return text
end

local function recalc_filter()
    local buffer = wish.get_buffer()
    local filter = buffer:sub(#state.buffer + 1)

    if state.filter and (buffer:sub(1, #state.buffer) ~= state.buffer or filter:find('%s$')) then
        M.stop()
        return
    end

    if #filter == 0 or not state.filter then
        -- no filtering
        state.filter_text = nil
        if state.reverse then
            state.filtered = {}
            for i = #state.lines, 1, -1 do
                table.insert(state.filtered, state.lines[i])
            end
        else
            state.filtered = state.lines
        end

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

    local text = {}
    for i = top, bottom do
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

function trigger_change_callback()
    if state.change_callback and state.real_selected then
        state.change_callback(state.real_selected, state.lines[state.real_selected])
    end
end

-- opts:
--      filter: bool: whether to do text filtering
--      reverse: bool: show in reverse
--      accept_callback: function: function to call when selection made
--      change_callback: function: function to call when selection changed
--      selected: int: selected index
--      lines: string[]: lines fot text to select
--      data: any
function M.start(opts)

    if state and opts.data ~= state.data then
        state = {
            data = opts.data,
            buffer = wish.get_buffer(),
            filter = true,
            event_id = wish.add_event_callback('buffer_change', function()
                if state.filter then
                    recalc_filter()
                end
            end),
            keymap_layer = wish.add_keymap_layer(),
            lines = opts.lines or {},
            selected = 1,
            real_selected = nil,
        }

        wish.set_keymap('<up>', M.up, state.keymap_layer)
        wish.set_keymap('<down>', M.down, state.keymap_layer)
        wish.set_keymap('<tab>', M.accept, state.keymap_layer)
    end

    local old_selected = state.selected

    state.accept_callback = opts.accept_callback or state.accept_callback
    state.change_callback = opts.change_callback or state.change_callback
    state.selected = opts.selected or state.selected

    opts.height = 'max:'..(SIZE + 2)
    opts.hidden = false

    if selection_widget and not wish.check_message(selection_widget) then
        selection_widget = nil
    end
    opts.id = selection_widget
    selection_widget = wish.set_message(opts)

    recalc_filter()
    if old_select ~= state.selected then
        trigger_change_callback()
    end
end

function M.add_lines(lines)
    if lines then
        for i = 1, #lines do
            table.insert(state.lines, lines[i])
        end
        -- state.filter_text = nil
        recalc_filter()
    end
end

function M.clear()
    for i = #state.lines, 1, -1 do
        state.lines[i] = nil
    end
    recalc_filter()
end

function M.accept()
    if state.accept_callback then
        state.accept_callback(state.real_selected)
    end
    M.stop()
end

function M.up()
    state.selected = state.selected - 1
    recalc_filter()
    trigger_change_callback()
end

function M.down()
    state.selected = state.selected + 1
    recalc_filter()
    trigger_change_callback()
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
