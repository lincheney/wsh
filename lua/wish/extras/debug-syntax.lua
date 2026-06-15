return wish.plugin(function(wish, opts, plugin)

    local msg = wish.set_message{persist = true, border = {fg = 'grey'}}

    local QUERY = require('wish.syntax-query')
    QUERY.add_buffer_callback(function(tokens, str)
        if not plugin.is_enabled() then
            return true
        end
        local output = QUERY.debug_tokens(tokens, str), true
        wish.set_message{id=msg, text=output}
    end)
end)
