use mlua::prelude::*;

pub struct MaxNumber;

#[derive(Clone, Copy)]
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

impl<T: std::fmt::Debug> std::fmt::Debug for PossiblyMaxNumber<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Self::Raw(val) => val.fmt(f),
            Self::Max => "max".fmt(f),
        }
    }
}

macro_rules! impl_for {
    ($type:ty) => {

        impl Default for PossiblyMaxNumber<$type> {
            fn default() -> Self {
                Self::Raw(0)
            }
        }

        impl FromLua for PossiblyMaxNumber<$type> {
            fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self> {
                if let LuaValue::UserData(ref value) = value && value.is::<MaxNumber>() {
                    Ok(Self::Max)
                } else {
                    <$type>::from_lua(value, lua).map(Self::Raw)
                }
            }
        }

        impl From<PossiblyMaxNumber<$type>> for $type {
            fn from(value: PossiblyMaxNumber<$type>) -> Self {
                value.into_option().unwrap_or(Self::MAX)
            }
        }

    };
}

pub type PossiblyMaxUsize = PossiblyMaxNumber<usize>;
impl_for!(usize);
