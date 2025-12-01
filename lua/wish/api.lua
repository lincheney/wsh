function wish.repr(val)
    if type(val) == 'table' then
        local text = {}
        for k, v in ipairs(val) do
            table.insert(text, wish.repr(v))
        end
        for k, v in pairs(val) do
            if type(k) == 'string' and not k:find('%W') then
                table.insert(text, k .. ' = ' .. wish.repr(v))
            elseif type(k) ~= 'number' or k > #val then
                table.insert(text, '['..wish.repr(k)..'] = ' .. wish.repr(v))
            end
        end
        return '{' .. table.concat(text, ', ') .. '}'
    elseif type(val) == 'string' then
        local val = string.format('%q', val):gsub('\\\n', '\\n')
        return val
    else
        return tostring(val)
    end
end

function wish.pprint(val)
    wish.log.debug(wish.repr(val))
end

function wish.async.spawn(...)
    local proc, stdin, stdout, stderr = wish.__spawn(...)
    return {
        stdin = stdin,
        stdout = stdout,
        stderr = stderr,
        pid = function() return proc:pid() end,
        is_finished = function() return proc:is_finished() end,
        wait = function() return proc:wait() end,
        kill = function() return proc:kill() end,
        term = function() return proc:kill('SIGTERM') end,
    }
end

function wish.cmd(...)
    local proc, stdin, stdout, stderr = wish.__shell_run(...)
    return {
        stdin = stdin,
        stdout = stdout,
        stderr = stderr,
        wait = function(self) return proc:wait() end,
        kill = function(self) return self:kill() end,
        term = function(self) return self:kill('SIGTERM') end,
    }
end

function wish.async.zpty(...)
    local proc, pty = wish.__zpty(...)
    return {
        pty = pty,
        wait = function(self) return proc:wait() end,
        kill = function(self) return self:kill() end,
        term = function(self) return self:kill('SIGTERM') end,
    }
end

function wish.eval(args)
    local proc, stdin, stdout, stderr = wish.__shell_run{args = args, stdout = 'piped'}
    local stdout = stdout:read_all()
    local code = proc:wait()
    wish.pprint({stdout=stdout})
    return code, stdout
end

function wish.shell_split(str)
    return wish.in_param_scope(function()
        wish.set_var('str', str)
        wish.cmd[[ () { emulate -LR zsh; str=( ${(z)str} ); } ]].wait()
        return wish.get_var('str')
    end)
end
