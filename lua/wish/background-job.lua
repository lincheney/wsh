local M = {}

local jobs = {}
local active_job = nil

M.PROMPT_TIMEOUT = 0.04 -- same as rlwrap

M.BORDER_RUNNING = 'lightblue'
M.BORDER_WAITING = M.BORDER_RUNNING
M.BORDER_SUCCEEDED = 'lightgreen'
M.BORDER_FAILED = '#ffaaaa'

M.TITLE_RUNNING = 'blue'
M.TITLE_WAITING = M.TITLE_RUNNING
M.TITLE_SUCCEEDED = 'green'
M.TITLE_FAILED = 'red'

local function update_message(job)
    local props = {
        id = job.msg,
        border = {
            title = {fg = M.TITLE_RUNNING, bold = true, text = ' ' .. job.command .. ' ' },
        },
    }

    if active_job and job == active_job.job then
        props.dim = false
    else
        props.dim = true
    end

    if job.output_marker == 0 then
        props.border.sides = 'Top'
        props.border.title = {
            { fg = M.BORDER_RUNNING, text = '─' },
            props.border.title,
        }
    else
        props.border.sides = 'All'
    end

    if job.code then
        props.border.title.fg = job.code > 0 and M.TITLE_FAILED or M.TITLE_SUCCEEDED
        if job.output_marker > 0 then
            props.border.title = {
                { fg = job.code > 0 and M.BORDER_FAILED or M.BORDER_SUCCEEDED, text = '─' },
                props.border.title,
            }
        end
        props.border.dim = false
        props.border.fg = job.code > 0 and M.BORDER_FAILED or M.BORDER_SUCCEEDED
    elseif job.waiting_for_input then
        props.border.fg = M.BORDER_WAITING
        props.border.title.fg = M.TITLE_WAITING
        props.border.title = {
            {text = ' '},
            {fg = 'black', bg = M.TITLE_WAITING, dim = false, bold = true, text = 'input' },
            props.border.title,
        }
    else
        props.border.fg = M.BORDER_RUNNING
    end

    wish.set_message(props)
    wish.redraw()
end

local function unfocus()
    if active_job then
        local job = active_job.job
        -- focus back on the buffer
        if active_job.keymap_layer then
            wish.del_keymap_layer(active_job.keymap_layer)
        end
        if active_job.key_event_id then
            wish.remove_event_callback(active_job.key_event_id)
        end
        active_job = nil
        if job then
            update_message(job)
        end
    end
end

function M.run_in_background(command)
    wish.schedule(function()

        local msg = wish.set_message{
            persist = true,
            height = 'max:7',
            border = {
                type = 'Rounded',
                show_empty = true,
            },
        }
        local job = {
            msg = msg,
            command = command,
            output_marker = 0,
        }
        jobs[msg] = job
        update_message(job)

        job.proc = wish.async.zpty(command)
        while true do
            local data = job.proc.stdout:read()
            if not data then
                break
            end

            -- got some data
            wish.feed_ansi_message(msg, data)
            if string.find(data, '%S') then
                if job.output_marker == 0 or job.waiting_for_input then
                    job.waiting_for_input = false
                    update_message(job)
                end

                -- check if it matches a prompt
                local lines = wish.get_message_text(msg)
                if string.find(lines[#lines], '[:?] ?$') then
                    -- its a prompt, wait a bit to see if there is any more output
                    wish.schedule(function()
                        local marker = job.output_marker
                        wish.sleep(M.PROMPT_TIMEOUT)
                        -- marker has not changed, so output/prompt has not changed
                        if marker == job.output_marker and jobs[msg] then
                            job.waiting_for_input = true
                            update_message(job)
                        end
                    end)
                end

                job.output_marker = job.output_marker + 1
            end
            wish.redraw()
        end

        job.code = job.proc:wait()
        if active_job and job == active_job.job then
            unfocus()
        else
            update_message(job)
        end
        local output = wish.message_to_ansi_string(msg)
        jobs[msg] = nil
        wish.remove_message(msg)

        wish.print(output)
    end)
end

function M.focus_next_job(exit_key)
    local msg, job = next(jobs, active_job and active_job.job.msg)
    if msg then
        active_job = active_job or {
            key_event_id = wish.add_event_callback('key', function(key, data)
                if wish.iter(exit_key):all(function(k, v) return key[k] == v end) then
                    -- run it later or the user keybind may trigger immediately
                    wish.schedule(function()
                        M.focus_next_job(exit_key)
                    end)
                else
                    active_job.job.proc.stdin:write(data)
                end
            end),
            keymap_layer = wish.add_keymap_layer(true),
        }
        active_job.job = job
        update_message(job)
    else
        unfocus()
    end
end

return M
