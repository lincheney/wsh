local M = {}
local SELECTION = require('wish/selection-widget')
local COMPLETION = {}

function M.complete()
    local all_matches = {}
    SELECTION.show{
        align = 'Left',
        border = {
            fg = 'magenta',
            type = 'Rounded',
            title = {
                text = 'completion ...',
            },
        },
        selected = 1,
        data = COMPLETION,
        callback = function(i)
            if all_matches[i] then
                wish.insert_completion(all_matches[i])
                SELECTION.stop()
            end
        end,
    }

    local comp = wish.get_completions()
    for chunk in comp do
        if SELECTION.get_data() ~= COMPLETION then
            comp:cancel()
            return
        end

        local text = {}
        for _, cmatch in ipairs(chunk) do
            table.insert(all_matches, cmatch)
            table.insert(text, {text = tostring(cmatch) .. '\n'})
        end

        if #text > 0 then
            SELECTION.add_lines(text)
        end
    end

    if #all_matches == 1 then
        wish.insert_completion(all_matches[1])
        SELECTION.stop()
    else
        -- indicate we are finished
        SELECTION.show{border = {title = {text = 'completion' }}}
    end

end

return M
