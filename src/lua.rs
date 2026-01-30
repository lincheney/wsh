use std::borrow::Cow;
use std::str::FromStr;
use serde::{Deserialize, Deserializer, de};
use bstr::BString;
use std::io::Write;
use mlua::prelude::*;
use anyhow::Result;
use crate::ui::{Ui};
use std::time::SystemTime;

mod keybind;
mod string;
mod completion;
mod history;
mod events;
mod tui;
mod log;
mod process;
mod asyncio;
mod parser;
mod variables;
mod functions;
mod regex;
pub use keybind::KeybindMapping;
pub use events::{EventCallbacks, HasEventCallbacks};

#[derive(Debug, Copy, Clone)]
pub struct SerdeWrap<T>(T);
impl<'de, T: FromStr> Deserialize<'de> for SerdeWrap<T>
    where <T as FromStr>::Err: std::fmt::Display
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let data = String::deserialize(deserializer)?;
        Ok(Self(T::from_str(&data).map_err(de::Error::custom)?))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RedrawOptions {
    prompt: bool,
    postdisplay: bool,
    predisplay: bool,
    buffer: bool,
    messages: bool,
    status_bar: bool,
    all: bool,
    now: bool,
}

fn get_cursor(ui: &Ui, _lua: &Lua, (): ()) -> Result<usize> {
    Ok(ui.get().borrow().buffer.get_cursor())
}

fn get_buffer(ui: &Ui, lua: &Lua, (): ()) -> Result<mlua::String> {
    Ok(lua.create_string(ui.get().borrow().buffer.get_contents())?)
}

fn set_cursor(ui: &Ui, _lua: &Lua, val: usize) -> Result<()> {
    ui.get().borrow_mut().buffer.set_cursor(val);
    Ok(())
}

async fn set_buffer(ui: Ui, _lua: Lua, (val, len): (mlua::String, Option<usize>)) -> Result<()> {
    ui.get().borrow_mut().buffer.splice_at_cursor(&val.as_bytes(), len);
    ui.trigger_buffer_change_callbacks(()).await;
    Ok(())
}

async fn undo_buffer(ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    if ui.get().borrow_mut().buffer.move_in_history(false) {
        ui.trigger_buffer_change_callbacks(()).await;
    }
    Ok(())
}

async fn redo_buffer(ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    if ui.get().borrow_mut().buffer.move_in_history(true) {
        ui.trigger_buffer_change_callbacks(()).await;
    }
    Ok(())
}

async fn accept_line(mut ui: Ui, _lua: Lua, (): ()) -> Result<bool> {
    ui.accept_line().await
}

async fn redraw(ui: Ui, lua: Lua, val: Option<LuaValue>) -> Result<()> {
    if let Some(val) = val {
        let val: RedrawOptions = lua.from_value(val)?;
        {
            let ui = ui.get();
            let mut ui = ui.borrow_mut();
            if val.all { ui.dirty = true; }
            if val.prompt { ui.cmdline.prompt_dirty = true; }
            if val.predisplay { ui.cmdline.predisplay_dirty = true; }
            if val.postdisplay { ui.cmdline.postdisplay_dirty = true; }
            if val.buffer { ui.buffer.dirty = true; }
            if val.messages { ui.tui.dirty = true; }
            if val.status_bar { ui.status_bar.dirty = true; }
        }

        if val.now {
            return ui.draw().await
        }
    }

    ui.queue_draw();
    Ok(())
}

fn exit(ui: &Ui, _lua: &Lua, code: Option<i32>) -> Result<()> {
    ui.events.read().exit(code.unwrap_or(0));
    Ok(())
}

async fn get_cwd(ui: Ui, _lua: Lua, (): ()) -> Result<BString> {
    Ok(ui.shell.get_cwd().await)
}

async fn call_hook_func(ui: Ui, _lua: Lua, mut args: Vec<BString>) -> Result<Option<i32>> {
    let arg0 = args.remove(0);
    ui.freeze_if(true, true, async {
        ui.shell.call_hook_func(Cow::Owned(arg0.into()), args.into_iter().map(|x| x.into()).collect()).await
    }).await
}

async fn print(ui: Ui, _lua: Lua, value: BString) -> Result<()> {
    ui.freeze_if(true, false, async {
        ui.get().borrow_mut().stdout.write_all(value.as_ref())
    }).await??;
    crate::log_if_err(ui.draw().await);
    Ok(())
}

fn time(_lua: &Lua, (): ()) -> LuaResult<f64> {
    Ok(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs_f64())
}

async fn sleep(_lua: Lua, seconds: f64) -> LuaResult<()> {
    tokio::time::sleep(std::time::Duration::from_secs_f64(seconds)).await;
    Ok(())
}

async fn lua_try(lua: Lua, (func, catch, finally): (LuaFunction, Option<LuaFunction>, Option<LuaFunction>)) -> LuaResult<LuaMultiValue> {
    let mut result = func.call_async(()).await;

    if let Some(catch) = catch {
        result = match result {
            Ok(x) => Ok(x),
            Err(e) => {
                let err = e.clone().into_lua(&lua).unwrap();
                let catch_result: LuaResult<LuaValue> = catch.call_async(err).await;
                match catch_result {
                    Ok(_) => Ok(LuaMultiValue::new()),
                    Err(new_err) if new_err.to_string() == e.to_string() => Err(e),
                    Err(new_err) => Err(new_err.context(e)),
                }
            },
        };
    }


    if let Some(finally) = finally {
        let finally_result: LuaResult<()> = finally.call_async(()).await;
        result = match (result, finally_result) {
            (x, Ok(_)) => x,
            (Ok(_), Err(e)) => Err(e),
            (Err(e1), Err(e2)) => Err(e2.context(e1)),
        };
    }
    result

}

pub async fn __laggy(_ui: Ui, lua: Lua, (): ()) -> Result<()> {
    let _ = tokio::task::spawn_blocking(move || {
        let _: Result<(), mlua::Error> = unsafe{ lua.exec_raw((), |_| {
            // let lock = ui.borrow_mut();
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    for i in 0..2 {
                        eprintln!("DEBUG(likes) \t{}\t= {:?}", stringify!(i), i);
                        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                    }
                });
            });
        }) };
    }).await;
    // drop(lock);
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_fn("get_cursor", get_cursor)?;
    ui.set_lua_fn("get_buffer", get_buffer)?;
    ui.set_lua_fn("set_cursor", set_cursor)?;
    ui.set_lua_async_fn("set_buffer", set_buffer)?;
    ui.set_lua_async_fn("undo_buffer", undo_buffer)?;
    ui.set_lua_async_fn("redo_buffer", redo_buffer)?;
    ui.set_lua_async_fn("accept_line", accept_line)?;
    ui.set_lua_async_fn("redraw",  redraw)?;
    ui.set_lua_fn("exit", exit)?;
    ui.set_lua_async_fn("get_cwd", get_cwd)?;
    ui.set_lua_async_fn("call_hook_func", call_hook_func)?;
    ui.set_lua_async_fn("print", print)?;
    ui.get_lua_api()?.set("sleep", ui.lua.create_async_function(sleep)?)?;
    ui.get_lua_api()?.set("time", ui.lua.create_function(time)?)?;
    ui.get_lua_api()?.set("try", ui.lua.create_async_function(lua_try)?)?;
    ui.set_lua_async_fn("__laggy", __laggy)?;

    keybind::init_lua(ui)?;
    string::init_lua(ui)?;
    completion::init_lua(ui)?;
    history::init_lua(ui)?;
    events::init_lua(ui)?;
    tui::init_lua(ui)?;
    log::init_lua(ui)?;
    asyncio::init_lua(ui)?;
    process::init_lua(ui)?;
    parser::init_lua(ui)?;
    variables::init_lua(ui)?;
    functions::init_lua(ui)?;
    regex::init_lua(ui)?;

    Ok(())
}
