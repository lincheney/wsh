use mlua::{prelude::*};

thread_local! {
    pub static FIELD_SEP_REGEX: regex::Regex = regex::Regex::new(r"\s+:\s+").unwrap();
}

pub fn strip_runtime_error(mut err: String) -> String {
    const RUNTIME_ERROR: &str = "runtime error: ";
    if err.starts_with(RUNTIME_ERROR) {
        err.replace_range(..RUNTIME_ERROR.len(), "");
    }
    err
}

pub fn make_error(err: String, path: &str) -> LuaError {
    let mut err = strip_runtime_error(err);
    if !err.starts_with(".") {
        err.insert_str(0, ": ");
    }
    err.insert_str(0, path);
    crate::lua::lua_error(err)
}

pub trait FromLuaRef: Sized {
    fn from_lua_ref(value: &LuaValue, lua: &Lua) -> LuaResult<Self>;
}

impl<T: FromLua> FromLuaRef for T {
    fn from_lua_ref(value: &LuaValue, lua: &Lua) -> LuaResult<Self> {
        Self::from_lua(value.clone(), lua)
    }
}

macro_rules! auto_from_lua {

    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident $(<$($generics:tt),*>)? { $(
            $(#[flatten $($dummy:ident)? ])?
            $field_vis:vis $field:ident: $type:ty,
        )* }
    ) => (

        $(#[$meta])*
        $vis struct $name $(<$($generics),*>)? { $(
            $field_vis $field: $type,
        )* }

        impl $(<$($generics),*>)? $name $(<$($generics),*>)? {
            fn from_lua_ref(value: &::mlua::Value, lua: &::mlua::Lua) -> ::mlua::Result<Self> {
                #[allow(unused_imports)]
                use crate::lua::auto_from_lua::{FromLuaRef};
                if let ::mlua::Value::Table(table) = &value {

                    if table.raw_len() != 0 {
                        return Err(crate::lua::lua_error("expected a non-array-like table"));
                    }

                    $(
                    let flatten = $($($dummy:ident)? true || )? false;
                    let $field: $type = if flatten {
                        <$type>::from_lua_ref(&value, lua)?
                    } else {
                        table.raw_get(stringify!($field))
                            .map_err(|err| crate::lua::auto_from_lua::make_error(err.to_string(), concat!(".", stringify!($field))) )?
                    };
                    )*

                    Ok(Self { $(
                        $field,
                    )* })
                } else {
                    Err(crate::lua::lua_error(format!("expected a table, got a {}", value.type_name())))
                }
            }
        }

        impl $(<$($generics),*>)? ::mlua::FromLua for $name $(<$($generics),*>)? {
            fn from_lua(value: ::mlua::Value, lua: &::mlua::Lua) -> ::mlua::Result<Self> {
                Self::from_lua_ref(&value, lua)
            }
        }


    );

    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident $(<$($generics:tt),*>)? { $(
            $(#[$variant_meta:meta])?
            $variant:ident
                $(($tuple_ty:ty))?
                $({ $(
                    $(#[flatten $($dummy:ident)? ])?
                    $field_vis:vis $field:ident: $struct_ty:ty,
                )* })?
            ,
        )+ }
    ) => (

        $(#[$meta])*
        $vis enum $name $(<$($generics),*>)? { $(
            $(#[$variant_meta])?
            $variant
                $(($tuple_ty))?
                $({ $(
                    $field_vis $field: $struct_ty
                ),* })?
        ),+ }

        impl $(<$($generics),*>)? $name $(<$($generics),*>)? {
            #[allow(unused_variables)]
            fn from_lua_ref(value: &::mlua::Value, lua: &::mlua::Lua) -> ::mlua::Result<Self> {
                #[allow(unused_imports)]
                use crate::lua::auto_from_lua::FromLuaRef;
                $(
                    #[allow(non_snake_case)]
                    let $variant = (|| {
                        return crate::lua::auto_from_lua::auto_from_lua_enum_variant!(
                            value, lua,
                            $variant
                                $(($tuple_ty))?
                                $({ $(
                                    $(#[flatten $($dummy)? ])?
                                    $field: $struct_ty),*
                                })?
                        );
                    })();
                    #[allow(non_snake_case)]
                    let $variant = match $variant {
                        Err($variant) => $variant,
                        value => return value,
                    };
                )+

                let msg = format!(
                    concat!(
                        "did not match any of:",
                        $(
                        "\n\t{} : {", stringify!($variant), "}",
                        )+
                    ),
                    $( {
                    let label = stringify!($variant$(($tuple_ty))?$({$($field: $struct_ty),*})? ).replace('\n', "");
                    crate::lua::auto_from_lua::FIELD_SEP_REGEX.with(|re| re.replace_all(&label, ": ").into_owned())
                    }, )+
                    $(
                    $variant = crate::lua::auto_from_lua::strip_runtime_error($variant.to_string().replace("\n\t", "\n\t\t")),
                    )+
                );
                return Err(crate::lua::lua_error(msg));
            }
        }

        impl $(<$($generics),*>)? ::mlua::FromLua for $name $(<$($generics),*>)? {
            fn from_lua(value: ::mlua::Value, lua: &::mlua::Lua) -> ::mlua::Result<Self> {
                Self::from_lua_ref(&value, lua)
            }
        }

    );

}

macro_rules! auto_from_lua_enum_variant {
    ($value:expr, $lua:expr, $variant:ident) => (
        if $value.as_string().is_some_and(|x| x.as_bytes().eq_ignore_ascii_case(stringify!($variant).as_bytes())) {
            Ok(Self::$variant)
        } else {
            Err(crate::lua::lua_error(concat!("does not match: ", stringify!($variant))))
        }
    );
    ($value:expr, $lua:expr, $variant:ident ($tuple_ty:ty)) => (
        Ok(Self::$variant(<$tuple_ty>::from_lua_ref($value, $lua)?))
    );
    (
        $value:expr,
        $lua:expr,
        $variant:ident { $(
            $(#[flatten $($dummy:ident)? ])?
            $field:ident: $struct_ty:ty
        ),* }
    ) => (

        {
            crate::lua::auto_from_lua::auto_from_lua! {
                struct Temp { $(
                    $(#[flatten $($dummy)? ])?
                    $field: $struct_ty,
                )* }
            }
            let value = Temp::from_lua_ref($value, $lua)?;
            Ok(Self::$variant{ $(
                $field: value.$field,
            )* })
        }
    );
}

pub(crate) use auto_from_lua;
pub(crate) use auto_from_lua_enum_variant;
