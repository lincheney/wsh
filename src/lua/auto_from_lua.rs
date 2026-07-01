use mlua::{prelude::*};

pub fn make_error(mut err: String, path: &str) -> LuaError {
    if err.starts_with("runtime error: ") {
        err.replace_range(.."runtime error: ".len(), "");
    }
    if err.starts_with(".") {
        err.insert_str(0, ": ");
    }
    err.insert_str(0, path);
    crate::lua::lua_error(err)
}

macro_rules! auto_from_lua {

    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident { $(
            $(#[flatten $($dummy:ident)? ])?
            $field_vis:vis $field:ident: $type:ty,
        )* }
    ) => (

        $(#[$meta])*
        $vis struct $name { $(
            $field_vis $field: $type,
        )* }

        impl ::mlua::FromLua for $name {
            fn from_lua(value: ::mlua::Value, lua: &::mlua::Lua) -> ::mlua::Result<Self> {
                if let ::mlua::Value::Table(table) = value.clone() && table.raw_len() == 0 {
                    $(
                    let flatten = $($($dummy:ident)? true || )? false;
                    let $field: $type = if flatten {
                        <$type>::from_lua(value.clone(), lua)?
                    } else {
                        table.raw_get(stringify!($field))
                            .map_err(|err| crate::lua::auto_from_lua::make_error(err.to_string(), concat!(".", stringify!($field))) )?
                    };
                    )*

                    Ok(Self { $(
                        $field,
                    )* })
                } else {
                    Err(crate::lua::lua_error("expected a table"))
                }
            }
        }

    );

    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident { $(
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
        $vis enum $name { $(
            $(#[$variant_meta])?
            $variant
                $(($tuple_ty))?
                $({ $(
                    $field_vis $field: $struct_ty
                ),* })?
        ),+ }

        impl ::mlua::FromLua for $name {
            #[allow(unused_variables)]
            fn from_lua(value: ::mlua::Value, lua: &::mlua::Lua) -> ::mlua::Result<Self> {
                $(
                    #[allow(non_snake_case)]
                    let $variant = (|| {
                        return crate::lua::auto_from_lua::auto_from_lua_enum_variant!(
                            value.clone(), lua,
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
                        "did not match any of:\n",
                        $(
                        "\t", stringify!($variant$(($tuple_ty))?$({{$($field: $struct_ty),*}})? ), ": {}", "\n",
                        )+
                    ),
                    $($variant.to_string().replace("\n\t", "\n\t\t"),)+
                );
                return Err(crate::lua::lua_error(msg));
            }
        }
    );

}

macro_rules! auto_from_lua_enum_variant {
    ($value:expr, $lua:expr, $variant:ident) => (
        if $value.as_string().is_some_and(|x| x == stringify!($variant)) {
            Ok(Self::$variant)
        } else {
            Err(crate::lua::lua_error(concat!("does not match: ", stringify!($variant))))
        }
    );
    ($value:expr, $lua:expr, $variant:ident ($tuple_ty:ty)) => (
        Ok(Self::$variant(<$tuple_ty>::from_lua($value, $lua)?))
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
            let value = Temp::from_lua($value, $lua)?;
            Ok(Self::$variant{ $(
                $field: value.$field,
            )* })
        }
    );
}

pub(crate) use auto_from_lua;
pub(crate) use auto_from_lua_enum_variant;
