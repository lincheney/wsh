use crate::lua::LuaWrapper;
use crate::shell::{MetaString};
use std::collections::HashMap;
use bstr::BString;
use crate::ui::Ui;
use anyhow::Result;
use mlua::prelude::*;
use crate::shell::variables;

fn value_to_lua(lua: &Lua, val: Option<variables::Value>) -> Result<LuaValue> {
    let val = match val {
        Some(variables::Value::String(val)) => val.into_lua(lua)?,
        Some(variables::Value::Array(val)) => val.into_lua(lua)?,
        Some(variables::Value::HashMap(val)) => val.into_lua(lua)?,
        Some(variables::Value::Integer(val)) => val.into_lua(lua)?,
        Some(variables::Value::Float(val)) => val.into_lua(lua)?,
        None => LuaValue::Nil,
    };
    Ok(val)
}

fn get_var(ui: &Ui, lua: &Lua, (name, zle): (BString, Option<bool>)) -> Result<LuaValue> {
    let name: MetaString = name.into();
    value_to_lua(lua, ui.shell.get_var(name.as_ref(), zle.unwrap_or(false))?)
}

fn get_vars(ui: &Ui, lua: &Lua, (names, zle): (Vec<BString>, Option<bool>)) -> Result<LuaTable> {
    let varnames: Vec<MetaString> = names.iter().map(|n| n.clone().into()).collect();
    let results = ui.shell.get_vars(varnames.iter(), zle.unwrap_or(false))?;

    let table = lua.create_table()?;
    for (name, val) in names.into_iter().zip(results) {
        if val.is_some() {
            table.set(name.to_string(), value_to_lua(lua, val)?)?;
        }
    }
    Ok(table)
}

fn set_var(ui: &Ui, lua: &Lua, (name, val, global): (BString, LuaValue, Option<bool>)) -> Result<()> {
    let val: variables::Value = match val {
        LuaValue::Integer(val) => val.into(),
        LuaValue::Number(val) => val.into(),
        LuaValue::String(val) => BString::new(val.as_bytes().to_owned()).into(),
        LuaValue::Table(val) => {
            let mut size = 0;
            val.for_each(|_: LuaValue, _: LuaValue| { size += 1; Ok(()) })?;

            if val.raw_len() == size {
                let val = Vec::<BString>::from_lua(LuaValue::Table(val), lua)?;
                val.into()
            } else {
                let val = HashMap::<BString, BString>::from_lua(LuaValue::Table(val), lua)?;
                val.into()
            }
        },
        val => {
            return Err(anyhow::anyhow!("invalid value: {:?}", val))
        },
    };
    let name: MetaString = name.into();
    ui.shell.set_var(name.as_ref(), val, !global.unwrap_or(false))?;
    Ok(())
}

fn unset_var(ui: &Ui, _lua: &Lua, name: BString) -> Result<()> {
    let name: MetaString = name.into();
    ui.shell.unset_var(name.as_ref());
    Ok(())
}

fn export_var(ui: &Ui, _lua: &Lua, name: BString) -> Result<()> {
    let name: MetaString = name.into();
    ui.shell.export_var(name.as_ref());
    Ok(())
}

async fn in_param_scope(ui: Ui, _lua: Lua, func: LuaFunction) -> Result<LuaValue> {
    // TODO
    ui.shell.startparamscope();
    let result = func.call_async(()).await;
    ui.shell.endparamscope();
    Ok(result?)
}

async fn in_zle_param_scope(ui: Ui, _lua: Lua, func: LuaFunction) -> Result<LuaValue> {
    ui.shell.start_zle_scope();
    let result = func.call_async(()).await;
    ui.shell.end_zle_scope();
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

    let weak = ui.downgrade();

    macro_rules! make_dynamic_var_func {
        (|$($arg:ident),*| $result:expr) => (
            {
                let weak = weak.clone();
                Box::new(move |$($arg),*| {
                    if let Ok(ui) = Ui::try_upgrade(&weak) {
                        match ui.shell_loop($result) {
                            Ok(Ok(val)) => return val,
                            Ok(Err(err)) => ::log::error!("{}", err),
                            Err(err) => ::log::error!("{}", err),
                        }
                    } else {
                        eprintln!("Lua instance is destroyed")
                    }
                    Default::default()
                })
            }
        )
    }

    macro_rules! make_dynamic_var {
        ($func:ident) => (
            ui.shell.$func(
                crate::shell::MetaString::from(name).as_ref(),
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
        VarType::string => make_dynamic_var!(create_dynamic_string_var),
        VarType::integer => make_dynamic_var!(create_dynamic_integer_var),
        VarType::float => make_dynamic_var!(create_dynamic_float_var),
        VarType::array => make_dynamic_var!(create_dynamic_array_var),
        VarType::hashmap => make_dynamic_var!(create_dynamic_hash_var),
    }
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_fn("get_var", get_var)?;
    lua.set_fn("get_vars", get_vars)?;
    lua.set_fn("set_var", set_var)?;
    lua.set_fn("unset_var", unset_var)?;
    lua.set_fn("export_var", export_var)?;
    lua.set_async_fn("in_param_scope", in_param_scope)?;
    lua.set_async_fn("in_zle_param_scope", in_zle_param_scope)?;
    lua.set_async_fn("create_dynamic_var", create_dynamic_var)?;

    Ok(())
}
