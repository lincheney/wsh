local M = {}

local SELECTION = require('wish/selection-widget')
local HISTORY_MENU = {}
local HISTORY_SEARCH = {}

local show_history
function show_history(widget, filter, data)
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
            wish.goto_history(history[result].histnum)
            wish.redraw()
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

