local M = {}

local SELECTION = require('wish/selection-widget')
local HISTORY_MENU = {}
local HISTORY_SEARCH = {}

local function show_history(data)
    local index, histnums, history = wish.get_history()

    local ix = 0
    local text = {}
    for i = 1, #history do
        table.insert(text, {text = history[i] .. '\n'})
        if histnums[i] == index then
            ix = #text
        end
    end

    if data == HISTORY_MENU then
        ix = #history + 1 - ix
    end

    SELECTION.show{
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
        reverse = data == HISTORY_MENU,
        filter = data == HISTORY_SEARCH,
        data = data,
        callback = function(i)
            wish.goto_history(histnums[i])
            SELECTION.stop()
        end,
    }
end


function M.history_up()
    if not SELECTION.is_active() or SELECTION.get_data() == HISTORY_MENU then
        local index = wish.get_history_index()
        local newindex, value = wish.get_prev_history(index)
        if index ~= newindex and newindex then
            wish.goto_history(newindex)
            if SELECTION.is_active() then
                SELECTION.up()
            else
                show_history(HISTORY_MENU)
            end
        end
        return true
    end
end

function M.history_down()
    if not SELECTION.is_active() or SELECTION.get_data() == HISTORY_MENU then
        local index = wish.get_history_index()
        local newindex, value = wish.get_next_history(index)
        if index ~= newindex then
            wish.goto_history(newindex or index + 1)
            if SELECTION.is_active() then
                SELECTION.down()
            else
                show_history(HISTORY_MENU)
            end
        end
        return true
    end
end

function M.history_search()
    show_history(HISTORY_SEARCH)
end

return M

