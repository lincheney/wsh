local M = {}

local SIZE = 5
local selection_widget = nil
local state = nil
local match_fg = 'yellow'

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
    local filtered = nil

    if #filter > 0 and filter ~= state.filter_text then
        state.filter_text = filter
        filtered = {}
        for i = 1, #state.lines do
            local s = score(state.lines[i].text, filter)
            if s then
                table.insert(filtered, {s, i})
            end
        end
        table.sort(filtered, function(a, b)
            if #a[1] == #b[1] then
                return a[2] < b[2]
            else
                return #a[1] < #b[1]
            end
        end)
        state.selected = 0

    else
        filtered = state.lines
    end

    -- center around the selected item
    local bottom = math.min(#filtered, state.selected + math.ceil(SIZE / 2) - 1)
    local top = math.max(1, bottom - SIZE + 1)
    bottom = math.min(#filtered, top + SIZE - 1)

    local lines = {}
    for i = top, bottom do
        local bg = i == state.selected and 'darkgrey' or nil
        if filtered[i].text then
            table.insert(lines, filtered[i])
            filtered[i].bg = bg
        else
            for j = 1, #filtered[i][1] do
                table.insert(lines, filtered[i][1][j])
                filtered[i][1][j].bg = bg
            end
        end
    end

    if selection_widget:exists() then
        selection_widget:set_options{text = lines}
        wish.redraw()
    end
end

function M.show(opts)
    if not state then
        state = {
            buffer = wish.get_buffer(),
            on_key = wish.add_event_callback('key', recalc_filter),
        }
    end

    if opts.text then
        state.lines = opts.text
        state.filter_text = nil
        opts.text = nil
    end

    state.selected = opts.selected
    opts.selected = nil

    opts.height = 'max:'..(SIZE + 2)
    if selection_widget and selection_widget:exists() then
        selection_widget:set_options(opts)
    else
        selection_widget = wish.show_message(opts)
    end

    recalc_filter()
    return selection_widget
end

function M.hide()
    if selection_widget and selection_widget:exists() then
        selection_widget:set_options{visible = false}
        wish.redraw()
    end
end

wish.add_event_callback('accept_line', function()
    if state then
        wish.remove_event_callback(state.on_key)
    end
end)

return M
