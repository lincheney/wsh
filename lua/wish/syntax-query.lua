local M = {}

function M.debug_tokens(tokens, buffer)
    local x = {}
    for i = 1, #tokens do
        x[i] = {
            buffer:sub(tokens[i].start, tokens[i].finish),
            tokens[i].kind,
            tokens[i].nested and M.debug_tokens(tokens[i].nested, buffer)
        }
    end
    return x
end

local function apply_matcher(matcher, token, str)

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
        tokstr = tokstr or string.sub(str, token.start, token.finish)
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

    local values = {{matcher, token}}
    if matcher.contains then
        local matched = M.apply_seq(matcher.contains, token.nested, str, function(matches)
            wish.table.extend(values, matches)
        end)
        if not matched then
            return
        end
    end

    return values
end

local function apply_seq_at(seq, seq_index, tokens, str, token_index)
    -- try to apply the seq[seq_index:] at tokens[token_index:] and return the end index
    local values = {}
    local non_greedy = {}
    local matcher = seq[seq_index]
    local mod = matcher and matcher.mod

    while seq_index <= #seq do
        if mod == '*' or mod == '*?' or mod == '?' or mod == '??' then
            -- try the next matcher, non greedy
            local index, non_greedy_values = apply_seq_at(seq, seq_index+1, tokens, str, token_index)

            if index then
                if mod == '*?' or mod == '??' then
                    -- we wanted non greedy, so return it now
                    return index, non_greedy_values
                end

                non_greedy_values = wish.iter(values):chain(non_greedy_values):collect()
                -- non greedy match when we wanted greedy, save for later in case the greedy match doesn't work
                non_greedy = {index, non_greedy_values}
            end
        end

        local token = tokens[token_index]
        local next_matcher = false
        if mod == '$' then
            if token then
                -- expected the end
                return unpack(non_greedy)
            end
            next_matcher = true

        elseif mod == '^' then
            if token_index ~= 1 then
                -- expected the start
                return unpack(non_greedy)
            end
            next_matcher = true

        elseif not token then
            -- ran out of tokens before the end of the seq
            return unpack(non_greedy)
        else

            local matches = apply_matcher(matcher, token, str)
            if matches then
                wish.table.extend(values, matches)
                token_index = token_index + 1
            end

            if mod == '*' or mod == '*?' then
                next_matcher = not matches
            elseif mod == '+' or mod == '+?' then
                mod = '*' .. string.sub(mod, 2)
                next_matcher = not matches
            elseif matches then
                next_matcher = true
            else
                -- no match
                return unpack(non_greedy)
            end
        end

        if next_matcher then
            seq_index = seq_index + 1
            matcher = seq[seq_index]
            mod = matcher and matcher.mod
        end
    end
    return token_index, values
end

function M.apply_seq(seq, tokens, str, callback)
    local matched = false
    local token_index = 1
    while token_index <= #tokens do
        local finish, values = apply_seq_at(seq, 1, tokens, str, token_index)
        if finish then
            matched = true
            token_index = finish
            callback(values)
        else
            token_index = token_index + 1
        end
    end
    return matched
end

function M.apply_rules(rules, tokens, str, callback)
    for i = 1, #rules do
        M.apply_seq(rules[i], tokens, str, callback)
    end
    for i = 1, #tokens do
        if tokens[i].nested then
            M.apply_rules(rules, tokens[i].nested, str, callback)
        end
    end
end

local CALLBACKS = {}

function M.add_buffer_callback(func)
    table.insert(CALLBACKS, func)
end

local prev_buffer = nil
local prev_complete = false
local prev_tokens = nil

local function parse_buffer()
    local buffer = wish.get_buffer()
    -- rehighlight if last buffer was not a valid zsh command
    -- or the new buffer has changed (excepting ending whitespace changes)
    if not prev_complete or string.sub(buffer, 1, #prev_buffer) ~= prev_buffer or string.find(buffer, '%S', #prev_buffer+1) then
        -- is this going to be slow? do we need a debounce or something?
        prev_complete, prev_tokens = wish.parse(buffer)
        wish.log.debug(wish.repr(M.debug_tokens(prev_tokens, buffer), true))
        prev_buffer = buffer
        return true
    end
end

function M.parse_buffer()
    parse_buffer()
    return prev_tokens, prev_buffer
end

wish.add_event_callback('buffer_change', function()
    -- don't bother if no-one cares about the syntax tree
    if #CALLBACKS == 0 or not parse_buffer() then
        return
    end

    local shift = 0
    for i = 1, #CALLBACKS do
        if CALLBACKS[i](prev_tokens, prev_buffer) then
            -- remove this callback
            shift = shift + 1
            CALLBACKS[i] = nil
        elseif shift > 0 then
            CALLBACKS[i - shift] = CALLBACKS[i]
            CALLBACKS[i] = nil
        end
    end
end)

return M
