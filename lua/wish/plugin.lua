return function(plugin_fn)
    local state = {
        enabled = false,
        event_callbacks = {},
        keymap_layers = {},
        plugin_keymap_layer = nil,
        messages = {},
        highlight_namespaces = {},
        processes = {},
        vars = {},
    }

    local plugin_obj = {}

    function plugin_obj.is_enabled()
        return state.enabled
    end

    function plugin_obj.disable()
        if not state.enabled then
            return
        end
        state.enabled = false

        -- kill all processes
        for i = #state.processes, 1, -1 do
            local proc = state.processes[i]
            if not proc:is_finished() then
                proc:term()
            end
            state.processes[i] = nil
        end

        -- clear all buffer highlights
        for i = #state.highlight_namespaces, 1, -1 do
            wish.clear_buf_highlights(state.highlight_namespaces[i])
            state.highlight_namespaces[i] = nil
        end

        -- remove event callbacks
        for i = #state.event_callbacks, 1, -1 do
            wish.remove_event_callback(state.event_callbacks[i])
            state.event_callbacks[i] = nil
        end

        -- delete keymap layers
        for i = #state.keymap_layers, 1, -1 do
            wish.del_keymap_layer(state.keymap_layers[i])
            state.keymap_layers[i] = nil
        end
        state.plugin_keymap_layer = nil

        -- remove messages
        for id in pairs(state.messages) do
            wish.remove_message(id)
            state.messages[id] = nil
        end

        -- unset vars
        for i = #state.vars, 1, -1 do
            wish.unset_var(state.vars[i])
            state.vars[i] = nil
        end

    end

    function plugin_obj.enable(config)
        if state.enabled then
            return
        end
        state.enabled = true

        local function track_process(handle)
            if handle then
                table.insert(state.processes, handle)
            end
            return handle
        end

        -- Create sub-proxies
        local async_proxy = setmetatable({
            spawn = function(...)
                return track_process(wish.async.spawn(...))
            end,
            zpty = function(...)
                return track_process(wish.async.zpty(...))
            end,
        }, { __index = wish.async })

        -- Create the main wish proxy
        local proxy
        proxy = setmetatable({
            async = async_proxy,

            add_event_callback = function(...)
                local id = wish.add_event_callback(...)
                table.insert(state.event_callbacks, id)
                return id
            end,

            set_keymap = function(key, cb, layer)
                if not layer then
                    if not state.plugin_keymap_layer then
                        state.plugin_keymap_layer = proxy.add_keymap_layer()
                    end
                    layer = state.plugin_keymap_layer
                end
                return wish.set_keymap(key, cb, layer)
            end,

            add_keymap_layer = function(...)
                local layer = wish.add_keymap_layer(...)
                table.insert(state.keymap_layers, layer)
                return layer
            end,

            set_message = function(opts)
                local id = wish.set_message(opts)
                if not opts.id then
                    state.messages[id] = true
                end
                return id
            end,

            remove_message = function(id)
                state.messages[id] = nil
                return wish.remove_message(id)
            end,

            add_buf_highlight_namespace = function(...)
                local ns = wish.add_buf_highlight_namespace(...)
                table.insert(state.highlight_namespaces, ns)
                return ns
            end,

            cmd = function(...)
                return track_process(wish.cmd(...))
            end,

            silent_cmd = function(...)
                return track_process(wish.silent_cmd(...))
            end,

            create_dynamic_var = function(name, ...)
                wish.create_dynamic_var(name, ...)
                table.insert(vars, name)
            end,

        }, { __index = wish })

        wish.try{
            try = function()
                plugin_fn(proxy, config, plugin_obj)

                if config and config.keybinds then
                    for k, v in pairs(config.keybinds) do
                        proxy.set_keymap(k, plugin_obj[v])
                    end
                end
            end,
            finally = function(err)
                if err then
                    plugin_obj.disable()
                end
            end,
        }
    end

    return plugin_obj
end
