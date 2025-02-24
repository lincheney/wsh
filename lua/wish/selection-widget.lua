local M = {}

local selection_widget = nil

function M.show(opts)
    if not (selection_widget and selection_widget:exists()) then
        selection_widget = wish.show_message{}
    end
    selection_widget:set_options(opts)
    wish.redraw()
    return selection_widget
end

function M.hide()
    if selection_widget and selection_widget:exists() then
        selection_widget:set_options{visible = false}
        wish.redraw()
    end
end

return M
