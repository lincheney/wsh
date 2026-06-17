return wish.plugin(function(wish, opts, plugin)

    local QUERY = require('wish.syntax-query')
    local NAMESPACE = wish.add_buf_highlight_namespace()

    local styles = opts.styles or {
        {fg = 'lightblue' },
        {fg = 'lightred' },
        {fg = 'lightgreen' },
        {fg = 'lightyellow' },
    }
    local unmatched_style = opts.unmatched_style or {
        bg = 'red'
    }

    local MATCHING_BRACKET = {
        ['('] = ')',
        ['{'] = '}',
    }
    local RULES = {
        { {regex = '^[(){}]$', highlight = true} },
        { {kind = 'heredoc_end', highlight = true} },
        {
            {kind = 'DINANG|DINANGDASH'},
            {kind = 'STRING', highlight = true},
        },
    }

    QUERY.add_buffer_callback(function(tokens, str)
        if not plugin.is_enabled() then
            return true
        end

        wish.clear_buf_highlights(NAMESPACE)

        local brackets = {}
        QUERY.apply_rules(RULES, tokens, str, function(matches)
            for i = 1, #matches do
                if matches[i][1].highlight then
                    table.insert(brackets, matches[i][2])
                end
            end
        end)
        wish.table.sort_by(brackets, 'start')
        local stack = {}
        local items = {}

        -- match up the brackets
        for i = 1, #brackets do
            local tokstr = string.sub(str, brackets[i].start, brackets[i].finish)
            local expected = tokstr
            local rhs
            if brackets[i].kind == 'heredoc_end' then
                expected = 'heredoc'
            elseif brackets[i].kind == 'STRING' then
                rhs = 'heredoc'
            else
                rhs = MATCHING_BRACKET[tokstr]
            end
            table.insert(items, {brackets[i], rhs})

            if rhs then
                table.insert(stack, items[#items])
            else
                for j = #stack, 1, -1 do
                    if expected == stack[j][2] then
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
                hl = wish.table.merge(wish.table.copy(styles[level % #styles + 1]), hl)
                if not left then
                    level = level - 1
                end
            else
                hl = wish.table.merge(wish.table.copy(unmatched_style), hl)
            end
            wish.add_buf_highlight(hl)
        end
    end)

end)
