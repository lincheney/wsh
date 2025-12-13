function wish.repr(val, multiline, indent)
    if type(val) == 'table' then
        indent = indent or ''
        local text = {}
        for k, v in ipairs(val) do
            table.insert(text, wish.repr(v, multiline, indent .. '  '))
        end
        for k, v in pairs(val) do
            if type(k) == 'string' and not k:find('[^%w_]') then
                table.insert(text, k .. ' = ' .. wish.repr(v, multiline, indent .. '  '))
            elseif type(k) ~= 'number' or k > #val then
                table.insert(text, '['..wish.repr(k)..'] = ' .. wish.repr(v, multiline, indent .. '  '))
            end
        end

        -- multiline only if too long or newlines
        if multiline and (#text > 2 or #table.concat(text, '') > 20 or string.find(table.concat(text, ''), '\n')) then
            return '{\n' .. indent .. '  ' .. table.concat(text, ',\n' .. indent .. '  ') .. '\n' .. indent .. '}'
        else
            return '{' .. table.concat(text, ', ') .. '}'
        end
    elseif type(val) == 'string' then
        local val = string.format('%q', val):gsub('\\\n', '\\n')
        return val
    else
        return tostring(val)
    end
end

function wish.pprint(...)
    local val = table.concat(wish.iter{...}:map(function(k, v) return wish.repr(v) end):collect(), ' ')
    wish.log.debug(val)
end

function wish.async.spawn(...)
    local proc, stdin, stdout, stderr = wish.__spawn(...)
    return {
        stdin = stdin,
        stdout = stdout,
        stderr = stderr,
        pid = function(self) return proc:pid() end,
        is_finished = function(self) return proc:is_finished() end,
        wait = function(self) return proc:wait() end,
        kill = function(self, ...) return proc:kill(...) end,
        term = function(self) return proc:kill('SIGTERM') end,
    }
end

function wish.cmd(...)
    local proc, stdin, stdout, stderr = wish.__shell_run(...)
    return {
        stdin = stdin,
        stdout = stdout,
        stderr = stderr,
        wait = function(self) return proc:wait() end,
        kill = function(self, ...) return proc:kill(...) end,
        term = function(self) return proc:kill('SIGTERM') end,
    }
end

function wish.async.zpty(...)
    local proc, pty = wish.__zpty(...)
    return {
        pty = pty,
        is_finished = function(self) return proc:is_finished() end,
        wait = function(self) return proc:wait() end,
        kill = function(self, ...) return proc:kill(...) end,
        term = function(self) return proc:kill('SIGTERM') end,
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
