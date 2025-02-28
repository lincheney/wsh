require('wish/api')
require('wish/keybind')

function wish.async.spawn(...)
    local proc, stdin, stdout, stderr = wish.async.__spawn(...)
    return {
        stdin = stdin,
        stdout = stdout,
        stderr = stderr,
        id = function(self) return proc:id() end,
        wait = function(self) return proc:wait() end,
    }
end

return wish
