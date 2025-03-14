local prev_buffer = nil

wish.add_event_callback('buffer_change', function()
    local buffer = wish.get_buffer()
    if buffer ~= prev_buffer then
        -- rehighlight
        local complete, starts, ends, kinds = wish.parse(buffer)

        wish.clear_buf_highlights()
        -- wish.pprint(kinds)
        for i = 1, #kinds do
            if kinds[i] ~= 'LEXERR' and kinds[i]:lower() ~= 'string' then
                wish.add_buf_highlight{
                    start = starts[i],
                    ['end'] = ends[i],
                    fg = 'yellow',
                    bold = true,
                }
            end
        end
        wish.redraw{buffer = true}

        prev_buffer = buffer
    end
end)
