local prev_buffer = nil

local PUNCTUATION = {fg = 'grey', bold = true}
local NEW_COMMAND = {fg = 'yellow'}
local STRING = nil
local KEYWORD = {fg = 'green', bold = true}

local highlights = {
    SEPER = NEW_COMMAND,
    DBAR = NEW_COMMAND,
    DAMPER = NEW_COMMAND,
    BAR = NEW_COMMAND,
    BARAMP = NEW_COMMAND,
    STRING = STRING,
    ENVSTRING = STRING,
    ENVARRAY = STRING,

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
    TYPESET = KEYWORD
}

wish.add_event_callback('buffer_change', function()
    local buffer = wish.get_buffer()
    if buffer ~= prev_buffer then
        -- rehighlight
        -- is this going to be slow? do we need a debounce or something?
        local complete, starts, ends, kinds = wish.parse(buffer)

        wish.clear_buf_highlights()
        for i = 1, #kinds do
            local hl = highlights[kinds[i]]
            if not hl and kinds[i] ~= 'STRING' then
                hl = PUNCTUATION
            end

            if hl then
                local arg = {}
                for k, v in pairs(hl) do
                    arg[k] = v
                end
                arg.start = starts[i]
                arg['end'] = ends[i]
                wish.add_buf_highlight(arg)
            end
        end
        wish.redraw{buffer = true}

        prev_buffer = buffer
    end
end)
