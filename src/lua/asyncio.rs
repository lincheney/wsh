use std::os::fd::AsRawFd;
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};
use tokio::io::{BufReader, BufWriter, AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, AsyncBufReadExt};
use crate::ui::Ui;

pub struct ReadableFile<T>(pub Option<BufReader<T>>);

impl<T: AsyncRead + AsRawFd + std::marker::Unpin + mlua::MaybeSend + 'static> UserData for ReadableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.0.as_ref().map(|x| x.get_ref().as_raw_fd()))
        });

        methods.add_method_mut("close", |_lua, file, ()| {
            file.0 = None;
            Ok(())
        });

        methods.add_async_method_mut("read", |lua, mut file, _val: ()| async move {
            if let Some(file) = file.0.as_mut() {
                let mut buf = [0; 4096];
                let n = file.read(&mut buf).await?;
                if n != 0 {
                    return Ok(Some(lua.create_string(&buf[..n])?))
                }
            }
            Ok(None)
        });

        methods.add_async_method_mut("read_all", |lua, mut file, _val: ()| async move {
            if let Some(file) = file.0.as_mut() {
                let mut buf = vec![];
                loop {
                    let start = buf.len();
                    buf.resize(buf.len() + 4096, 0);
                    let slice = &mut buf[start..];
                    let n = file.read(slice).await?;
                    buf.resize(start + n, 0);
                    if n == 0 {
                        return Ok(Some(lua.create_string(&buf)?));
                    }
                }
            }
            Ok(None)
        });

        methods.add_async_method_mut("read_until", |lua, mut file, val: u8| async move {
            if let Some(file) = file.0.as_mut() {
                let mut buf = vec![];
                let n = file.read_until(val, &mut buf).await?;
                if n != 0 {
                    return Ok(Some(lua.create_string(&buf[..n])?))
                }
            }
            Ok(None)
        });

        methods.add_async_method_mut("read_line", |lua, mut file, _val: ()| async move {
            if let Some(file) = file.0.as_mut() {
                let mut buf = vec![];
                let n = file.read_until(b'\n', &mut buf).await?;
                if n != 0 {
                    return Ok(Some(lua.create_string(&buf[..n])?))
                }
            }
            Ok(None)
        });

    }
}

pub struct WriteableFile<T>(pub Option<BufWriter<T>>);

impl<T: AsyncWrite + AsRawFd + std::marker::Unpin + mlua::MaybeSend + 'static> UserData for WriteableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.0.as_ref().map(|x| x.get_ref().as_raw_fd()))
        });

        methods.add_method_mut("close", |_lua, file, ()| {
            file.0 = None;
            Ok(())
        });

        methods.add_async_method_mut("write", |_lua, mut file, val: LuaString| async move {
            if let Some(file) = file.0.as_mut() {
                file.write_all(&val.as_bytes()).await?;
                file.flush().await?;
            }
            Ok(())
        });

    }
}

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
                Ok(Some(receiver.await.map_err(|e| LuaError::RuntimeError(format!("{}", e)))?))
            } else {
                Ok(None)
            }
        });
    }
}

pub async fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_fn("schedule", schedule)?;

    let tbl = ui.lua.create_table()?;
    ui.get_lua_api()?.set("async", &tbl)?;

    tbl.set("sleep", ui.lua.create_async_function(|_, millis: u64| async move {
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        Ok(())
    })?)?;

    // this exists bc mlua calls coroutine.resume all the time so we can't use it
    tbl.set("promise", ui.lua.create_function(|lua, ()| {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        Ok(lua.pack_multi((
            Sender(Some(sender)),
            Receiver(Some(receiver)),
        ))?)
    })?)?;

    Ok(())
}
