use anyhow::Result;
use mlua::{prelude::*, Function};
use serde::{Deserialize};
use crate::ui::Ui;
use crate::shell::Shell;

macro_rules! event_types {
    ($( $name:ident($($arg:ty),*) ),*) => (

        #[derive(Debug, Deserialize, Clone, Copy)]
        pub enum EventType {
            #[allow(non_camel_case_types)]
            $($name),*
        }

    paste::paste!{

        #[derive(Default)]
        pub struct EventCallbacks {
        $(
            [<callbacks_ $name>]: Vec<Function>,
        ),*
        }

        impl EventCallbacks {
        $(
            pub fn [<trigger_ $name>](&self, ui: &Ui, shell: &Shell, val: ($($arg),*)) {
                for cb in self.[<callbacks_ $name>].iter() {
                    ui.call_lua_fn(shell.clone(), cb.clone(), val);
                }
            }
        ),*

            fn get_callbacks(&self, typ: EventType) -> &Vec<Function> {
                match typ {
                $( EventType::$name => &self.[<callbacks_ $name>], )*
                }
            }

            fn get_callbacks_mut(&mut self, typ: EventType) -> &mut Vec<Function> {
                match typ {
                $( EventType::$name => &mut self.[<callbacks_ $name>], )*
                }
            }


            fn add_event_callback(&mut self, typ: EventType, cb: Function) {
                self.get_callbacks_mut(typ).push(cb);
            }
        }
    }

    )
}

event_types!(key());


async fn add_event_callback(mut ui: Ui, _shell: Shell, lua: Lua, (typ, callback): (LuaValue, Function)) -> Result<()> {
    let typ: EventType = lua.from_value(typ)?;
    ui.borrow_mut().await.event_callbacks.add_event_callback(typ, callback);
    Ok(())
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("add_event_callback", shell, add_event_callback).await?;

    Ok(())
}

