return wish.plugin(function(wish)

    require('wish.custom_command').register(wish, {
        keyword = '//py',
        callback = function(command)
            wish.cmd('python3 -c ' .. wish.shell_quote(command))
        end,
    })

end)
