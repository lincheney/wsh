use crate::lua::LuaWrapper;
use std::ops::ControlFlow;
use crate::lua::{HasEventCallbacks};
use crate::lua::{Ui};
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods, MetaMethod};
use std::rc::Rc;

#[derive(FromLua, Clone)]
struct Match {
    inner: Rc<crate::shell::completion::Match>,
}

impl UserData for Match {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_lua, m, ()| {
            Ok(m.inner.get_orig().map(|s| s.to_string_lossy().into_owned()))
        });

        methods.add_method("mode", |_lua, m, ()| {
            Ok(m.inner.get_mode())
        });

        methods.add_method("fmode", |_lua, m, ()| {
            Ok(m.inner.get_fmode())
        });
    }
}

async fn get_completions(ui: Ui, _lua: Lua, (val, callback): (Option<String>, LuaFunction)) -> Result<()> {

    let val = if let Some(val) = val {
        val.into()
    } else {
        ui.try_borrow()?.buffer.get_contents().clone()
    };

    ui.shell.trampoline_out_callback(move |ui, token| {
        let ui = ui.clone();
        let ui2 = ui.clone();
        let result = ui2.shell.get_completions(token, val, Box::new(move |matches| {

            let result = (|| {
                let matches = ui.lua.create_sequence_from(matches.into_iter().map(|x| Match{inner: Rc::new(x)}))?;
                ui.shell_loop(false, crate::lua::call_lua_fn::<_, LuaValue>(&callback, matches))??;
                anyhow::Ok(())
            })();

            if crate::log_if_err(ui.report_error::<(), _>(result)).is_some() {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }));

        if let Some(msg) = result && !msg.is_empty() {
            let tui = &mut ui2.try_borrow_mut()?.tui;
            tui.clear_zle();
            tui.add_zle_message(msg.as_ref());
        }
        anyhow::Ok(())
    }).await??;

    Ok(())
}

async fn insert_completion(ui: Ui, _lua: Lua, val: Match) -> Result<()> {
    let buffer = ui.try_borrow()?.buffer.get_contents().clone();
    let suffix = val.inner.as_suffix();
    let (new_buffer, new_pos) = ui.shell.insert_completion(buffer, &val.inner);
    {
        // see if this can be done as an insert
        let mut ui = ui.try_borrow_mut()?;
        ui.buffer.insert_or_set(Some(new_buffer.as_ref()), Some(new_pos));
        ui.buffer.replace_completion_suffix(suffix);
    }

    ui.trigger_buffer_change_callbacks().await?;
    ui.trigger_buffer_cursor_move_callbacks().await?;
    ui.queue_draw();
    Ok(())
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_async_fn("get_completions", get_completions)?;
    lua.set_async_fn("insert_completion", insert_completion)?;

    Ok(())
}
