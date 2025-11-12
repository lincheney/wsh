use crate::lua::{HasEventCallbacks};
use crate::ui::{Ui, ThreadsafeUiInner};
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods, MetaMethod};
use std::sync::Arc;
use crate::utils::*;

#[derive(FromLua, Clone)]
struct CompletionStream {
    inner: AsyncArcMutex<crate::shell::completion::StreamConsumer>,
    parent: crate::shell::Completer,
}

#[derive(FromLua, Clone)]
struct CompletionMatch {
    inner: Arc<crate::shell::completion::cmatch>,
}

impl UserData for CompletionStream {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method(MetaMethod::Call, |_lua, stream, ()| async move {
            let mut stream = stream.inner.lock().await;
            let chunks = stream.chunks().await;
            Ok(chunks.map(|c| c.map(|inner| CompletionMatch{inner}).collect::<Vec<_>>()))
        });

        methods.add_async_method("cancel", |_lua, stream, ()| async move {
            stream.parent.cancel().map_err(|e| mlua::Error::RuntimeError(format!("{e}")))
        });
    }
}

impl UserData for CompletionMatch {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_lua, m, ()| {
            Ok(m.inner.get_orig().map(|s| s.to_string_lossy().into_owned()))
        });
    }
}

async fn get_completions(ui: Ui, _lua: Lua, val: Option<String>) -> Result<CompletionStream> {

    let val = if let Some(val) = val {
        val.into()
    } else {
        ui.get().inner.borrow().await.buffer.get_contents().clone()
    };

    let (consumer, producer) = ui.shell.lock().await.get_completions(val.as_ref());
    let parent = producer.clone();

    // run this in a thread
    tokio::task::spawn_blocking(move || {
        let tid = nix::unistd::gettid();
        let mut shell = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let this = ui.unlocked.read();
                this.inner.borrow_mut().await.threads.insert(tid);
                ui.shell.lock().await
            })
        });
        // this blocks
        match producer.run(&mut shell) {
            Ok(msg) => {
                drop(shell);
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        let this = ui.unlocked.read();
                        let mut ui = this.inner.borrow_mut().await;
                        ui.threads.remove(&tid);
                        // ui.activate();
                        if !msg.is_empty() {
                            ui.tui.add_zle_message(msg.as_ref());
                        }
                    });
                });
            },
            err => {
                let mut ui = ui.clone();
                tokio::task::spawn(async move {
                    ui.report_error(false, err).await
                });
            },
        }
    });

    Ok(CompletionStream{inner: consumer, parent})
}

async fn insert_completion(mut ui: Ui, _lua: Lua, (stream, val): (CompletionStream, CompletionMatch)) -> Result<()> {
    {
        let this = ui.unlocked.read();
        let buffer = this.inner.borrow().await.buffer.get_contents().clone();
        let completion_word_len = stream.parent.get_completion_word_len();
        let (new_buffer, new_pos) = ui.shell.lock().await.insert_completion(buffer.as_ref(), completion_word_len, &val.inner);

        // see if this can be done as an insert
        let mut ui = this.inner.borrow_mut().await;
        let cursor = ui.buffer.cursor_byte_pos();
        let contents = ui.buffer.get_contents();
        let (prefix, suffix) = &contents.split_at_checked(cursor).unwrap_or((contents, b""));

        if new_buffer.starts_with(prefix) && new_buffer.ends_with(suffix) {
            let new_buffer = &new_buffer[prefix.len() .. new_buffer.len() - suffix.len()];
            ui.buffer.insert_at_cursor(new_buffer);
            if ui.buffer.get_cursor() != new_pos {
                ui.buffer.set_cursor(new_pos);
            }
        } else {
            ui.buffer.set(Some(&new_buffer), Some(new_pos));
        }
    }

    ui.trigger_buffer_change_callbacks(()).await;
    ui.draw().await?;
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_completions", get_completions)?;
    ui.set_lua_async_fn("insert_completion", insert_completion)?;

    Ok(())
}
