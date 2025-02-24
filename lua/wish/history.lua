local M = {}

local function show_history()
    local index, histnums, history = wish.get_history()

    local ix = #history + 1
    local text = {}
    -- reverse
    for i = #history, 1, -1 do
        table.insert(text, {text = history[i] .. '\n'})
        if histnums[i] == index then
            ix = #text
        end
    end

    require('wish/selection-widget').show{
        align = 'Left',
        border = {
            fg = 'green',
            type = 'Rounded',
            title = {
                text = 'history',
            },
        },
        selected = ix,
        text = text,
    }
end


function M.history_up()
    local index = wish.get_history_index()
    local newindex, value = wish.get_prev_history(index)
    if index ~= newindex and newindex then
        wish.goto_history(newindex)
        show_history()
        wish.redraw()
    end
end

function M.history_down()
    local index = wish.get_history_index()
    local newindex, value = wish.get_next_history(index)
    if index ~= newindex then
        wish.goto_history(newindex or index + 1)
        show_history()
        wish.redraw()
    end
end

function M.history_search()
    show_history()
end

return M

