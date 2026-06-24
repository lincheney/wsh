return wish.plugin(function(wish)

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
        return '() { set -o localoptions -o extendedglob -o forcefloat; local __expr='..wish.shell_quote(expr)..'; printf -v __value "$(( $__expr ))"; }'
    end

    local function do_maths(expr)
        return wish.in_param_scope(function()
            local code = wish.silent_cmd(make_script(expr))
            local value = wish.get_var('__value')
            return code > 0 and 'failed' or value
        end)
    end

    wish.add_event_callback('buffer_change', function()
        clear()
        local expr = CUSTOM_COMMAND.extract(wish.get_buffer(), '//')
        if expr then
            local value = do_maths(expr)
            local args = wish.table.merge({}, wish.style.number, {
                start = wish.MAXNUM,
                finish = wish.MAXNUM,
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
            wish.print(do_maths(command))
        end,
    })

end)
