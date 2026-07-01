use mlua::prelude::*;

// this is really just Vec<T> but exists so that i can have error messages
// that tell you which element failed
#[derive(Debug, Clone, Default)]
pub struct Array<T>(pub Vec<T>);

impl<T: FromLua> Array<T> {
    pub fn from_lua_ref(value: &LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let Some(table) = value.as_table() {
            let mut vec = Vec::with_capacity(table.raw_len());
            for (i, val) in table.sequence_values().enumerate() {
                let val = val.map_err(|err| super::auto_from_lua::make_error(err.to_string(), &format!(".[{}]", i+1)))?;
                vec.push(val);
            }
            Ok(Self(vec))
        } else {
            Err(crate::lua::lua_error(format!("expected a table, got a {}", value.type_name())))
        }
    }
}

impl<T: FromLua> FromLua for Array<T> {
    fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self> {
        Self::from_lua_ref(&value, lua)
    }
}
