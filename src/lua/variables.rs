use std::collections::HashMap;
use bstr::BString;
use crate::ui::Ui;
use anyhow::Result;
use mlua::prelude::*;
use crate::shell::variables;

async fn get_var(ui: Ui, lua: Lua, name: BString) -> Result<LuaValue> {
    let val = match ui.shell.lock().await.get_var(name.as_ref())? {
        Some(variables::Value::String(val)) => val.into_lua(&lua)?,
        Some(variables::Value::Array(val)) => val.into_lua(&lua)?,
        Some(variables::Value::HashMap(val)) => val.into_lua(&lua)?,
        Some(variables::Value::Integer(val)) => val.into_lua(&lua)?,
        Some(variables::Value::Float(val)) => val.into_lua(&lua)?,
        None => LuaValue::Nil,
    };
    Ok(val)
}

async fn set_var(ui: Ui, lua: Lua, (name, val, global): (BString, LuaValue, Option<bool>)) -> Result<()> {
    let val: variables::Value = match val {
        LuaValue::Integer(val) => val.into(),
        LuaValue::Number(val) => val.into(),
        LuaValue::String(val) => BString::new(val.as_bytes().to_owned()).into(),
        LuaValue::Table(val) => {
            let mut size = 0;
            val.for_each(|_: LuaValue, _: LuaValue| { size += 1; Ok(()) })?;

            if val.raw_len() == size {
                let val = Vec::<BString>::from_lua(LuaValue::Table(val), &lua)?;
                val.into()
            } else {
                let val = HashMap::<BString, BString>::from_lua(LuaValue::Table(val), &lua)?;
                val.into()
            }
        },
        val => {
            return Err(anyhow::anyhow!("invalid value: {:?}", val))
        },
    };
    ui.shell.lock().await.set_var(name.as_ref(), val, !global.unwrap_or(false))?;
    Ok(())
}

async fn unset_var(ui: Ui, _lua: Lua, name: BString) -> Result<()> {
    ui.shell.lock().await.unset_var(name.as_ref());
    Ok(())
}

async fn export_var(ui: Ui, _lua: Lua, name: BString) -> Result<()> {
    ui.shell.lock().await.export_var(name.as_ref());
    Ok(())
}

async fn in_param_scope(ui: Ui, _lua: Lua, name: LuaFunction) -> Result<LuaValue> {
    ui.shell.lock().await.startparamscope();
    let result = name.call_async(()).await;
    ui.shell.lock().await.endparamscope();
    Ok(result?)
}

async fn in_zle_param_scope(ui: Ui, _lua: Lua, name: LuaFunction) -> Result<LuaValue> {
    ui.shell.lock().await.start_zle_scope();
    let result = name.call_async(()).await;
    ui.shell.lock().await.end_zle_scope();
    Ok(result?)
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_var", get_var)?;
    ui.set_lua_async_fn("set_var", set_var)?;
    ui.set_lua_async_fn("unset_var", unset_var)?;
    ui.set_lua_async_fn("export_var", export_var)?;
    ui.set_lua_async_fn("in_param_scope", in_param_scope)?;
    ui.set_lua_async_fn("in_zle_param_scope", in_zle_param_scope)?;

    Ok(())
}
