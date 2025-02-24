local M = {}

local SIZE = 5

local function show_history(size)
    local index, histnums, history = wish.get_history()

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
    for i = math.min(#history, start + size - 1), start, -1 do
        table.insert(text, {text = history[i] .. '\n'})
        if i == ix then
            text[#text].bg = 'darkgrey'
        end
    end

    require('wish/selection-widget').show{
        align = 'Left',
        border = {
            fg = 'green',
            type = 'Rounded',
            title = 'history',
        },
        height = 'max:'..(size + 2),
        text = text or '',
    }
end


function M.history_up()
    local index = wish.get_history_index()
    local newindex, value = wish.get_prev_history(index)
    if index ~= newindex and newindex then
        wish.goto_history(newindex)
        show_history(SIZE)
        wish.redraw()
    end
end

function M.history_down()
    local index = wish.get_history_index()
    local newindex, value = wish.get_next_history(index)
    if index ~= newindex then
        wish.goto_history(newindex or index + 1)
        show_history(SIZE)
        wish.redraw()
    end
end

function M.history_search()
    show_history(SIZE)
end

return M

