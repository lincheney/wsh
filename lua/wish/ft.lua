local M = {}
local NAMESPACE = wish.add_buf_highlight_namespace()
local CHARS = 'fjdkslarueiwoqpvn'

local key_event_id = nil
local keymap_layer = nil

function M.deactivate()
    if keymap_layer then
        wish.del_keymap_layer(keymap_layer)
        keymap_layer = nil
    end
    if key_event_id then
        wish.remove_event_callback(key_event_id)
        key_event_id = nil
    end
    wish.clear_buf_highlights(NAMESPACE)
    wish.redraw()
end

function M.activate()
    local positions = nil
    key_event_id = wish.add_event_callback('key', function(key, data)
        wish.try(function()

            -- we have highlighted keys and waiting for one of them
            if positions then
                if positions[data] then
                    wish.set_cursor(positions[data])
                    -- deactivate later
                    wish.schedule(M.deactivate)
                else
                    M.deactivate()
                end
                return
            end

            -- i pressed something else
            if not data:find('^[%w%s%p]$') then
                M.deactivate()
                return
            end

            local smartcase = data:find('^[a-z]$') and '['..data..data:upper()..']'
            local buffer = wish.get_buffer()
            local cursor = wish.get_cursor()
            cursor = wish.str.to_byte_pos(buffer, cursor) or #buffer - 1
            local matches = {}
            local start = 1
            local s, e
            while start <= #buffer do
                if smartcase then
                    s, e = buffer:find(smartcase, start)
                else
                    s, e = buffer:find(data, start, true)
                end
                if not s then
                    break
                end
                start = e + 1
                if s ~= cursor + 1 then
                    s = wish.str.from_byte_pos(buffer, s - 1)
                    e = wish.str.from_byte_pos(buffer, e) or #buffer
                    table.insert(matches, {s, e})
                end
            end

            -- nothing matched
            if #matches == 0 then
                wish.schedule(M.deactivate)
                return
            end

            -- jump directly to the only match
            if #matches == 1 then
                wish.set_cursor(matches[1][1])
                -- deactivate later
                wish.schedule(M.deactivate)
                return
            end

            -- sort by distance from cursor
            wish.table.sort_by(matches, function(m) return math.abs(m[1] - cursor) end)

            -- highlight the keys
            positions = {}
            for i, m in ipairs(matches) do
                local c = CHARS:sub(i, i)
                wish.add_buf_highlight{
                    start = m[1],
                    finish = m[2],
                    namespace = NAMESPACE,
                    fg = 'cyan',
                    underline = true,
                    bold = true,
                    virtual_text = c,
                    conceal = true,
                    no_blend = true,
                }
                positions[c] = m[1]
            end
            wish.redraw()

        end, function(e)
                -- deactivate on any error so we don't get stuck
                M.deactivate()
                error(e)
            end)
    end)
    keymap_layer = wish.add_keymap_layer(true)
    -- dim everything
    wish.add_buf_highlight{
        start = 0,
        finish = math.pow(2, 32) - 1,
        dim = true,
        namespace = NAMESPACE,
        no_blend = true,
    }
    wish.redraw()
end

return M
