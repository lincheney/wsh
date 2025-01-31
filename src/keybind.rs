use std::collections::HashMap;
use anyhow::Result;
use mlua::{prelude::*, Function};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crate::ui::UiState;

pub type KeybindMapping = HashMap<(KeyCode, KeyModifiers), Function>;


fn set_keymap(state: &UiState, _lua: &Lua, (key, callback): (String, Function)) -> LuaResult<()> {
    let mut modifiers = KeyModifiers::empty();

    let original = &key;
    let mut key = key.as_str();
    if key.starts_with("<") && key.ends_with(">") && key.contains('-') {
        // this has modifiers
        key = &key[1..key.len()-1];
        for modifier in key.rsplit('-').skip(1) {
            match modifier {
                "c" => modifiers |= KeyModifiers::CONTROL,
                "s" => modifiers |= KeyModifiers::SHIFT,
                "a" => modifiers |= KeyModifiers::ALT,
                _ => return Err(mlua::Error::RuntimeError(format!("invalid keybind: {original:?}"))),
            }
        }
        key = key.rsplit('-').next().unwrap();
    }

    let key = match key {
        "<bs>" => KeyCode::Backspace,
        "<cr>" => KeyCode::Enter,
        "<left>" => KeyCode::Left,
        "<right>" => KeyCode::Right,
        "<up>" => KeyCode::Up,
        "<down>" => KeyCode::Down,
        "<home>" => KeyCode::Home,
        "<end>" => KeyCode::End,
        "<pageup>" => KeyCode::PageUp,
        "<pagedown>" => KeyCode::PageDown,
        "<tab>" => KeyCode::Tab,
        "<backtab>" => KeyCode::BackTab,
        "<delete>" => KeyCode::Delete,
        "<insert>" => KeyCode::Insert,
        "<null>" => KeyCode::Null,
        "<esc>" => KeyCode::Esc,
        "<capslock>" => KeyCode::CapsLock,
        "<scrolllock>" => KeyCode::ScrollLock,
        "<numlock>" => KeyCode::NumLock,
        "<printscreen>" => KeyCode::PrintScreen,
        "<pause>" => KeyCode::Pause,
        "<menu>" => KeyCode::Menu,

        "<lt>" => KeyCode::Char('<'),
        key if key.len() == 1 && &key[0..1] != "<" && key.is_ascii() => KeyCode::Char(key.chars().next().unwrap()),
        key if key.starts_with("<f") && key.ends_with(">") && key[2..key.len()-1].parse::<u8>().is_ok() => {
            KeyCode::F(key[2..key.len()-1].parse::<u8>().unwrap())
        },

        key => {
            return Err(mlua::Error::RuntimeError(format!("invalid keybind: {original:?}")))
        },
    };

    let mut state = state.borrow_mut();
    state.keybinds.insert((key, modifiers), callback);

    Ok(())
}

pub fn init_lua(ui: &crate::ui::Ui) -> Result<()> {

    ui.set_lua_fn("set_keymap", set_keymap)?;

    Ok(())
}
