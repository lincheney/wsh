local M = {}

local SIZE = 9
local selection_widget = nil
local state = nil
local match_fg = 'yellow'

function M.stop()
    wish.remove_event_callback(state.event_id)
    state = nil
    M.hide()
end

-- poor mans fuzzy find
local function score(x, y)
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
        state.filter_text = nil
        if state.reverse then
            state.filtered = {}
            for i = #state.text, 1, -1 do
                table.insert(state.filtered, state.text[i])
            end
        else
            state.filtered = state.text
        end

    elseif filter ~= state.filter_text then
        state.filter_text = filter
        state.filtered = {}
        for i = 1, #state.text do
            local s = score(state.text[i].text, filter)
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
            table.insert(text, state.filtered[i])
            state.filtered[i].bg = bg
            if i == state.selected then
                state.real_selected = i
            end

        else
            for j = 1, #state.filtered[i][1] do
                table.insert(text, state.filtered[i][1][j])
                state.filtered[i][1][j].bg = bg
            end
            if i == state.selected then
                state.real_selected = state.filtered[i][2]
            end
        end
    end

    if selection_widget and wish.check_message(selection_widget) then
        wish.set_message{id = selection_widget, text = #text > 0 and text or ''}
        wish.redraw()
    end
end

function M.show(opts)
    if opts.data and state and opts.data ~= state.data then
        M.stop()
    end

    if not state then
        state = {
            buffer = wish.get_buffer(),
            filter = true,
            event_id = wish.add_event_callback('buffer_change', function()
                if state.filter then
                    recalc_filter()
                end
            end),
            text = {},
            selected = 1,
            real_selected = nil,
        }
    end

    for _, key in ipairs{'filter', 'data', 'reverse', 'callback', 'selected', 'text'} do
        if opts[key] ~= nil then
            state[key] = opts[key]
            opts[key] = nil
        end
    end

    opts.height = 'max:'..(SIZE + 2)
    opts.hidden = false
    if selection_widget and not wish.check_message(selection_widget) then
        selection_widget = nil
    end
    opts.id = selection_widget
    selection_widget = wish.set_message(opts)

    recalc_filter()
    return selection_widget
end

function M.hide()
    if selection_widget and wish.check_message(selection_widget) then
        wish.set_message{id = selection_widget, hidden = true}
        wish.redraw()
    end
end

function M.add_lines(lines)
    for i = 1, #lines do
        table.insert(state.text, lines[i])
    end
    state.filter_text = nil
    recalc_filter()
end

function M.up()
    if M.is_active() then
        state.selected = state.selected - 1
        recalc_filter()
    end
end

function M.down()
    if M.is_active() then
        state.selected = state.selected + 1
        recalc_filter()
    end
end

function M.insert()
    state.selected = state.selected + 1
    recalc_filter()
end

function M.is_active()
    return state ~= nil
end

function M.get_data()
    return state and state.data
end

function M.trigger()
    if M.is_active() and state.callback then
        state.callback(state.real_selected)
    end
end

wish.add_event_callback('accept_line', function()
    if state then
        M.stop()
    end
end)

return M
