use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods, MetaMethod};
use crate::ui::Ui;

struct Regex {
    inner: regex::bytes::Regex,
    full: Option<regex::bytes::Regex>,
}

impl UserData for Regex {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_lua, regex, ()| {
            Ok(format!("{:?}", regex.inner.as_str()))
        });
        methods.add_method("is_match", |_lua, regex, (arg, start): (LuaString, Option<usize>)| {
            Ok(regex.inner.is_match_at(&arg.as_bytes(), start.unwrap_or(0)))
        });
        methods.add_method_mut("is_full_match", |_lua, regex, arg: LuaString| {
            let regex = regex.full.get_or_insert_with(|| {
                let pat = regex.inner.as_str();
                let pat = format!("^(?:{pat})$");
                regex::bytes::Regex::new(&pat).unwrap()
            });

            let bytes = arg.as_bytes();
            Ok(regex.is_match(&bytes))
        });
        methods.add_method("find", |_lua, regex, (arg, start): (LuaString, Option<usize>)| {
            if let Some(m) = regex.inner.find_at(&arg.as_bytes(), start.unwrap_or(0)) {
                Ok((Some(m.start()+1), Some(m.end())))
            } else {
                Ok((None, None))
            }
        });
        methods.add_method("find_all", |lua, regex, arg: LuaString| {
            lua.create_sequence_from(
                regex.inner.find_iter(&arg.as_bytes())
                .map(|m| [m.start()+1, m.end()])
            )
        });
        methods.add_method("captures", |lua, regex, (arg, start): (LuaString, Option<usize>)| {
            if let Some(captures) = regex.inner.captures_at(&arg.as_bytes(), start.unwrap_or(0)) {
                Ok(Some(lua.create_sequence_from(captures.iter().map(|m| m.map(|m| [m.start()+1, m.end()])))?))
            } else {
                Ok(None)
            }
        });
        methods.add_method("captures_all", |lua, regex, arg: LuaString| {
            let mut captures = vec![];
            for c in regex.inner.captures_iter(&arg.as_bytes()) {
                captures.push(lua.create_sequence_from(c.iter().map(|m| m.map(|m| [m.start()+1, m.end()])))?);
            }
            Ok(captures)
        });
        methods.add_method("replace", |_lua, regex, (arg, replace, limit): (LuaString, LuaString, Option<usize>)| {
            Ok(regex.inner.replacen(&arg.as_bytes(), limit.unwrap_or(1), &*replace.as_bytes()).into_owned())
        });
        methods.add_method("replace_all", |_lua, regex, (arg, replace): (LuaString, LuaString)| {
            Ok(regex.inner.replace_all(&arg.as_bytes(), &*replace.as_bytes()).into_owned())
        });
    }
}

fn regex(_lua: &Lua, string: String) -> LuaResult<Regex> {
    let regex = regex::bytes::Regex::new(&string);
    let regex = regex.map_err(|e| mlua::Error::RuntimeError(format!("{e}")))?;
    Ok(Regex{ inner: regex, full: None })
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.get_lua_api()?.set("regex", ui.lua.create_function(regex)?)?;

    Ok(())
}


