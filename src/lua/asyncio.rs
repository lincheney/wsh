use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};
use crate::ui::Ui;
mod file;
pub use file::{ReadableFile, WriteableFile};

fn schedule(ui: &Ui, _lua: &Lua, cb: LuaFunction) -> Result<()> {
    let ui = ui.clone();
    tokio::task::spawn(async move {
        ui.call_lua_fn(false, cb, ()).await;
    });
    Ok(())
}

struct Sender(Option<tokio::sync::oneshot::Sender<LuaValue>>);
struct Receiver(Option<tokio::sync::oneshot::Receiver<LuaValue>>);

impl UserData for Sender {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method_mut(mlua::MetaMethod::Call, |_lua, sender, val| {
            if let Some(sender) = sender.0.take() {
                let _ = sender.send(val);
            }
            Ok(())
        });
    }
}
impl UserData for Receiver {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method_mut(mlua::MetaMethod::Call, |_lua, mut receiver, ()| async move {
            if let Some(receiver) = receiver.0.take() {
                Ok(Some(receiver.await.map_err(|e| LuaError::RuntimeError(e.to_string()))?))
            } else {
                Ok(None)
            }
        });
    }
}


pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_fn("schedule", schedule)?;

    let tbl = ui.lua.create_table()?;
    ui.get_lua_api()?.set("async", &tbl)?;

    // this exists bc mlua calls coroutine.resume all the time so we can't use it
    tbl.set("promise", ui.lua.create_function(|lua, ()| {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        lua.pack_multi((
            Sender(Some(sender)),
            Receiver(Some(receiver)),
        ))
    })?)?;

    Ok(())
}
