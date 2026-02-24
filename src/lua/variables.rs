use std::collections::HashMap;
use bstr::BString;
use crate::ui::Ui;
use anyhow::Result;
use mlua::prelude::*;
use crate::shell::variables;

async fn get_var(ui: Ui, lua: Lua, (name, zle): (BString, Option<bool>)) -> Result<LuaValue> {
    let val = match ui.shell.get_var(name.into(), zle.unwrap_or(false)).await? {
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
    ui.shell.set_var(name.into(), val, !global.unwrap_or(false)).await?;
    Ok(())
}

async fn unset_var(ui: Ui, _lua: Lua, name: BString) -> Result<()> {
    ui.shell.unset_var(name.into()).await;
    Ok(())
}

async fn export_var(ui: Ui, _lua: Lua, name: BString) -> Result<()> {
    ui.shell.export_var(name.into()).await;
    Ok(())
}

async fn in_param_scope(ui: Ui, _lua: Lua, func: LuaFunction) -> Result<LuaValue> {
    ui.shell.startparamscope().await;
    let result = func.call_async(()).await;
    ui.shell.endparamscope().await;
    Ok(result?)
}

async fn in_zle_param_scope(ui: Ui, _lua: Lua, func: LuaFunction) -> Result<LuaValue> {
    ui.shell.start_zle_scope().await;
    let result = func.call_async(()).await;
    ui.shell.end_zle_scope().await;
    Ok(result?)
}

#[derive(Debug, strum::EnumString)]
#[allow(non_camel_case_types)]
enum VarType {
    string,
    integer,
    float,
    array,
    hashmap,
}

async fn create_dynamic_var(
    ui: Ui,
    lua: Lua,
    (name, typ, get, set, unset): (BString, LuaValue, LuaFunction, Option<LuaFunction>, Option<LuaFunction>),
) -> Result<()> {

    let weak = lua.weak();

    macro_rules! make_dynamic_var_func {
        (|$($arg:ident),*| $result:expr) => (
            {
                let weak = weak.clone();
                Box::new(move |$($arg),*| {
                    if weak.try_upgrade().is_none() {
                        eprintln!("Lua instance is destroyed")
                    } else {
                        match crate::shell::run_with_shell($result) {
                            Ok(Ok(val)) => return val,
                            Ok(Err(err)) => ::log::error!("{}", err),
                            Err(err) => ::log::error!("{}", err),
                        }
                    }
                    Default::default()
                })
            }
        )
    }

    macro_rules! make_dynamic_var {
        ($func:ident) => (
            ui.shell.clone().$func(
                name.into(),
                make_dynamic_var_func!(| | get.call_async(())),
                if let Some(set) = set {
                    Some(make_dynamic_var_func!(|x| set.call_async(x)))
                } else {
                    None
                },
                if let Some(unset) = unset {
                    Some(make_dynamic_var_func!(|x| unset.call_async(x)))
                } else {
                    None
                },
            )
        )
    }

    let typ: super::SerdeWrap<VarType> = lua.from_value(typ)?;
    match typ.0 {
        VarType::string => make_dynamic_var!(create_dynamic_string_var).await,
        VarType::integer => make_dynamic_var!(create_dynamic_integer_var).await,
        VarType::float => make_dynamic_var!(create_dynamic_float_var).await,
        VarType::array => make_dynamic_var!(create_dynamic_array_var).await,
        VarType::hashmap => make_dynamic_var!(create_dynamic_hash_var).await,
    }
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_var", get_var)?;
    ui.set_lua_async_fn("set_var", set_var)?;
    ui.set_lua_async_fn("unset_var", unset_var)?;
    ui.set_lua_async_fn("export_var", export_var)?;
    ui.set_lua_async_fn("in_param_scope", in_param_scope)?;
    ui.set_lua_async_fn("in_zle_param_scope", in_zle_param_scope)?;
    ui.set_lua_async_fn("create_dynamic_var", create_dynamic_var)?;

    Ok(())
}
