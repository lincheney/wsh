use tokio::sync::{mpsc, Mutex};
use crate::lua::{HasEventCallbacks};
use crate::ui::{Ui, ThreadsafeUiInner};
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods, MetaMethod};
use std::sync::Arc;

#[derive(FromLua, Clone)]
struct Stream {
    inner: Arc<Mutex<mpsc::UnboundedReceiver<Vec<crate::shell::completion::Match>>>>,
}

#[derive(FromLua, Clone)]
struct Match {
    inner: Arc<crate::shell::completion::Match>,
}

impl UserData for Stream {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method(MetaMethod::Call, |_lua, stream, ()| async move {
            let mut stream = stream.inner.lock().await;
            let Some(matches) = stream.recv().await
                else { return Ok(None) };
            let matches: Vec<_> = matches.into_iter().map(|x| Match{inner: Arc::new(x)}).collect();
            Ok(Some(matches))
        });

        methods.add_async_method("cancel", |_lua, stream, ()| async move {
            if stream.inner.lock().await.is_closed() {
                Ok(())
            } else {
                crate::shell::control_c().map_err(|e| mlua::Error::RuntimeError(e.to_string()))
            }
        });
    }
}

impl UserData for Match {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_lua, m, ()| {
            Ok(m.inner.get_orig().map(|s| s.to_string_lossy().into_owned()))
        });
    }
}

async fn get_completions(ui: Ui, _lua: Lua, val: Option<String>) -> Result<Stream> {

    let val = if let Some(val) = val {
        val.into()
    } else {
        ui.get().inner.borrow().buffer.get_contents().clone()
    };

    let (sender, receiver) = mpsc::unbounded_channel();

    // run this in another thread so it doesn't block us returning
    tokio::task::spawn(async move {
        let tid = nix::unistd::gettid();
        ui.add_thread(tid);

        match ui.shell.get_completions(val, sender).await {
            Ok(msg) => {
                ui.remove_thread(tid);
                // ui.activate();
                if !msg.is_empty() {
                    let this = ui.unlocked.read();
                    let mut ui = this.inner.borrow_mut();
                    ui.tui.add_zle_message(msg.as_ref());
                }
            },
            err => {
                let mut ui = ui.clone();
                tokio::task::spawn(async move {
                    ui.report_error(err).await;
                });
            },
        }
    });

    Ok(Stream{inner: Arc::new(Mutex::new(receiver))})
}

async fn insert_completion(ui: Ui, _lua: Lua, val: Match) -> Result<()> {
    let buffer = {
        let this = ui.unlocked.read();
        this.inner.borrow().buffer.get_contents().clone()
    };

    let (new_buffer, new_pos) = ui.shell.insert_completion(buffer, val.inner).await;
    {
        // see if this can be done as an insert
        let this = ui.unlocked.read();
        let mut ui = this.inner.borrow_mut();
        ui.buffer.insert_or_set(Some(new_buffer.as_ref()), Some(new_pos));
    }

    ui.trigger_buffer_change_callbacks(()).await;
    ui.queue_draw();
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_completions", get_completions)?;
    ui.set_lua_async_fn("insert_completion", insert_completion)?;

    Ok(())
}
