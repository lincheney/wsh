return wish.plugin(function(wish)
    wish.pprint('calc')

    local CUSTOM_COMMAND = require('wish.custom_command')

    local NAMESPACE = wish.add_buf_highlight_namespace()

    local active = false
    local function clear()
        if active then
            wish.clear_buf_highlights(NAMESPACE)
            active = false
        end
    end

    local function make_script(expr)
        return '() { set -o localoptions -o extendedglob -o forcefloat; local __expr='..wish.shell_quote(expr)..'; echo "$(( $__expr ))"; }'
    end

    local function do_maths(expr)
        local proc = wish.cmd{
            args = make_script(expr),
            stdin = 'null',
            stdout = 'piped',
            stderr = 'piped',
        }
        local code = proc:wait()
        local stdout = proc.stdout:read_all():gsub('\n$', '')
        local stderr = proc.stderr:read_all():gsub('\n$', '')
        return code > 0 and stderr or stdout
    end

    wish.add_event_callback('buffer_change', function()
        clear()
        local expr = CUSTOM_COMMAND.extract(wish.get_buffer(), '//')
        if expr then
            local value = do_maths(expr)
            local args = wish.table.merge({}, wish.style.number, {
                start = math.pow(2, 32) - 1,
                finish = math.pow(2, 32) - 1,
                dim = true,
                virtual_text = ' = ' .. value,
                namespace = NAMESPACE,
            })
            wish.add_buf_highlight(args)
            active = true
        end
    end)

    wish.add_event_callback('accept_line', function()
        clear()
    end)

    CUSTOM_COMMAND.register(wish, {
        keyword = '//',
        callback = function(command)
            wish.cmd(make_script(command)):wait()
        end,
    })

end)
