local M = {}

local SELECTION = require('wish.selection-widget')
local HISTORY_MENU = {}
local HISTORY_SEARCH = {}

local selector = SELECTION.new().enable{
    style = {
        border = {
            title_top = 'history',
            fg = 'green',
            type = 'rounded',
        }
    }
}

local function show_history(filter, data)
    local current, history = wish.get_history()
    local reverse = not filter

    local ix = 0
    local lines = {}
    for i = 1, #history do
        table.insert(lines, {text = history[i].text})
        if history[i].histnum == current then
            ix = #lines
        end
    end

    selector.start(lines, function(index)
        if index then
            wish.goto_history(history[index].histnum)
        end
    end)
end

function M.history_up()
    wish.goto_history_relative(-1)
    show_history(SELECTION.default, false, HISTORY_MENU)
end

function M.history_down()
    wish.goto_history_relative(1)
    show_history(SELECTION.default, false, HISTORY_MENU)
end


function M.history_search()
    show_history(SELECTION, true, HISTORY_SEARCH)
end

return M

