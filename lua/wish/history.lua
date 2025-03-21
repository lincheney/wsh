local M = {}

local SELECTION = require('wish/selection-widget')
local HISTORY_MENU = {}
local HISTORY_SEARCH = {}

local show_history
function show_history(widget, filter, data)
    local index, histnums, history = wish.get_history()
    local reverse = not filter

    local ix = 0
    local lines = {}
    for i = 1, #history do
        table.insert(lines, {text = history[i]})
        if histnums[i] == index then
            ix = #lines
        end
    end

    widget.start{
        data = data,
        selected = ix,
        lines = lines,
        reverse = reverse,
        filter = filter,
        no_keymaps = not filter,
        accept_callback = filter and function(i)
            wish.goto_history(histnums[i])
            wish.redraw()
        end,
        -- change_callback = not filter and function(i)
            -- wish.goto_history(histnums[#history + 1 - i])
            -- wish.redraw()
        -- end,

        align = 'Left',
        border = {
            fg = 'green',
            type = 'Rounded',
            title = {
                text = 'history',
            },
        },
    }
    widget.add_lines()

end

function M.history_up()
    local index = wish.get_history_index()
    local newindex, value = wish.get_prev_history(index)
    if index ~= newindex and newindex then
        wish.goto_history(newindex)
    end
    show_history(SELECTION.default, false, HISTORY_MENU)
end

function M.history_down()
    local index = wish.get_history_index()
    local newindex, value = wish.get_next_history(index)
    if index ~= newindex then
        wish.goto_history(newindex or index + 1)
    end
    show_history(SELECTION.default, false, HISTORY_MENU)
end


function M.history_search()
    show_history(SELECTION, true, HISTORY_SEARCH)
end

return M

