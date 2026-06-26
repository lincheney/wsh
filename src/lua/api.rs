use crate::lua::LuaWrapper;
use crate::shell::{MetaString};
use std::str::FromStr;
use serde::{Deserialize, Deserializer, de};
use bstr::BString;
use std::io::Write;
use mlua::prelude::*;
use anyhow::Result;
use crate::lua::{Ui};
use std::time::SystemTime;

mod number;
pub mod keybind;
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
use crate::keybind::EventIndex;
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

impl<T: ToString> serde::Serialize for SerdeWrap<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
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
    Ok(ui.try_borrow()?.buffer.get_cursor() + 1)
}

fn get_buffer(ui: &Ui, lua: &Lua, (): ()) -> Result<(mlua::String, usize)> {
    let buffer = &ui.try_borrow()?.buffer;
    Ok((lua.create_string(buffer.get_contents())?, buffer.get_cursor() + 1))
}

fn set_cursor(ui: &Ui, _lua: &Lua, val: number::PossiblyMaxUsize) -> Result<()> {
    let val: usize = val.into();
    ui.try_borrow_mut()?.buffer.set_cursor(val.saturating_sub(1));
    ui.queue_draw();
    Ok(())
}

async fn set_buffer(ui: Ui, _lua: Lua, (val, cursor): (mlua::String, Option<number::PossiblyMaxUsize>)) -> Result<()> {
    let cursor = cursor.map(|c| usize::from(c).saturating_sub(1));
    ui.insert_or_set_buffer(false, &val.as_bytes(), cursor).await?;
    ui.trigger_buffer_change_callbacks().await?;
    ui.queue_draw();
    Ok(())
}

async fn insert_at_cursor(ui: Ui, _lua: Lua, val: mlua::String) -> Result<()> {
    ui.insert_or_set_buffer(true, &val.as_bytes(), None).await?;
    ui.trigger_buffer_change_callbacks().await?;
    ui.queue_draw();
    Ok(())
}

async fn delete_at_cursor(ui: Ui, _lua: Lua, count: isize) -> Result<()> {
    ui.try_borrow_mut()?.buffer.delete_at_cursor(count.unsigned_abs(), count >= 0);
    ui.trigger_buffer_change_callbacks().await?;
    ui.queue_draw();
    Ok(())
}

async fn undo_buffer(ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    if ui.try_borrow_mut()?.buffer.move_in_history(false) {
        ui.trigger_buffer_change_callbacks().await?;
        ui.queue_draw();
    }
    Ok(())
}

async fn redo_buffer(ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    if ui.try_borrow_mut()?.buffer.move_in_history(true) {
        ui.trigger_buffer_change_callbacks().await?;
        ui.queue_draw();
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
            let mut ui = ui.try_borrow_mut()?;
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

async fn exit(mut ui: Ui, _lua: Lua, code: Option<i32>) -> Result<()> {
    ui.shell.exit(code.unwrap_or(0));
    ui.accept_line().await?;
    Ok(())
}

fn get_cwd(ui: &Ui, _lua: &Lua, (): ()) -> Result<BString> {
    Ok(ui.shell.get_cwd())
}

fn get_size(ui: &Ui, _lua: &Lua, (): ()) -> Result<(u32, u32)> {
    Ok(ui.try_borrow()?.size)
}

async fn call_hook_func(ui: Ui, _lua: Lua, mut args: Vec<BString>) -> Result<Option<i32>> {
    let arg0 = args.remove(0);
    let args: Vec<MetaString> = args.into_iter().map(|x| x.into()).collect();
    ui.freeze_if(true, true, async {
        ui.shell.call_hook_func(MetaString::from(arg0).as_ref(), args.iter())
    }).await
}

async fn print(ui: Ui, _lua: Lua, value: BString) -> Result<()> {
    ui.freeze_if::<Result<()>, _>(true, false, async {
        Ok(ui.try_borrow_mut()?.stdout.write_all(value.as_ref())?)
    }).await??;
    Ok(())
}

async fn set_interrupt_key(ui: Ui, _lua: Lua, label: String) -> Result<()> {
    if let EventIndex::Key(key) = EventIndex::parse_from_label(&label)?
        && let Some(byte) = key.try_into_byte()
    {
        ui.set_vintr(byte).await?;
        return Ok(())
    }
    anyhow::bail!("{label:?} cannot be represented using one byte")
}

fn time(_lua: &Lua, (): ()) -> LuaResult<f64> {
    Ok(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs_f64())
}

async fn sleep(_lua: Lua, seconds: f64) -> LuaResult<()> {
    crate::interrupter::run(tokio::time::sleep(std::time::Duration::from_secs_f64(seconds))).await
        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
    Ok(())
}

fn shell_quote(_lua: &Lua, val: BString) -> LuaResult<BString> {
    let meta_val: crate::shell::MetaString = val.into();
    let quoted = crate::shell::shell_quote(meta_val);
    Ok(quoted.unmetafy())
}

async fn lua_try(lua: Lua, args: LuaTable) -> LuaResult<LuaMultiValue> {
    let func: LuaFunction = args.get("try")?;
    let catch: Option<LuaFunction> = args.get("catch")?;
    let finally: Option<LuaFunction> = if catch.is_none() {
        Some(args.get("finally")?)
    } else {
        args.get("finally")?
    };

    let mut result = crate::lua::call_lua_fn(&func, ()).await;
    let mut error = None;

    if let Some(catch) = catch {
        result = match result {
            Ok(x) => Ok(x),
            Err(e) => {
                let err = e.clone().into_lua(&lua).unwrap();
                let catch_result: LuaResult<LuaValue> = crate::lua::call_lua_fn(&catch, err.clone()).await;
                error = Some(err);
                match catch_result {
                    Ok(_) => Ok(LuaMultiValue::new()),
                    Err(new_err) if new_err.to_string() == e.to_string() => Err(e),
                    Err(new_err) => Err(new_err.context(e)),
                }
            },
        };
    }


    if let Some(finally) = finally {
        let finally_result: LuaResult<()> = crate::lua::call_lua_fn(&finally, error).await;
        result = match (result, finally_result) {
            (x, Ok(_)) => x,
            (Ok(_), Err(e)) => Err(e),
            (Err(e1), Err(e2)) => Err(e2.context(e1)),
        };
    }
    result
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_fn("get_cursor", get_cursor)?;
    lua.set_fn("get_buffer", get_buffer)?;
    lua.set_fn("set_cursor", set_cursor)?;
    lua.set_async_fn("set_buffer", set_buffer)?;
    lua.set_async_fn("insert_at_cursor", insert_at_cursor)?;
    lua.set_async_fn("delete_at_cursor", delete_at_cursor)?;
    lua.set_async_fn("undo_buffer", undo_buffer)?;
    lua.set_async_fn("redo_buffer", redo_buffer)?;
    lua.set_async_fn("accept_line", accept_line)?;
    lua.set_async_fn("redraw",  redraw)?;
    lua.set_async_fn("exit", exit)?;
    lua.set_fn("get_cwd", get_cwd)?;
    lua.set_fn("get_size", get_size)?;
    lua.set_async_fn("call_hook_func", call_hook_func)?;
    lua.set_async_fn("print", print)?;
    lua.set_async_fn("set_interrupt_key", set_interrupt_key)?;
    lua.api.set("sleep", lua.create_async_function(sleep)?)?;
    lua.api.set("time", lua.create_function(time)?)?;
    lua.api.set("shell_quote", lua.create_function(shell_quote)?)?;
    lua.api.set("try", lua.create_async_function(lua_try)?)?;
    lua.api.set("MAXNUM", lua.create_any_userdata(number::MaxNumber)?)?;

    keybind::init_lua(lua)?;
    string::init_lua(lua)?;
    completion::init_lua(lua)?;
    history::init_lua(lua)?;
    events::init_lua(lua)?;
    tui::init_lua(lua)?;
    log::init_lua(lua)?;
    asyncio::init_lua(lua)?;
    process::init_lua(lua)?;
    parser::init_lua(lua)?;
    variables::init_lua(lua)?;
    functions::init_lua(lua)?;
    regex::init_lua(lua)?;

    Ok(())
}
