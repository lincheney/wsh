use std::collections::HashMap;
use std::default::Default;
use anyhow::Result;
use mlua::{prelude::*, Function};
use crate::keybind::parser::{Key, KeyModifiers};
use crate::ui::{Ui, ThreadsafeUiInner};

#[derive(Default)]
pub struct KeybindMapping {
    id: usize,
    pub inner: HashMap<(Key, KeyModifiers), Function>,
}


async fn set_keymap(ui: Ui, _lua: Lua, (key, callback, layer): (String, Function, Option<usize>)) -> Result<()> {
    let mut modifiers = KeyModifiers::empty();

    let original = &key;
    let mut key = key.as_str();
    let special = key.starts_with('<') && key.ends_with('>');

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
        "bs" if special => Key::Backspace,
        "cr" if special => Key::Enter,
        "left" if special => Key::Left,
        "right" if special => Key::Right,
        "up" if special => Key::Up,
        "down" if special => Key::Down,
        "home" if special => Key::Home,
        "end" if special => Key::End,
        "pageup" if special => Key::Pageup,
        "pagedown" if special => Key::Pagedown,
        "tab" if special => Key::Char('\t'),
        // "backtab" if special => Key::BackTab,
        "delete" if special => Key::Delete,
        "insert" if special => Key::Insert,
        "esc" if special => Key::Escape,
        // "capslock" if special => Key::CapsLock,
        // "scrolllock" if special => Key::ScrollLock,
        // "numlock" if special => Key::NumLock,
        // "printscreen" if special => Key::PrintScreen,
        // "pause" if special => Key::Pause,
        // "menu" if special => Key::Menu,

        "lt" if special => Key::Char('<'),
        key if key.len() == 1 && &key[0..1] != "<" && key.is_ascii() => Key::Char(key.chars().next().unwrap()),
        key if special && key.starts_with('f') && key[1..].parse::<u8>().is_ok() => {
            Key::Function(key[1..].parse().unwrap())
        },

        _ => {
            return Err(anyhow::anyhow!("invalid keybind: {:?}", original))
        },
    };

    let ui = ui.get();
    let mut ui = ui.inner.borrow_mut();
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

async fn add_keymap_layer(ui: Ui, _lua: Lua, _val: ()) -> Result<usize> {
    let ui = ui.get();
    let mut ui = ui.inner.borrow_mut();
    ui.keybind_layer_counter += 1;
    let id = ui.keybind_layer_counter;
    ui.keybinds.push(KeybindMapping{id, inner: HashMap::default()});
    Ok(id)
}

async fn del_keymap_layer(ui: Ui, _lua: Lua, layer: usize) -> Result<()> {
    let ui = ui.get();
    let mut ui = ui.inner.borrow_mut();
    ui.keybinds.retain(|k| k.id != layer);
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("set_keymap", set_keymap)?;
    ui.set_lua_async_fn("add_keymap_layer", add_keymap_layer)?;
    ui.set_lua_async_fn("del_keymap_layer", del_keymap_layer)?;

    Ok(())
}
