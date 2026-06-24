use mlua::prelude::*;

pub struct MaxNumber;

pub enum PossiblyMaxNumber<T> {
    Raw(T),
    Max,
}

impl<T> PossiblyMaxNumber<T> {
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Raw(x) => Some(x),
            Self::Max => None,
        }
    }
}

pub type PossiblyMaxUsize = PossiblyMaxNumber<usize>;
impl From<PossiblyMaxUsize> for usize {
    fn from(value: PossiblyMaxUsize) -> Self {
        value.into_option().unwrap_or(Self::MAX)
    }
}

impl<T: FromLua> FromLua for PossiblyMaxNumber<T> {
    fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::UserData(ref value) = value && value.is::<MaxNumber>() {
            Ok(Self::Max)
        } else {
            T::from_lua(value, lua).map(Self::Raw)
        }
    }
}
