use bstr::BString;
use anyhow::Result;
use mlua::{prelude::*, Function};
use serde::{Deserialize, Serialize};
use crate::ui::Ui;

fn get_non_empty_owned_callbacks(ui: &Ui, typ: EventType) -> Option<Vec<(usize, Function)>> {
    let ui = ui.get();
    let callbacks = ui.event_callbacks.lock().unwrap();
    let callbacks = callbacks.get_callbacks(typ);
    if callbacks.is_empty() {
        None
    } else {
        Some(callbacks.clone())
    }
}

async fn trigger_callbacks_multi_value(ui: &Ui, callbacks: Vec<(usize, Function)>, args: LuaMultiValue) {
    for (_, cb) in callbacks {
        ui.call_lua_fn(false, cb, args.clone()).await;
    }
}

macro_rules! event_types {
    ($( $name:ident($($arg:ident : $type:ty),*), )*) => (

        #[derive(Debug, Deserialize, Clone, Copy)]
        pub enum EventType {
        $(
            #[allow(non_camel_case_types)]
            $name,
        )*
        }

    paste::paste!{

        #[derive(Default)]
        pub struct EventCallbacks {
            counter: usize,
        $(
            pub [<callbacks_ $name>]: Vec<(usize, Function)>,
        )*
        }

        impl EventCallbacks {
            #[allow(dead_code)]
            fn get_callbacks(&self, typ: EventType) -> &Vec<(usize, Function)> {
                match typ {
                $( EventType::$name => &self.[<callbacks_ $name>], )*
                }
            }

            fn get_callbacks_mut(&mut self, typ: EventType) -> &mut Vec<(usize, Function)> {
                match typ {
                $( EventType::$name => &mut self.[<callbacks_ $name>], )*
                }
            }

            fn add_event_callback(&mut self, typ: EventType, cb: Function) -> usize {
                let counter = self.counter;
                self.get_callbacks_mut(typ).push((counter, cb));
                self.counter += 1;
                counter
            }

            fn remove_event_callback(&mut self, id: usize) {
            $(
                let callbacks = &mut self.[<callbacks_ $name>];
                if let Some(i) = callbacks.iter().position(|(i, _)| *i == id) {
                    callbacks.remove(i);
                    return
                }
            )*
            }

        }

        pub trait HasEventCallbacks {
        $(
            #[allow(unused_parens)]
            async fn [<trigger_ $name _callbacks>](&self, $($arg: &$type),*);
        )*
        }

        impl HasEventCallbacks for Ui {
        $(
            #[allow(unused_parens)]
            async fn [<trigger_ $name _callbacks>](&self, $($arg: &$type),*) {
                if let Some(callbacks) = get_non_empty_owned_callbacks(&self, EventType::$name) {
                    let args = ($(self.lua.to_value($arg).unwrap()),*);
                    let args = self.lua.pack_multi(args).unwrap();
                    trigger_callbacks_multi_value(&self, callbacks, args).await;
                }
            }
        )*
        }

        async fn trigger_event_callback(ui: Ui, _lua: Lua, (event, args): (String, LuaMultiValue)) -> Result<()> {
            let callbacks = match event.as_ref() {
                $(
                stringify!($name) => get_non_empty_owned_callbacks(&ui, EventType::$name),
                )*
                _ => anyhow::bail!("invalid event {event}"),
            };

            if let Some(callbacks) = callbacks {
                trigger_callbacks_multi_value(&ui, callbacks, args).await;
            }
            Ok(())
        }

    }

    )
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeyEvent {
    key: String,
    control: bool,
    shift: bool,
    alt: bool,
}

impl From<crate::keybind::parser::KeyEvent> for KeyEvent {
    fn from(ev: crate::keybind::parser::KeyEvent) -> Self {
        Self {
            key: ev.key.to_string(),
            control: ev.modifiers.contains(crate::keybind::parser::KeyModifiers::CONTROL),
            shift: ev.modifiers.contains(crate::keybind::parser::KeyModifiers::SHIFT),
            alt: ev.modifiers.contains(crate::keybind::parser::KeyModifiers::ALT),
        }
    }
}

event_types!(
    key(key: KeyEvent, data: BString),
    accept_line(),
    buffer_change(),
    precmd(),
    paste(data: BString),
    window_resize(width: u32, height: u32),
);


fn add_event_callback(ui: &Ui, lua: &Lua, (typ, callback): (LuaValue, Function)) -> Result<usize> {
    let typ: EventType = lua.from_value(typ)?;
    Ok(ui.get().event_callbacks.lock().unwrap().add_event_callback(typ, callback))
}

fn remove_event_callback(ui: &Ui, _lua: &Lua, id: usize) -> Result<()> {
    ui.get().event_callbacks.lock().unwrap().remove_event_callback(id);
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_fn("add_event_callback", add_event_callback)?;
    ui.set_lua_fn("remove_event_callback", remove_event_callback)?;
    ui.set_lua_async_fn("trigger_event_callback", trigger_event_callback)?;

    Ok(())
}

