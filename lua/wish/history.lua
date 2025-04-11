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

    wish.schedule(function()
        local result = widget.start{
            data = data,
            selected = filter and math.max(1, ix) or ix,
            source = lines,
            reverse = reverse,
            filter = filter,
            no_keymaps = not filter,
            reload_callback = filter and function()
                show_history(widget, filter, data)
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

        if filter and result then
            wish.goto_history(histnums[result])
            wish.redraw()
        end
    end)
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

