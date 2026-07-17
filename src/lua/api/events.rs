use std::cell::{Cell, RefCell};
use std::rc::Rc;
use crate::lua::{LuaWrapper, auto_from_lua};
use bstr::BString;
use anyhow::Result;
use mlua::{prelude::*, Function};
use serde::{Serialize};
use crate::ui::Ui;
use crate::keybind;

#[derive(Default)]
struct CallbackVec {
    inner: RefCell<Rc<Vec<(usize, Function)>>>,
}

impl CallbackVec {
    fn get_owned(&self) -> Rc<Vec<(usize, Function)>> {
        self.inner.borrow().clone()
    }

    fn modify<F: FnOnce(&mut Vec<(usize, Function)>) -> T, T>(&self, f: F) -> T{
        let mut inner = self.inner.borrow_mut();
        if let Some(inner) = Rc::get_mut(&mut inner) {
            f(inner)
        } else {
            // only clone the entire vec if necessary
            let mut vec = Rc::unwrap_or_clone(inner.clone());
            let result = f(&mut vec);
            *inner = Rc::new(vec);
            result
        }
    }

    fn add(&self, id: usize, cb: Function) {
        self.modify(|vec| vec.push((id, cb)));
    }

    fn remove(&self, id: usize) -> bool {
        self.modify(|vec| {
            if let Some(ix) = vec.iter().position(|(i, _)| *i == id) {
                vec.remove(ix);
                true
            } else {
                false
            }
        })
    }
}

async fn trigger_callbacks_multi_value(ui: &Ui, callbacks: &[(usize, Function)], args: LuaMultiValue) {
    for (_, cb) in callbacks {
        crate::log_if_err(ui.call_lua_fn(false, cb.clone(), args.clone()).await);
    }
}

macro_rules! event_types {
    ($( $name:ident($($arg:ident : $type:ty),*), )*) => (

        auto_from_lua! {
            #[derive(Debug, Clone, Copy)]
            pub enum EventType {
            $(
                #[allow(non_camel_case_types)]
                $name,
            )*
            }
        }

        #[derive(Default)]
        pub struct EventCallbacks {
            counter: Cell<usize>,
        $(
            $name: CallbackVec,
        )*
        }

        impl EventCallbacks {

            fn get_callbacks(&self, typ: EventType) -> &CallbackVec {
                match typ {
                $( EventType::$name => &self.$name, )*
                }
            }

            fn add_event_callback(&self, typ: EventType, cb: Function) -> usize {
                let counter = self.counter.get();
                self.get_callbacks(typ).add(counter, cb);
                self.counter.set(counter + 1);
                counter
            }

            fn remove_event_callback(&self, id: usize) {
            $(
                if self.$name.remove(id) {
                    return;
                }
            )*
            }

        $(
            pub async fn $name(&self, ui: &Ui, $($arg: $type),*) -> Result<()> {
                let callbacks = self.$name.get_owned();
                if !callbacks.is_empty() {
                    let args = ($(
                        ui.lua.to_value_with(
                            &$arg,
                            mlua::SerializeOptions::new().serialize_none_to_null(false),
                        ).unwrap()
                    ),*);
                    let args = ui.lua.pack_multi(args).unwrap();
                    trigger_callbacks_multi_value(&ui, &callbacks, args).await;
                }
                Ok(())
            }
        )*
        }

        async fn trigger_event_callback(ui: Ui, _lua: Lua, (event, args): (String, LuaMultiValue)) -> Result<()> {
            let callbacks = match event.as_ref() {
                $(
                stringify!($name) => ui.event_callbacks.$name.get_owned(),
                )*
                _ => anyhow::bail!("invalid event {event}"),
            };
            trigger_callbacks_multi_value(&ui, &callbacks, args).await;
            Ok(())
        }

    )
}


auto_from_lua! {
    #[derive(Debug, Serialize, Clone)]
    pub struct KeyEvent {
        key: String,
        control: bool,
        shift: bool,
        alt: bool,
    }
}

impl From<keybind::KeyEvent> for KeyEvent {
    fn from(ev: keybind::KeyEvent) -> Self {
        Self {
            key: ev.key.to_string(),
            control: ev.modifiers.contains(keybind::Modifiers::CONTROL),
            shift: ev.modifiers.contains(keybind::Modifiers::SHIFT),
            alt: ev.modifiers.contains(keybind::Modifiers::ALT),
        }
    }
}

auto_from_lua! {
    #[derive(Debug, Serialize, Clone)]
    pub struct MouseEvent {
        pub key: String,
        pub control: bool,
        pub shift: bool,
        pub alt: bool,
        pub x: usize,
        pub y: usize,
    }
}

impl From<keybind::MouseEvent> for MouseEvent {
    fn from(ev: keybind::MouseEvent) -> Self {
        Self {
            key: ev.mouse.to_string(),
            control: ev.modifiers.contains(keybind::Modifiers::CONTROL),
            shift: ev.modifiers.contains(keybind::Modifiers::SHIFT),
            alt: ev.modifiers.contains(keybind::Modifiers::ALT),
            x: ev.x,
            y: ev.y,
        }
    }
}

event_types!(
    init(),
    key(key: &KeyEvent, data: &BString),
    mouse(event: &MouseEvent, data: &BString),
    accept_line(data: &BString),
    buffer_change(),
    buffer_cursor_move(),
    precmd(data: Option<&BString>),
    paste(data: &BString),
    window_resize(width: u32, height: u32),
    message_resize(ids: &[usize]),
    exit(val: i32),
);


fn add_event_callback(ui: &Ui, _lua: &Lua, (typ, callback): (EventType, Function)) -> Result<usize> {
    Ok(ui.event_callbacks.add_event_callback(typ, callback))
}

fn remove_event_callback(ui: &Ui, _lua: &Lua, id: usize) -> Result<()> {
    ui.event_callbacks.remove_event_callback(id);
    Ok(())
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_fn("add_event_callback", add_event_callback)?;
    lua.set_fn("remove_event_callback", remove_event_callback)?;
    lua.set_async_fn("trigger_event_callback", trigger_event_callback)?;

    Ok(())
}

