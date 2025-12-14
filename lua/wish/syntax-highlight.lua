local QUERY = require('wish/syntax-query')
local NAMESPACE = wish.add_buf_highlight_namespace()

local RULES = {
    { { hl='command', kind='command' } },
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
    {{ kind='STRING|command', contains={
        {hl='escape', kind='Bnull'},
        {hl='escape', kind='', regex='^[^ ]', hlregex='^[^ ]'},
    } }},
    {{ kind='STRING|command', contains={
        {hl='escape_space', kind='Bnull'},
        {hl='escape_space', kind='', regex='^ ', hlregex='^ '},
    } }},
    {{ kind='STRING|command', contains={
        {kind='String'},
        {kind='Snull'},
        {hl='escape', not_kind='Snull', hlregex=[=[\\x[0-9a-fA-F]{0,2}|\\u\d{0,4}|\\.]=], mod='*'},
        {kind='Snull', mod='?'},
    } }},
    {{ kind='STRING|command', contains={
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
    { {kind='STRING|command', contains={
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
    -- but reset highlights on these
    { {kind='redirect', contains={ {hl='normal', kind='STRING'} }} },
    -- function
    { {kind='function', contains={ {hl='func', kind='FUNC'}, {hl='func', kind='STRING', mod='?'} }} },
    { {kind='function', contains={ {mod='^'}, {hl='func', kind='STRING'} }} },
    -- keywords
    { {hl='keyword', kind='CASE|COPROC|DOLOOP|DONE|ELIF|ELSE|ZEND|ESAC|FI|FOR|FOREACH|FUNC|IF|NOCORRECT|REPEAT|SELECT|THEN|TIME|UNTIL|WHILE|TYPESET'} },
    -- unmatched brackets
    -- { { hl='error', regex='^\\($' }, { not_regex='\\)', mod='*' }, { mod='$' } },
    -- { { hl='error', regex='^\\{$' }, { not_regex='\\}', mod='*' }, { mod='$' } },
}

local function apply_highlight_matcher(matcher, token, str, highlights, priority)
    if not matcher.hl then
        return
    end

    if matcher.hlregex then
        if type(matcher.hlregex) == 'string' then
            matcher.hlregex = wish.regex(matcher.hlregex)
        end

        local tokstr = string.sub(str, token.start+1, token.finish)
        local captures = matcher.hlregex:captures_all(tokstr)
        for _, capture in ipairs(captures) do
            local index = capture[2] or capture[1]
            local hl = wish.table.copy(wish.style[matcher.hl])
            hl.start = token.start + index[1] - 1
            hl.finish = token.start + index[2]
            hl.namespace = NAMESPACE
            hl.priority = priority
            table.insert(highlights, hl)
        end

    else
        local hl = wish.table.copy(wish.style[matcher.hl])
        hl.start = token.start
        hl.finish = token.finish
        hl.namespace = NAMESPACE
        hl.priority = priority
        table.insert(highlights, hl)
    end
end

local function apply_highlight_rules(rules, tokens, str, highlights, priority)
    priority = priority or 0
    highlights = highlights or {}
    local rule_priority
    local function callback(matches)
        for i = 1, #matches do
            apply_highlight_matcher(matches[i][1], matches[i][2], str, highlights, rule_priority)
        end
    end

    for i = 1, #rules do
        rule_priority = (priority + (rules[i].priority or 0)) * #rules * 2 + i
        QUERY.apply_seq(rules[i], tokens, str, callback)
    end
    for i = 1, #tokens do
        if tokens[i].nested then
            apply_highlight_rules(rules, tokens[i].nested, str, highlights, priority+1)
        end
    end
    return highlights
end

QUERY.add_buffer_callback(function(tokens, str)
    wish.clear_buf_highlights(NAMESPACE)

    local highlights = apply_highlight_rules(RULES, tokens, str)
    for i = 1, #highlights do
        highlights[i].priority = highlights[i].priority + i / #highlights / 2
    end
    wish.table.sort_by(highlights, 'priority')
    wish.log.debug(wish.repr(highlights, true))
    for i = 1, #highlights do
        wish.add_buf_highlight(highlights[i])
    end

    wish.redraw{buffer = true}
end)
