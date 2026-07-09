local M = {}

local SELECTION = require('wish.selection-widget')

return wish.plugin(function(wish, opts, plugin)

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

    local selector_keybinds = opts.selector_keybinds or {
        ['<enter>'] = 'accept',
        ['<up>'] = 'up',
        ['<down>'] = 'down',
    }

    function plugin.start()
        local current, history = wish.get_history()

        local ix = 0
        local lines = {}
        for i = 1, #history do
            table.insert(lines, {text = history[i].text})
            if history[i].histnum == current then
                ix = #lines
            end
        end

        local opts = {
            keybinds = selector_keybinds
        }
        selector.start(opts, lines, function(index)
            if index then
                wish.goto_history(history[index].histnum)
            end
        end)
    end

    -- function M.history_up()
        -- wish.goto_history_relative(-1)
        -- show_history(SELECTION.default, false, HISTORY_MENU)
    -- end
--
    -- function M.history_down()
        -- wish.goto_history_relative(1)
        -- show_history(SELECTION.default, false, HISTORY_MENU)
    -- end

end)
