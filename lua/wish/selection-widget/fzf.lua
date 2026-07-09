local M = {}

function M.new()
    return wish.plugin(function(wish, opts, plugin)

        local function get_proc()
            if not plugin.proc then
                plugin.cursor = wish.get_cursor()
                -- go to last line
                wish.set_cursor(wish.str.len(wish.get_buffer()) + 1)
                wish.redraw{now=true}

                plugin.proc = wish.async.spawn{
                    args = {
                        'fzf',
                        '--read0',
                        '--ansi',
                        '--exit-0',
                        '--height=40%',
                        '--reverse',
                        '--with-nth=2..',
                    },
                    foreground = true,
                    stdin = 'piped',
                    stdout = 'piped',
                }
            end
            return plugin.proc
        end

        local function finish()
            local result = nil
            if plugin.proc then
                plugin.proc.stdin:close()
                -- and wait for the proc to finish
                local code = plugin.proc:wait()
                if code == 0 then
                    result = tonumber(plugin.proc.stdout:read_to_end():match('^(%d+)\t'))
                end
                plugin.proc = nil

                -- go back up
                io.stdout:write('\x1b[A')
                io.stdout:flush()

                wish.set_cursor(plugin.cursor)
            end

            if plugin.on_accept then
                local on_accept = plugin.on_accept
                plugin.on_accept = nil
                on_accept(result)
            end
        end

        function plugin.start(source, on_accept)
            plugin.count = 0
            plugin.on_accept = on_accept

            if type(source) == 'table' then
                plugin.add_lines(source)
            elseif type(source) == 'function' then
                wish.schedule(function()
                    for lines in source do
                        if not plugin.inner.is_enabled() then
                            break
                        end
                        plugin.add_lines(lines)
                    end
                end)
            elseif source then
                error('expected source to be array of lines or function, got: ' .. type(source))
            end
        end

        function plugin.stop()
            if plugin.proc then
                plugin.proc.term()
            end
            finish()
        end

        function plugin.add_lines(lines)
            wish.try{
                try = function()
                    if lines and #lines > 0 then
                        local str = {}
                        for i = 1, #lines do
                            local sgr = wish.style_to_sgr(lines[i])
                            table.insert(str, string.format('%i\t%s%s\x1b[0m\0', plugin.count + i, sgr or '', lines[i].text))
                        end
                        plugin.count = plugin.count + #lines
                        get_proc().stdin:write(table.concat(str, ''))
                    end
                end,
                finally = function(err)
                    finish()
                end
            }
        end

    end)
end

return M
