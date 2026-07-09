require('wish.api')
wish.plugin = require('wish.plugin')
wish.iter = require('wish.iter')
wish.table = require('wish.table')
wish.style = require('wish.style')
wish.utf8 = require('wish.utf8')
require('wish.keybind')
require('wish.syntax-highlight').enable()
require('wish.paste').enable()
require('wish.completion').enable{
    keybinds = {
        ['<tab>'] = 'start',
    }
}

local config_home = os.getenv('XDG_CONFIG_HOME')
if not config_home then
    local home = os.getenv('HOME')
    config_home = home and home .. '/.config/wish/'
end
if config_home then
    package.path = config_home .. '?.lua;' .. package.path
    local file = io.open(config_home .. 'init.lua', "r")
    if file then
        file:close()
        dofile(config_home .. 'init.lua')
    end
end

-- require('wish.extras.mouse-demo').enable()

return wish
