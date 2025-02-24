use anyhow::Result;
use mlua::{prelude::*, Function};
use serde::{Deserialize, Serialize};
use crate::ui::Ui;
use crate::shell::Shell;
use crossterm::event;

macro_rules! event_types {
    ($( $name:ident($($arg:ty),*), )*) => (

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
            [<callbacks_ $name>]: Vec<(usize, Function)>,
        )*
        }

        impl EventCallbacks {
        $(
            #[allow(unused_parens)]
            pub fn [<trigger_ $name _callbacks>](&self, ui: &Ui, shell: &Shell, lua: &Lua, val: ($($arg),*)) {
                let val = lua.to_value(&val).unwrap();
                for (_, cb) in self.[<callbacks_ $name>].iter() {
                    ui.call_lua_fn(shell.clone(), false, cb.clone(), val.clone());
                }
            }

            pub fn [<has_ $name _callbacks>](&self) -> bool {
                !self.[<callbacks_ $name>].is_empty()
            }
        )*

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
                self.counter = self.counter + 1;
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
    }

    )
}


#[derive(Debug, Serialize, Clone)]
pub struct KeyEvent {
    key: String,
    control: bool,
    shift: bool,
    alt: bool,
}

impl From<event::KeyEvent> for KeyEvent {
    fn from(ev: event::KeyEvent) -> Self {
        Self{
            key: ev.code.to_string(),
            control: ev.modifiers.contains(event::KeyModifiers::CONTROL),
            shift: ev.modifiers.contains(event::KeyModifiers::SHIFT),
            alt: ev.modifiers.contains(event::KeyModifiers::ALT),
        }
    }
}

event_types!(
    key(KeyEvent),
    accept_line(),
);


async fn add_event_callback(mut ui: Ui, _shell: Shell, lua: Lua, (typ, callback): (LuaValue, Function)) -> Result<usize> {
    let typ: EventType = lua.from_value(typ)?;
    Ok(ui.borrow_mut().await.event_callbacks.add_event_callback(typ, callback))
}

async fn remove_event_callback(mut ui: Ui, _shell: Shell, _lua: Lua, id: usize) -> Result<()> {
    ui.borrow_mut().await.event_callbacks.remove_event_callback(id);
    Ok(())
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("add_event_callback", shell, add_event_callback).await?;
    ui.set_lua_async_fn("remove_event_callback", shell, remove_event_callback).await?;

    Ok(())
}

