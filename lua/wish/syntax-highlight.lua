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

wish.add_event_callback('buffer_change', function()
    local buffer = wish.get_buffer()
    if buffer ~= prev_buffer then
        -- rehighlight
        -- is this going to be slow? do we need a debounce or something?
        local complete, tokens = wish.parse(buffer, true)
        if true then return end
        wish.pprint(tokens)
        local x = {}
        for i = 1, #tokens do
            x[i] = buffer:sub(tokens[i].start+1, tokens[i].finish)
        end
        wish.pprint(x)

        wish.clear_buf_highlights(NAMESPACE)
        for i = 1, #tokens do
            local hl = highlights[tokens[i]]
            if not hl and tokens[i].kind ~= 'STRING' and not buffer:sub(tokens[i].start+1, tokens[i].finish):find('%w') then
                hl = PUNCTUATION
            end

            if hl and next(hl) then
                local arg = {}
                for k, v in pairs(hl) do
                    arg[k] = v
                end
                arg.start = tokens[i].start
                arg.finish = tokens[i].finish
                arg.namespace = NAMESPACE
                wish.add_buf_highlight(arg)
            end
        end
        wish.redraw{buffer = true}

        prev_buffer = buffer
    end
end)
