local M = {}

local SELECTION = require('wish/selection-widget')
local HISTORY_MENU = {}
local HISTORY_SEARCH = {}

local function show_history(widget, filter, data)
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

    if reverse then
        ix = #history + 1 - ix
    end

    widget.start{
        data = data,
        selected = ix,
        lines = lines,
        reverse = reverse,
        filter = filter,
        accept_callback = filter and function(i)
            wish.goto_history(histnums[i])
            wish.redraw()
        end,
        change_callback = not filter and function(i)
            wish.goto_history(histnums[#history + 1 - i])
            wish.redraw()
        end,

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
    show_history(SELECTION.default, false, HISTORY_MENU)
    SELECTION.default.up()
end

function M.history_down()
    show_history(SELECTION.default, false, HISTORY_MENU)
    SELECTION.default.down()
end

function M.history_search()
    show_history(SELECTION.fzf, true, HISTORY_SEARCH)
end

return M

