use std::os::raw::{c_int};
use std::cell::BorrowError;
use std::sync::atomic::{AtomicPtr, Ordering, AtomicUsize};
use crate::ui::{Ui, WeakUi};
use std::cell::RefCell;
use anyhow::Result;
use mlua::prelude::*;
mod api;
mod auto_from_lua;
use auto_from_lua::auto_from_lua;
pub use api::{
    init_lua,
    keybind::invoke_keybind_callback,
    KeybindMapping,
    EventCallbacks,
    HasEventCallbacks,
};

// i must use atomics here as these are used in signal handlers
static LUA_PTR: AtomicPtr<mlua::ffi::lua_State> = AtomicPtr::new(std::ptr::null_mut());
static LUA_LEVEL: AtomicUsize = AtomicUsize::new(0);
const LUA_HOOK_MASK: c_int = mlua::ffi::LUA_MASKCALL | mlua::ffi::LUA_MASKRET | mlua::ffi::LUA_MASKLINE | mlua::ffi::LUA_MASKCOUNT;

pub fn lua_error<S: ToString>(msg: S) -> mlua::Error {
    mlua::Error::RuntimeError(msg.to_string())
}

pub async fn call_lua_fn<T: IntoLuaMulti + 'static, R: FromLuaMulti>(callback: &mlua::Function, arg: T) -> LuaResult<R> {
    LUA_LEVEL.fetch_add(1, Ordering::Relaxed);
    let result = callback.call_async::<R>(arg).await;
    LUA_LEVEL.fetch_sub(1, Ordering::Relaxed);
    result
}

pub struct LuaWrapper {
    inner: Lua,
    pub api: LuaTable,
    pub ui: RefCell<WeakUi>,
}
crate::impl_deref_helper!(self: LuaWrapper, &self.inner => Lua);
crate::impl_deref_helper!(mut self: LuaWrapper, &mut self.inner => Lua);

impl LuaWrapper {
    pub fn new() -> Result<Self> {
        let inner = Lua::new();
        let api = inner.create_table()?;
        inner.globals().set("wish", &api)?;

        LUA_LEVEL.store(0, Ordering::Release);
        unsafe {
            inner.exec_raw::<()>((), |lua| {
                LUA_PTR.store(lua, Ordering::Release);
            })?;
        }

        Ok(Self {
            inner,
            api,
            ui: RefCell::default(),
        })
    }

    fn get_weak_ui(&self) -> Result<WeakUi, BorrowError> {
        self.ui.try_borrow().map(|ui| ui.clone())
    }

    fn try_upgrade_ui(ui: &WeakUi) -> LuaResult<Ui> {
        Ui::try_upgrade(ui).map_err(lua_error)
    }

    pub fn init_lua(&self) -> Result<()> {
        init_lua(self)?;

        self.inner.load(/*lua*/ r"
            local xdg_data = os.getenv('XDG_DATA_HOME')
            local home = os.getenv('HOME')
            local base = xdg_data or (home and home .. '/.local/share')
            local wish_path = base and (base .. '/wish/lua/?.lua;') or ''
            local p = (';' .. package.path .. ';'):gsub(';%./%?%.lua;', ''):gsub('^;', ''):gsub(';$', '')
            package.path = wish_path .. p
        ").exec()?;
        self.inner.load("require('wish')").exec()?;
        Ok(())
    }

    pub fn make_fn<F, A, R>(&self, func: F) -> Result<LuaFunction>
    where
        F: Fn(&Ui, &Lua, A) -> Result<R> + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
        let ui = self.get_weak_ui()?;
        Ok(self.inner.create_function(move |lua, value| {
            let ui = Self::try_upgrade_ui(&ui)?;
            func(&ui, lua, value).map_err(lua_error)
        })?)
    }

    pub fn set_fn<F, A, R>(&self, name: &str, func: F) -> Result<()>
    where
        F: Fn(&Ui, &Lua, A) -> Result<R> + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {

        Ok(self.api.set(name, self.make_fn(func)?)?)
    }

    pub fn make_async_fn<F, A, R, T>(&self, func: F) -> Result<LuaFunction>
    where
        F: Fn(Ui, Lua, A) -> T + 'static + Clone,
        A: FromLuaMulti + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + 'static,
    {
        let ui = self.get_weak_ui()?;
        Ok(self.inner.create_async_function(move |lua, value| {
            let ui = ui.clone();
            let func = func.clone();
            async move {
                let ui = Self::try_upgrade_ui(&ui)?;
                func(ui, lua, value).await.map_err(lua_error)
            }
        })?)
    }

    pub fn set_async_fn<F, A, R, T>(&self, name: &str, func: F) -> Result<()>
    where
        F: Fn(Ui, Lua, A) -> T + 'static + Clone,
        A: FromLuaMulti + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + 'static,
    {
        Ok(self.api.set(name, self.make_async_fn(func)?)?)
    }

}

impl Drop for LuaWrapper {
    fn drop(&mut self) {
        LUA_PTR.store(std::ptr::null_mut(), Ordering::Relaxed);
    }
}

#[inline(always)]
pub fn set_sigint_hook() {
    if LUA_LEVEL.load(Ordering::Relaxed) > 0 {
        let lua = LUA_PTR.load(Ordering::Relaxed);
        if !lua.is_null() {
            unsafe {
                // this is signal safe
                // https://lua-l.lua.narkive.com/2F1sf9Vo/signal-safety-of-lua-sethook
                mlua::ffi::lua_sethook(lua, Some(lua_sigint_hook), LUA_HOOK_MASK, 1);
            }
        }
    }
}

extern "C-unwind" fn lua_sigint_hook(lua: *mut mlua::ffi::lua_State, _ar: *mut mlua::ffi::lua_Debug) {
    unsafe {
        // keep interrupting lua so long as there is more
        if LUA_LEVEL.load(Ordering::Relaxed) <= 1 {
            mlua::ffi::lua_sethook(lua, None, LUA_HOOK_MASK, 1);
        }
        mlua::ffi::lua_pushliteral(lua, c"interrupted");
        mlua::ffi::lua_error(lua);
    }
}

#[derive(Debug, Copy, Clone)]
pub struct FromLuaStr<T>(T);

impl<T: std::str::FromStr> FromLua for FromLuaStr<T>
    where T::Err: std::fmt::Display
{
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let Some(value) = value.as_string() {
            let value = value.to_str()?;
            Ok(Self(T::from_str(&value).map_err(crate::lua::lua_error)?))
        } else {
            Err(crate::lua::lua_error("expected string"))
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct FromLuaSerde<T>(T);

impl<T: serde::de::DeserializeOwned> FromLua for FromLuaSerde<T> {
    fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self> {
        Ok(Self(lua.from_value(value)?))
    }
}
