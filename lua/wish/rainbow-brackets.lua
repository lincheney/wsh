local M = {}

local QUERY = require('wish/syntax-query')
local NAMESPACE = wish.add_buf_highlight_namespace()

local UNMATCHED = { bg = 'red' }
local COLOURS = {
    {fg = 'lightblue' },
    {fg = 'lightred' },
    {fg = 'lightgreen' },
    {fg = 'lightyellow' },
}
local MATCHING_BRACKET = {
    ['('] = ')',
    ['{'] = '}',
}
local RULES = {
    { { regex='^[(){}]$' } },
}

local have_prev_brackets = false
QUERY.add_buffer_callback(function(tokens, str)
    wish.clear_buf_highlights(NAMESPACE)

    local brackets = {}
    QUERY.apply_rules(RULES, tokens, str, function(matches)
        for i = 1, #matches do
            table.insert(brackets, matches[i][2])
        end
    end)
    wish.table.sort_by(brackets, 'start')
    local stack = {}
    local items = {}

    -- match up the brackets
    for i = 1, #brackets do
        local tokstr = string.sub(str, brackets[i].start+1, brackets[i].finish)
        table.insert(items, {brackets[i], MATCHING_BRACKET[tokstr]})

        if MATCHING_BRACKET[tokstr] then
            table.insert(stack, items[#items])
        else
            for j = #stack, 1, -1 do
                if tokstr == stack[j][2] then
                    items[#items][3] = true
                    stack[j][3] = true

                    -- pop off the stack
                    for k = #stack, j, -1 do
                        stack[k] = nil
                    end
                    break
                end
            end
        end
    end

    -- highlighting
    local level = 0
    for i = 1, #items do
        local token = items[i][1]
        local left = items[i][2]
        local matched = items[i][3]
        local hl = {
            start = token.start,
            finish = token.finish,
            namespace = NAMESPACE,
        }
        if matched then
            if left then
                level = level + 1
            end
            wish.table.merge(hl, COLOURS[level % #COLOURS + 1])
            if not left then
                level = level - 1
            end
        else
            wish.table.merge(hl, UNMATCHED)
        end
        wish.add_buf_highlight(hl)
    end

    if have_prev_brackets or #brackets > 0 then
        wish.redraw{buffer = true}
    end
    have_prev_brackets = #brackets > 0
end)

return M
