use std::sync::{atomic::Ordering};
use crate::ui::{Ui, WeakUi};
use std::cell::RefCell;
use anyhow::Result;
use mlua::prelude::*;
mod api;
pub use api::{
    init_lua,
    keybind::invoke_keybind_callback,
    KeybindMapping,
    EventCallbacks,
    HasEventCallbacks,
};

pub fn lua_error<S: ToString>(msg: S) -> mlua::Error {
    mlua::Error::RuntimeError(msg.to_string())
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
        Ok(Self {
            inner,
            api,
            ui: RefCell::default(),
        })
    }

    fn get_weak_ui(&self) -> WeakUi {
        self.ui.borrow().clone()
    }

    fn try_upgrade_ui(ui: &WeakUi) -> LuaResult<Ui> {
        Ui::try_upgrade(ui).map_err(lua_error)
    }

    pub fn init_lua(&self) -> Result<()> {
        init_lua(&self)?;

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

    pub fn make_fn<F, A, R>(&self, func: F) -> LuaResult<LuaFunction>
    where
        F: Fn(&Ui, &Lua, A) -> Result<R> + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
        let ui = self.get_weak_ui();
        self.inner.create_function(move |lua, value| {
            let ui = Self::try_upgrade_ui(&ui)?;
            func(&ui, lua, value).map_err(lua_error)
        })
    }

    pub fn set_fn<F, A, R>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(&Ui, &Lua, A) -> Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {

        let func = self.make_fn(func)?;
        self.api.set(name, func)
    }

    pub fn make_async_fn<F, A, R, T>(&self, func: F) -> LuaResult<LuaFunction>
    where
        F: Fn(Ui, Lua, A) -> T + 'static + Clone,
        A: FromLuaMulti + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + 'static,
    {
        let ui = self.get_weak_ui();
        self.inner.create_async_function(move |lua, value| {
            let ui = ui.clone();
            let func = func.clone();
            async move {
                let ui = Self::try_upgrade_ui(&ui)?;
                func(ui, lua, value).await.map_err(lua_error)
            }
        })
    }

    pub fn set_async_fn<F, A, R, T>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(Ui, Lua, A) -> T + 'static + Clone,
        A: FromLuaMulti + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + 'static,
    {
        let func = self.make_async_fn(func)?;
        self.api.set(name, func)
    }

    pub async fn call_lua_fn<T: IntoLuaMulti + 'static>(&self, callback: mlua::Function, arg: T) -> LuaResult<LuaValue> {
        crate::shell::LUA_LEVEL.fetch_add(1, Ordering::Relaxed);
        let result = callback.call_async::<LuaValue>(arg).await;
        crate::shell::LUA_LEVEL.fetch_sub(1, Ordering::Relaxed);
        result
    }



}
