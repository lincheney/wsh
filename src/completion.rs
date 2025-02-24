use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods, MetaMethod};
use std::sync::Arc;
use crate::ui::Ui;
use crate::shell::Shell;
use crate::utils::*;

struct CompletionStream {
    inner: AsyncArcMutex<crate::zsh::completion::StreamConsumer>,
}

#[derive(FromLua, Clone)]
struct CompletionMatch {
    inner: Arc<crate::zsh::cmatch>,
}

impl UserData for CompletionStream {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method(MetaMethod::Call, |_lua, stream, ()| async move {
            let mut stream = stream.inner.lock().await;
            let chunks = stream.chunks().await;
            Ok(chunks.map(|c| c.map(|inner| CompletionMatch{inner}).collect::<Vec<_>>()))
        });

        methods.add_async_method("cancel", |_lua, stream, ()| async move {
            stream.inner.lock().await.cancel().map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))
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

async fn get_completions(ui: Ui, shell: Shell, _lua: Lua, val: Option<String>) -> Result<CompletionStream> {

    let val = if let Some(val) = val {
        val.into()
    } else {
        ui.borrow().await.buffer.get_contents().clone()
    };

    let result = shell.lock().await.get_completions(val.as_ref());
    let (consumer, producer) = result.map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))?;

    let shell_clone = shell.clone();
    let mut ui_clone = ui.clone();
    // run this in a thread
    async_std::task::spawn_blocking(move || {
        let tid = nix::unistd::gettid();
        let shell = shell_clone.lock();
        let shell = async_std::task::block_on(async {
            ui_clone.borrow_mut().await.threads.insert(tid);
            shell.await
        });
        producer.start(&shell);
        drop(shell);
        async_std::task::block_on(async {
            let mut ui = ui_clone.borrow_mut().await;
            ui.threads.remove(&tid);
            ui.activate()
        })
    });

    Ok(CompletionStream{inner: consumer})
}

async fn insert_completion(mut ui: Ui, shell: Shell, _lua: Lua, val: CompletionMatch) -> Result<()> {
    let buffer = ui.borrow().await.buffer.get_contents().clone();
    let (buffer, pos) = shell.lock().await.insert_completion(buffer.as_ref(), &val.inner);
    ui.borrow_mut().await.buffer.set(Some(&buffer), Some(pos));
    ui.draw(&shell).await?;
    Ok(())
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("get_completions", shell, get_completions).await?;
    ui.set_lua_async_fn("insert_completion", shell, insert_completion).await?;

    Ok(())
}
