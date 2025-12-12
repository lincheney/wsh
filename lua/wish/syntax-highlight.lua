local prev_buffer = nil
local prev_complete = false

local NAMESPACE = wish.add_buf_highlight_namespace()

local function debug_tokens(tokens, buffer)
    local x = {}
    for i = 1, #tokens do
        x[i] = {buffer:sub(tokens[i].start+1, tokens[i].finish), tokens[i].kind}
        if tokens[i].nested then
            x[i][3] = debug_tokens(tokens[i].nested, buffer)
        end
    end
    return x
end

local HL = {
    normal = {
        fg = 'reset',
        bg='reset',
        bold=false,
        dim=false,
        italic=false,
        underline=false,
        strikethrough=false,
        reversed=false,
        blink=false,
    },
    flag = {fg = '#ffaaaa'},
    string = {fg = '#ffffaa', bg='#333300'},
    heredoc_tag = {fg = 'lightblue', bold = true},
    variable = {fg = 'magenta'},
    command = {fg = 'lightgreen', bold = true},
    func = {fg = 'yellow'},
    keyword = {fg = 'red'},
    punctuation = {fg = 'cyan'},
    comment = {fg = 'grey'},
    error = {bg = 'red'},
}

local RULES = {
    -- comments
    { {hl='comment', kind='comment'} },
    -- punctuation
    { {hl='punctuation', pat='^%W+$'} },
    { {hl='flag', kind='STRING', pat='^%-'} },
    -- strings
    {
        {hl='string', kind={'Dnull', 'Snull'}},
        {hl='string', not_kind={'Dnull', 'Snull'}, mod='*'},
        {hl='string', kind={'Dnull', 'Snull'}, mod='?'},
    },
    -- heredocs
    { {hl='string', kind='heredoc_body'} },
    { {hl='heredoc_tag', kind={'heredoc_open_tag', 'heredoc_close_tag'}} },
    -- reset highlight on substitutions in strings
    { {kind='STRING', contains={ {hl='normal', kind='substitution'} } } },
    {
        {hl='variable', kind={'Qstring', 'String'}},
        {hl='variable', kind='Inbrace'},
        {hl='variable', mod='*?'},
        {hl='variable', kind='Outbrace'},
    },
    -- this will match the first string then consume the rest
    {
        {hl='command', kind='STRING'},
        {not_kind={'SEPER', 'BAR', 'DBAR', 'AMPER', 'DAMPER', 'BARAMP', 'AMPERBANG', 'SEMIAMP', 'SEMIBAR'}, mod='*'},
    },
    -- but reset highlights on these
    { {kind='redirect', contains={ {hl='normal', kind='STRING'} }} },
    -- function
    { {kind='function', contains={ {hl='func', kind='FUNC'}, {hl='func', kind='STRING', mod='?'} }} },
    { {kind='function', contains={ {mod='^'}, {hl='func', kind='STRING'} }} },
    -- keywords
    { {hl='keyword', kind={'CASE', 'COPROC', 'DOLOOP', 'DONE', 'ELIF', 'ELSE', 'ZEND', 'ESAC', 'FI', 'FOR', 'FOREACH', 'FUNC', 'IF', 'NOCORRECT', 'REPEAT', 'SELECT', 'THEN', 'TIME', 'UNTIL', 'WHILE', 'TYPESET'} } },
    -- unmatched brackets
    { { hl='error', pat='^%($' }, { not_pat='%)', mod='*' }, { mod='$' } },
    { { hl='error', pat='^%{$' }, { not_pat='%}', mod='*' }, { mod='$' } },
}

local apply_highlight_seq

local function append_highlights(highlights, new)
    if new and #new > 0 then
        for i = 1, #new do
            table.insert(highlights, new[i])
        end
    end
end

local function check_matcher(matcher, func)
    if type(matcher) == 'table' then
        return wish.iter(matcher):any(function(k, v) return func(v) end)
    else
        return func(matcher)
    end
end

local function apply_highlight_matcher(matcher, token, str)
    if matcher.kind and not check_matcher(matcher.kind, function(x) return x == token.kind end) then
        return
    end
    if matcher.not_kind and check_matcher(matcher.not_kind, function(x) return x == token.kind end) then
        return
    end
    if matcher.pat or matcher.not_pat then
        local tokstr = string.sub(str, token.start+1, token.finish)
        if matcher.pat and not check_matcher(matcher.pat, function(x) return string.find(tokstr, x) end) then
            return
        end
        if matcher.not_pat and check_matcher(matcher.not_pat, function(x) return string.find(tokstr, x) end) then
            return
        end
    end

    local highlights = {}
    if matcher.hl then
        local hl = wish.iter(HL[matcher.hl]):copy()
        hl.start = token.start
        hl.finish = token.finish
        hl.namespace = NAMESPACE
        table.insert(highlights, hl)
    end

    if matcher.contains then
        if not token.nested then
            -- matcher asserts nested tokens but there aren't any
            return
        end
        local hl = apply_highlight_seq(matcher.contains, token.nested, str, false)
        if not hl then
            -- nested rules don't match
            return
        end
        append_highlights(highlights, hl)
    end

    return highlights
end

local function apply_highlight_seq_at(seq, seq_index, tokens, str, token_index)
    -- try to apply the seq[seq_index:] at tokens[token_index:] and return the end index
    local highlights = {}
    local non_greedy = {}
    local matcher = seq[seq_index]
    local mod = matcher and matcher.mod

    while seq_index <= #seq do
        if mod == '*' or mod == '*?' or mod == '?' or mod == '??' then
            -- try the next matcher, non greedy
            local hl, index = apply_highlight_seq_at(seq, seq_index+1, tokens, str, token_index)
            if hl then
                if mod == '*?' or mod == '??' then
                    append_highlights(highlights, hl)
                    -- we wanted non greedy, so return it now
                    return highlights, index
                end

                hl = wish.iter(highlights):chain(hl):collect()
                -- non greedy match when we wanted greedy, save for later in case the greedy match doesn't work
                non_greedy = {hl, index}
            end
        end

        local token = tokens[token_index]
        local next_matcher = false
        local hl = nil
        if mod == '$' then
            if token then
                -- expected the end
                return not_greedy and unpack(not_greedy)
            end
            next_matcher = true

        elseif mod == '^' then
            if token_index ~= 1 then
                -- expected the start
                return not_greedy and unpack(not_greedy)
            end
            next_matcher = true

        elseif not token then
            -- ran out of tokens before the end of the seq
            return unpack(non_greedy)
        else

            hl = apply_highlight_matcher(matcher, token, str)
            if mod == '*' or mod == '*?' then
                next_matcher = not hl
            elseif mod == '+' or mod == '+?' then
                mod = '*' .. string.sub(mod, 2)
                next_matcher = not hl
            elseif hl then
                next_matcher = true
            else
                -- no match
                return non_greedy and unpack(non_greedy)
            end
        end

        if hl then
            append_highlights(highlights, hl)
            token_index = token_index + 1
        end

        if next_matcher then
            seq_index = seq_index + 1
            matcher = seq[seq_index]
            mod = matcher and matcher.mod
        end
    end
    return highlights, token_index
end

function apply_highlight_seq(seq, tokens, str, do_nested)
    local highlights = nil
    local token_index = 1
    for i = 1, #tokens do
        if do_nested and tokens[i].nested then
            local hl = apply_highlight_seq(seq, tokens[i].nested, str, do_nested)
            if hl then
                highlights = highlights or {}
                append_highlights(highlights, hl)
            end
        end

        if i == token_index then
            local hl, finish = apply_highlight_seq_at(seq, 1, tokens, str, token_index)
            if hl then
                highlights = highlights or {}
                append_highlights(highlights, hl)
                token_index = finish
            else
                token_index = token_index + 1
            end
        end
    end
    return highlights
end

local function apply_highlight_rules(rules, tokens, str)
    for i = 1, #rules do
        local hl = apply_highlight_seq(rules[i], tokens, str, true)
        if hl then
            for _, hl in ipairs(hl) do
                wish.add_buf_highlight(hl)
            end
        end
    end
end

wish.add_event_callback('buffer_change', function()
    local buffer = wish.get_buffer()
    -- rehighlight if last buffer was not a valid zsh command
    -- or the new buffer has changed (excepting ending whitespace changes)
    if not prev_complete or string.sub(buffer, 1, #prev_buffer) ~= prev_buffer or string.find(buffer, '%S', #prev_buffer+1) then
        -- is this going to be slow? do we need a debounce or something?
        local complete, tokens = wish.parse(buffer)
        -- wish.pprint(tokens)
        wish.log.debug(wish.repr(debug_tokens(tokens, buffer), true))
        prev_buffer = buffer
        prev_complete = complete

        wish.clear_buf_highlights(NAMESPACE)
        apply_highlight_rules(RULES, tokens, buffer)
        wish.redraw{buffer = true}
    end
end)
