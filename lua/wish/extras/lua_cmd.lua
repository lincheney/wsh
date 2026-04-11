return wish.plugin(function(wish)

    require('wish.custom_command').register(wish, {
        keyword = '//lua',
        callback = function(command)
            wish.cmd('wsh lua ' .. wish.shell_quote(command))
        end,
    })

end)
