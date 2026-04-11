return wish.plugin(function(wish, opts, plugin)

    local msg_id
    wish.add_event_callback('init', function()
        -- Create a message widget with a border
        msg_id = wish.set_message{
            text = 'Click me! Mouse demo widget.\nHover over this area.',
            border = { enabled = true, title = 'Mouse Demo' },
            persist = true,
        }
        wish.redraw()

        -- Enable SGR mouse mode
        wish.enable_mouse_mode(true)
    end)

    -- Hook into mouse events
    wish.add_event_callback('mouse', function(event, data)
        wish.pprint(event)

        -- Get the geometry of our message widget
        local geom = msg_id and wish.get_message_geometry(msg_id)
        if not geom then
            return
        end

        wish.pprint(geom)
        -- Hit test: is the mouse inside the widget?
        local inside = geom.x <= event.x and event.x < geom.x + geom.width and geom.y <= event.y and event.y < geom.y + geom.height
        wish.pprint(inside)

        -- -- Also check status bar
        -- local sb_geom = wish.get_status_bar_geometry()
        -- local in_status = false
        -- if sb_geom then
            -- local sx, sy, sw, sh = sb_geom[1], sb_geom[2], sb_geom[3], sb_geom[4]
            -- in_status = mx >= sx and mx < sx + sw and my >= sy and my < sy + sh
        -- end

        -- Update the widget text to show mouse info
        -- local text = string.format(
            -- 'Mouse: %s at (%d, %d)\nWidget: (%d, %d) %dx%d\nInside widget: %s\nInside status bar: %s',
            -- event.key, mx, my,
            -- wx, wy, ww, wh,
            -- tostring(inside),
            -- tostring(in_status)
        -- )
        wish.set_message{
            id = msg_id,
            border = {
                enabled = true,
                title = 'Mouse Demo',
                fg = inside and 'green' or 'red',
            },
        }
        wish.redraw()
    end)

    -- function plugin.disable_hook()
        -- wish.enable_mouse_mode(false)
    -- end
end)
