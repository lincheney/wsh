local prev_buffer = nil

local PUNCTUATION = {fg = 'grey', bold = true}
local NEW_COMMAND = {fg = 'yellow'}
local STRING = {}
local KEYWORD = {fg = 'green', bold = true}
local COMMENT = {fg = 'grey'}
local NAMESPACE = wish.add_buf_highlight_namespace()

local highlights = {
    SEPER = NEW_COMMAND,
    DBAR = NEW_COMMAND,
    DAMPER = NEW_COMMAND,
    BAR = NEW_COMMAND,
    BARAMP = NEW_COMMAND,
    STRING = STRING,
    ENVSTRING = STRING,
    ENVARRAY = STRING,
    LEXERR = STRING,

    CASE = KEYWORD,
    COPROC = KEYWORD,
    DOLOOP = KEYWORD,
    DONE = KEYWORD,
    ELIF = KEYWORD,
    ELSE = KEYWORD,
    ZEND = KEYWORD,
    ESAC = KEYWORD,
    FI = KEYWORD,
    FOR = KEYWORD,
    FOREACH = KEYWORD,
    FUNC = KEYWORD,
    IF = KEYWORD,
    NOCORRECT = KEYWORD,
    REPEAT = KEYWORD,
    SELECT = KEYWORD,
    THEN = KEYWORD,
    TIME = KEYWORD,
    UNTIL = KEYWORD,
    WHILE = KEYWORD,
    TYPESET = KEYWORD,

    comment = COMMENT,
}

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
    flag = {fg = 'green'},
    string = {fg = '#ff0000'},
}

local RULES = {
    {{hl=HL.flag, kind='STRING', pat='^%-'}},
    {{hl=HL.string, kind='STRING', pat='^".*"$'}},
    {{kind='substitution', contains={
        'start',
        {hl='green', pat='%$'},
        {hl='green', pat='('},
        '.*',
        {hl='green', pat=')'},
        'end',
    } }},
}

local apply_highlight_seq

local function apply_highlight_matcher(matcher, token, str)
    if matcher.kind and matcher.kind ~= token.kind then
        return
    end
    if matcher.pat and not string.find(string.sub(str, token.start+1, token.finish), matcher.pat) then
        return
    end

    local highlights = {}
    if matcher.hl then
        local hl = wish.iter.copy(matcher.hl)
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
        local hl = apply_highlight_seq(matcher.contains, token.nested, str)
        if not hl then
            -- nested rules don't match
            return
        end
        for i = 1, #hl do
            table.insert(highlights, hl[i])
        end
    end

    return highlights
end

local function apply_highlight_seq_at(seq, seq_index, tokens, str, token_index)
    -- try to apply the seq[seq_index:] at tokens[token_index:] and return the end index
    local highlights = {}
    local matcher = seq[seq_index]
    while seq_index <= #seq do
        if token_index > #tokens then
            -- ran out of tokens before the end of the seq
            return
        end

        local token = tokens[token_index]
        if matcher == '*' then
            -- branch
            -- non greedy?
            local hl, index = apply_highlight_seq_at(seq, seq_index+1, tokens, str, token_index+1)
            if hl then
                for i = 1, #hl do
                    table.insert(highlights, hl[i])
                end
                return highlights, index
            end

        else
            local hl = apply_highlight_matcher(matcher, token, str)
            if hl then
                for i = 1, #hl do
                    table.insert(highlights, hl[i])
                end
                seq_index = seq_index + 1
                matcher = seq[seq_index]
            else
                return
            end
        end

        token_index = token_index + 1
    end
    return highlights, token_index
end

function apply_highlight_seq(seq, tokens, str)
    local highlights = nil
    local token_index = 1
    for i = 1, #tokens do
        if tokens[i].nested then
            local hl = apply_highlight_seq(seq, tokens[i].nested, str)
            if hl then
                highlights = highlights or {}
                for i = 1, #hl do
                    table.insert(highlights, hl[i])
                end
            end
        end

        if i == token_index then
            local hl, finish = apply_highlight_seq_at(seq, 1, tokens, str, token_index)
            if hl then
                highlights = highlights or {}
                token_index = finish
                for i = 1, #hl do
                    table.insert(highlights, hl[i])
                end
            else
                token_index = token_index + 1
            end
        end
    end
    return highlights
end

local function apply_highlight_rules(rules, tokens, str)
    for i = 1, #rules do
        local hl = apply_highlight_seq(rules[i], tokens, str)
        if hl then
            for _, hl in ipairs(hl) do
                wish.add_buf_highlight(hl)
            end
        end
    end
end

wish.add_event_callback('buffer_change', function()
    local buffer = wish.get_buffer()
    if buffer ~= prev_buffer then
        -- rehighlight
        -- is this going to be slow? do we need a debounce or something?
        local complete, tokens = wish.parse(buffer, true)
        -- wish.pprint(tokens)
        wish.log.debug(wish.repr(debug_tokens(tokens, buffer), true))

        wish.clear_buf_highlights(NAMESPACE)
        apply_highlight_rules(RULES, tokens, buffer)
        -- for i = 1, #tokens do
            -- local hl = highlights[tokens[i].kind]
            -- if not hl and tokens[i].kind ~= 'STRING' and not buffer:sub(tokens[i].start+1, tokens[i].finish):find('%w') then
                -- hl = PUNCTUATION
            -- end
--
            -- if hl and next(hl) then
                -- local arg = {}
                -- for k, v in pairs(hl) do
                    -- arg[k] = v
                -- end
                -- arg.start = tokens[i].start
                -- arg.finish = tokens[i].finish
                -- arg.namespace = NAMESPACE
                -- wish.add_buf_highlight(arg)
            -- end
        -- end
        wish.redraw{buffer = true}

        prev_buffer = buffer
    end
end)
