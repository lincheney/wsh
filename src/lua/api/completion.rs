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
        ui.borrow().buffer.get_contents().clone()
    };

    ui.shell.trampoline_out_callback(move |mut ui, token| {
        let mut ui_clone = ui.clone();
        let result = ui_clone.shell.get_completions(token, val, Box::new(move |matches| {
            let matches: Vec<_> = matches.into_iter().map(|x| Match{inner: Rc::new(x)}).collect();

            let result = ui.shell_loop(false, callback.call_async(matches));

            if let Some(result) = crate::log_if_err(result) {
                ui.report_error::<(), _>(result);
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        }));

        match result {
            Ok(msg) => {
                if !msg.is_empty() {
                    ui_clone.borrow_mut().tui.add_zle_message(msg.as_ref());
                }
            },
            err => {
                ui_clone.report_error(err);
            },
        }
    }).await?;

    Ok(())
}

async fn insert_completion(ui: Ui, _lua: Lua, val: Match) -> Result<()> {
    let buffer = ui.borrow().buffer.get_contents().clone();
    let suffix = val.inner.as_suffix();
    let (new_buffer, new_pos) = ui.shell.insert_completion(buffer, &val.inner);
    {
        // see if this can be done as an insert
        let mut ui = ui.borrow_mut();
        ui.buffer.insert_or_set(Some(new_buffer.as_ref()), Some(new_pos));
        ui.buffer.replace_completion_suffix(suffix);
    }

    ui.trigger_buffer_change_callbacks().await;
    ui.queue_draw();
    Ok(())
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_async_fn("get_completions", get_completions)?;
    lua.set_async_fn("insert_completion", insert_completion)?;

    Ok(())
}
