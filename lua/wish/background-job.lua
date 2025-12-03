local M = {}

M.BORDER_RUNNING = 'lightblue'
M.BORDER_SUCCEEDED = 'lightgreen'
M.BORDER_FAILED = '#ffaaaa'

M.TITLE_RUNNING = 'blue'
M.TITLE_SUCCEEDED = 'green'
M.TITLE_FAILED = 'red'

function M.run_in_background(command)
    wish.schedule(function()

        local msg = wish.set_ansi_message{
            persist = true,
            dim = true,
            border = {
                fg = M.BORDER_RUNNING,
                type = 'Rounded',
                title = {
                    { fg = M.BORDER_RUNNING, text = '─' },
                    { fg = M.TITLE_RUNNING, bold = true, text = ' ' .. command .. ' '},
                },
                sides = 'Top',
                show_empty = true,
            },
        }
        wish.redraw()

        local proc = wish.async.zpty(command)
        local has_output = false
        while true do
            local data = proc.pty:read()
            if not data then
                break
            end

            wish.feed_ansi_message(msg, data)
            if not has_output and string.find(data, '%S') then
                has_output = true
                wish.set_message{
                    id = msg,
                    border = {
                        sides = 'All',
                        title = {fg = M.TITLE_RUNNING, bold = true, text = ' ' .. command .. ' ' },
                    },
                }
            end
            wish.redraw()
        end

        local code = proc:wait()

        local title = {
            fg = code > 0 and M.TITLE_FAILED or M.TITLE_SUCCEEDED,
            bold = true,
            text = ' ' .. command .. ' ',
        }
        if not has_output then
            title = {
                { fg = code > 0 and M.BORDER_FAILED or M.BORDER_SUCCEEDED, text = '─' },
                title,
            }
        end

        wish.set_message{
            id = msg,
            border = {
                dim = false,
                fg = code > 0 and M.BORDER_FAILED or M.BORDER_SUCCEEDED,
                title = title,
            },
        }
        local output = wish.message_to_ansi_string(msg)
        wish.remove_message(msg)

        wish.print(output)
    end)
end

return M
