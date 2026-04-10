use serde::{Serialize};
use std::collections::HashMap;
use std::default::Default;
use anyhow::Result;
use mlua::{prelude::*, Function};
use crate::keybind::event::{Event, EventIndex};
use crate::ui::{Ui};

#[derive(Default)]
pub struct KeybindMapping {
    id: usize,
    pub inner: HashMap<EventIndex, Function>,
    pub no_fallthrough: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum EventPayload {
    Mouse{x: usize, y: usize},
}

impl mlua::IntoLua for EventPayload {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
        lua.to_value(&self)
    }
}

impl TryFrom<&Event> for EventPayload {
    type Error = ();
    fn try_from(value: &Event) -> Result<Self, Self::Error> {
        match value {
            Event::Mouse(ev) => Ok(Self::Mouse{x: ev.x, y: ev.y}),
            _ => Err(()),
        }
    }
}

pub async fn invoke_keybind_callback(ui: &Ui, event: &Event) -> bool {

    let Ok(index) = event.try_into() else {
        return false;
    };

    // look for a lua callback
    let callback = {
        let ui = ui.get();
        let ui = ui.borrow();
        let mut keymaps = ui.keybinds.iter().rev();
        loop {
            let Some(k) = keymaps.next() else {
                return false;
            };
            if let Some(callback) = k.inner.get(&index) {
                break callback.clone();
            } else if k.no_fallthrough {
                return true;
            };
        }
    };

    let payload: Option<EventPayload> = event.try_into().ok();
    ui.call_lua_fn(true, callback, payload).await;
    true
}

async fn set_keymap(ui: Ui, _lua: Lua, (key, callback, layer): (String, Function, Option<usize>)) -> Result<()> {
    let key = EventIndex::parse_from_label(&key)?;

    let ui = ui.get();
    let mut ui = ui.borrow_mut();
    let layer = if let Some(layer) = layer {
        if let Some(layer) = ui.keybinds.iter_mut().find(|k| k.id == layer) {
            layer
        } else {
            return Err(anyhow::anyhow!("invalid layer: {:?}", layer))
        }
    } else {
        ui.keybinds.last_mut().unwrap()
    };
    layer.inner.insert(key, callback);

    Ok(())
}

async fn add_keymap_layer(ui: Ui, _lua: Lua, no_fallthrough: Option<bool>) -> Result<usize> {
    let no_fallthrough = no_fallthrough.unwrap_or(false);
    let ui = ui.get();
    let mut ui = ui.borrow_mut();
    ui.keybind_layer_counter += 1;
    let id = ui.keybind_layer_counter;
    ui.keybinds.push(KeybindMapping{id, inner: HashMap::default(), no_fallthrough});
    Ok(id)
}

async fn del_keymap_layer(ui: Ui, _lua: Lua, layer: usize) -> Result<()> {
    let ui = ui.get();
    let mut ui = ui.borrow_mut();
    ui.keybinds.retain(|k| k.id != layer);
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("set_keymap", set_keymap)?;
    ui.set_lua_async_fn("add_keymap_layer", add_keymap_layer)?;
    ui.set_lua_async_fn("del_keymap_layer", del_keymap_layer)?;

    Ok(())
}
