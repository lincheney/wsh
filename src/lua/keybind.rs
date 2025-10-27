use std::collections::HashMap;
use std::default::Default;
use anyhow::Result;
use mlua::{prelude::*, Function};
use crossterm::event::{KeyCode, KeyModifiers};
use crate::ui::{Ui, ThreadsafeUiInner};

#[derive(Default)]
pub struct KeybindMapping {
    id: usize,
    pub inner: HashMap<(KeyCode, KeyModifiers), Function>,
}


async fn set_keymap(mut ui: Ui, _lua: Lua, (key, callback, layer): (String, Function, Option<usize>)) -> Result<()> {
    let mut modifiers = KeyModifiers::empty();

    let original = &key;
    let mut key = key.as_str();
    let special = key.starts_with("<") && key.ends_with(">");

    if special {
        key = &key[1..key.len()-1];

        if key.contains('-') {
            // this has modifiers
            for modifier in key.rsplit('-').skip(1) {
                match modifier {
                    "c" => modifiers |= KeyModifiers::CONTROL,
                    "s" => modifiers |= KeyModifiers::SHIFT,
                    "a" => modifiers |= KeyModifiers::ALT,
                    _ => return Err(anyhow::anyhow!("invalid keybind: {:?}", original)),
                }
            }
            key = key.rsplit('-').next().unwrap();
        }
    }

    let key = match key {
        "bs" if special => KeyCode::Backspace,
        "cr" if special => KeyCode::Enter,
        "left" if special => KeyCode::Left,
        "right" if special => KeyCode::Right,
        "up" if special => KeyCode::Up,
        "down" if special => KeyCode::Down,
        "home" if special => KeyCode::Home,
        "end" if special => KeyCode::End,
        "pageup" if special => KeyCode::PageUp,
        "pagedown" if special => KeyCode::PageDown,
        "tab" if special => KeyCode::Tab,
        "backtab" if special => KeyCode::BackTab,
        "delete" if special => KeyCode::Delete,
        "insert" if special => KeyCode::Insert,
        "null" if special => KeyCode::Null,
        "esc" if special => KeyCode::Esc,
        "capslock" if special => KeyCode::CapsLock,
        "scrolllock" if special => KeyCode::ScrollLock,
        "numlock" if special => KeyCode::NumLock,
        "printscreen" if special => KeyCode::PrintScreen,
        "pause" if special => KeyCode::Pause,
        "menu" if special => KeyCode::Menu,

        "lt" if special => KeyCode::Char('<'),
        key if key.len() == 1 && &key[0..1] != "<" && key.is_ascii() => KeyCode::Char(key.chars().next().unwrap()),
        key if special && key.starts_with("f") && key[1..].parse::<u8>().is_ok() => {
            KeyCode::F(key[1..].parse::<u8>().unwrap())
        },

        _ => {
            return Err(anyhow::anyhow!("invalid keybind: {:?}", original))
        },
    };

    let mut ui = ui.inner.borrow_mut().await;
    let layer = if let Some(layer) = layer {
        if let Some(layer) = ui.keybinds.iter_mut().find(|k| k.id == layer) {
            layer
        } else {
            return Err(anyhow::anyhow!("invalid layer: {:?}", layer))
        }
    } else {
        ui.keybinds.last_mut().unwrap()
    };
    layer.inner.insert((key, modifiers), callback);

    Ok(())
}

async fn add_keymap_layer(mut ui: Ui, _lua: Lua, _val: ()) -> Result<usize> {
    let mut ui = ui.inner.borrow_mut().await;
    ui.keybind_layer_counter += 1;
    let id = ui.keybind_layer_counter;
    ui.keybinds.push(KeybindMapping{id, inner: Default::default()});
    Ok(id)
}

async fn del_keymap_layer(mut ui: Ui, _lua: Lua, layer: usize) -> Result<()> {
    let mut ui = ui.inner.borrow_mut().await;
    ui.keybinds.retain(|k| k.id != layer);
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("set_keymap", set_keymap)?;
    ui.set_lua_async_fn("add_keymap_layer", add_keymap_layer)?;
    ui.set_lua_async_fn("del_keymap_layer", del_keymap_layer)?;

    Ok(())
}
