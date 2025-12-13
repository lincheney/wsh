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

local RULES = {
    -- comments
    { {hl='comment', kind='comment'} },
    -- punctuation
    { {hl='symbol', regex='^\\W+$'}, priority=-1000 },
    -- strings
    {
        {hl='string', kind='Dnull|Snull'},
        {hl='string', not_kind='Dnull|Snull', mod='*'},
        {hl='string', kind='Dnull|Snull', mod='?'},
        priority=-1,
    },
    -- heredocs
    { {hl='string', kind='heredoc_body'} },
    { {hl='heredoc_tag', kind='heredoc_open_tag|heredoc_close_tag'} },
    -- escapes
    {{ kind='STRING', contains={
        {hl='escape', kind='Bnull'},
        {hl='escape', kind='', regex='^[^ ]', hlregex='^[^ ]'},
    } }},
    {{ kind='STRING', contains={
        {hl='escape_space', kind='Bnull'},
        {hl='escape_space', kind='', regex='^ ', hlregex='^ '},
    } }},
    {{ kind='STRING', contains={
        {kind='String'},
        {kind='Snull'},
        {hl='escape', not_kind='Snull', hlregex=[=[\\x[0-9a-fA-F]{0,2}|\\u\d{0,4}|\\.]=], mod='*'},
        {kind='Snull', mod='?'},
    } }},
    {{ kind='STRING', contains={
        {kind='Dnull'},
        {hl='escape', not_kind='Dnull', hlregex='\\\\.', mod='*'},
        {kind='Dnull', mod='?'},
    } }},
    {{ kind='heredoc_body', contains={{contains={
        { hl='escape', kind='Bnull'},
        { hl='escape', regex='^[\\\\$]', hlregex='^[\\\\$]'},
    } }} }},
    -- env vars
    { {hl='env_var_key', kind='ENVSTRING', hlregex='^[^=]+'} },
    { {hl='env_var_value', kind='ENVSTRING', hlregex='=(.*)$'} },
    -- reset highlight on substitutions in strings
    { {kind='STRING', contains={
        {hl='normal', kind='substitution', contains={{hl='symbol', regex='^\\W+$'}} },
    } } },
    { {hl='flag', kind='STRING', regex='^-', hlregex='^-[^=]*'} },
    { {hl='flag_value', kind='STRING', regex='^-.*=', hlregex='^-[^=]*=(.+)'} },
    { {kind='heredoc_body', contains={ {contains={ {hl='normal', kind='substitution'} } } } } },
    -- variables
    {
        {hl='variable', kind='Qstring|String'},
        {hl='variable', kind='|String|Quest'},
    },
    {
        {hl='variable', kind='Qstring|String'},
        {hl='variable', kind='Inbrace'},
        {hl='variable', mod='*?'},
        {hl='variable', kind='Outbrace'},
    },
    -- this will match the first string then consume the rest
    {
        {hl='command', kind='STRING'},
        {not_kind='SEPER|BAR|DBAR|AMPER|DAMPER|BARAMP|AMPERBANG|SEMIAMP|SEMIBAR', mod='*'},
    },
    -- but reset highlights on these
    { {kind='redirect', contains={ {hl='normal', kind='STRING'} }} },
    -- function
    { {kind='function', contains={ {hl='func', kind='FUNC'}, {hl='func', kind='STRING', mod='?'} }} },
    { {kind='function', contains={ {mod='^'}, {hl='func', kind='STRING'} }} },
    -- keywords
    { {hl='keyword', kind='CASE|COPROC|DOLOOP|DONE|ELIF|ELSE|ZEND|ESAC|FI|FOR|FOREACH|FUNC|IF|NOCORRECT|REPEAT|SELECT|THEN|TIME|UNTIL|WHILE|TYPESET'} },
    -- unmatched brackets
    { { hl='error', regex='^\\($' }, { not_regex='\\)', mod='*' }, { mod='$' } },
    { { hl='error', regex='^\\{$' }, { not_regex='\\}', mod='*' }, { mod='$' } },
}

local apply_highlight_seq

local function apply_highlight_matcher(matcher, token, str)

    if matcher.contains and not token.nested then
        -- matcher asserts nested tokens but there aren't any
        return
    end

    local tokstr = nil
    local kind = token.kind or ''
    if matcher.kind then
        if type(matcher.kind) == 'string' then
            matcher.kind = wish.regex(matcher.kind)
        end
        if not matcher.kind:is_full_match(kind) then
            return
        end
    end
    if matcher.not_kind then
        if type(matcher.not_kind) == 'string' then
            matcher.not_kind = wish.regex(matcher.not_kind)
        end
        if matcher.not_kind:is_full_match(kind) then
            return
        end
    end

    if matcher.regex or matcher.not_regex then
        tokstr = tokstr or string.sub(str, token.start+1, token.finish)
        if type(matcher.regex) == 'string' then
            matcher.regex = wish.regex(matcher.regex)
        end
        if type(matcher.not_regex) == 'string' then
            matcher.not_regex = wish.regex(matcher.not_regex)
        end
        if matcher.regex and not matcher.regex:is_match(tokstr) then
            return
        end
        if matcher.not_regex and matcher.not_regex:is_match(tokstr) then
            return
        end
    end

    local highlights = {}
    if matcher.hl then

        if matcher.hlregex then
            if type(matcher.hlregex) == 'string' then
                matcher.hlregex = wish.regex(matcher.hlregex)
            end

            tokstr = tokstr or string.sub(str, token.start+1, token.finish)
            local captures = matcher.hlregex:captures_all(tokstr)
            for _, capture in ipairs(captures) do
                local index = capture[2] or capture[1]
                local hl = wish.table.copy(wish.style[matcher.hl])
                hl.start = token.start + index[1] - 1
                hl.finish = token.start + index[2]
                hl.namespace = NAMESPACE
                table.insert(highlights, hl)
            end

        else
            local hl = wish.table.copy(wish.style[matcher.hl])
            hl.start = token.start
            hl.finish = token.finish
            hl.namespace = NAMESPACE
            table.insert(highlights, hl)
        end
    end

    if matcher.contains then
        local hl = apply_highlight_seq(matcher.contains, token.nested, str)
        if not hl then
            -- nested rules don't match
            return
        end
        wish.table.extend(highlights, hl)
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
                    wish.table.extend(highlights, hl)
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
                return unpack(not_greedy)
            end
            next_matcher = true

        elseif mod == '^' then
            if token_index ~= 1 then
                -- expected the start
                return unpack(not_greedy)
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
            wish.table.extend(highlights, hl)
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

function apply_highlight_seq(seq, tokens, str)
    local highlights = nil
    local token_index = 1
    for i = 1, #tokens do
        if i == token_index then
            local hl, finish = apply_highlight_seq_at(seq, 1, tokens, str, token_index)
            if hl and #hl > 0 then
                highlights = wish.table.extend(highlights or {}, hl)
                token_index = finish
            else
                token_index = token_index + 1
            end
        end
    end
    return highlights
end

local function apply_highlight_rules(rules, tokens, str, highlights, priority)
    for i = 1, #rules do
        local hl = apply_highlight_seq(rules[i], tokens, str)
        if hl then
            local p = priority + (rules[i].priority or 0) + i / #rules / 2
            for j = 1, #hl do
                hl[j].priority = p
            end
            wish.table.extend(highlights, hl)
        end
    end

    for i = 1, #tokens do
        if tokens[i].nested then
            apply_highlight_rules(rules, tokens[i].nested, str, highlights, priority+1)
        end
    end

    return highlights
end

wish.add_event_callback('buffer_change', function()
    local buffer = wish.get_buffer()
    -- rehighlight if last buffer was not a valid zsh command
    -- or the new buffer has changed (excepting ending whitespace changes)
    if not prev_complete or string.sub(buffer, 1, #prev_buffer) ~= prev_buffer or string.find(buffer, '%S', #prev_buffer+1) then
        -- is this going to be slow? do we need a debounce or something?
        local complete, tokens = wish.parse(buffer)
        wish.log.debug(wish.repr(debug_tokens(tokens, buffer), true))
        prev_buffer = buffer
        prev_complete = complete

        wish.clear_buf_highlights(NAMESPACE)

        local highlights = apply_highlight_rules(RULES, tokens, buffer, {}, 0)
        wish.table.sort_by(highlights, 'priority')
        for i = 1, #highlights do
            wish.add_buf_highlight(highlights[i])
        end

        wish.redraw{buffer = true}
    end
end)
