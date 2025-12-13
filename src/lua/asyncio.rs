use std::os::fd::AsRawFd;
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};
use tokio::fs::File;
use tokio::io::{
    BufReader,
    BufWriter,
    BufStream,
    AsyncRead,
    AsyncWrite,
    AsyncReadExt,
    AsyncWriteExt,
    AsyncBufReadExt,
};
use crate::ui::Ui;

trait Writeable<W: AsyncWrite> {
    fn get_writer(&mut self) -> Option<&mut W>;
}

trait Readable<R: AsyncRead> {
    fn get_reader(&mut self) -> Option<&mut R>;
    fn is_tty_master(&self) -> bool;
}

pub struct ReadableFile<T>(pub Option<BufReader<T>>);
impl<T: AsyncRead> Readable<BufReader<T>> for ReadableFile<T> {
    fn get_reader(&mut self) -> Option<&mut BufReader<T>> {
        self.0.as_mut()
    }
    fn is_tty_master(&self) -> bool {
        false
    }
}

fn add_readable_methods<R: Send+AsyncRead+Unpin, T: 'static+Send+Readable<R>, M: UserDataMethods<T>>(methods: &mut M) {

    methods.add_async_method_mut("read", |lua, mut file, ()| async move {
        let is_tty_master = file.is_tty_master();
        if let Some(file) = file.get_reader() {
            let mut buf = [0; 4096];
            loop {
                match file.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => return Ok(Some(lua.create_string(&buf[..n])?)),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    // master tty and EIO means EOF
                    Err(e) if is_tty_master && e.raw_os_error() == Some(nix::errno::Errno::EIO as _) => {
                        break
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(None)
    });

    methods.add_async_method_mut("read_all", |lua, mut file, ()| async move {
        if let Some(file) = file.get_reader() {
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

}

fn add_bufreadable_methods<R: Send+AsyncRead+AsyncBufReadExt+Unpin, T: 'static+Send+Readable<R>, M: UserDataMethods<T>>(methods: &mut M) {

    methods.add_async_method_mut("read_until", |lua, mut file, val: u8| async move {
        if let Some(file) = file.get_reader() {
            let mut buf = vec![];
            let n = file.read_until(val, &mut buf).await?;
            if n != 0 {
                return Ok(Some(lua.create_string(&buf[..n])?))
            }
        }
        Ok(None)
    });


    methods.add_async_method_mut("read_line", |lua, mut file, ()| async move {
        if let Some(file) = file.get_reader() {
            let mut buf = vec![];
            let n = file.read_until(b'\n', &mut buf).await?;
            if n != 0 {
                return Ok(Some(lua.create_string(&buf[..n])?))
            }
        }
        Ok(None)
    });

}

fn add_writeable_methods<R: Send+AsyncWrite+Unpin, T: 'static+Send+Writeable<R>, M: UserDataMethods<T>>(methods: &mut M) {
    methods.add_async_method_mut("write", |_lua, mut file, val: LuaString| async move {
        if let Some(file) = file.get_writer() {
            file.write_all(&val.as_bytes()).await?;
            file.flush().await?;
        }
        Ok(())
    });
}

impl<T: AsyncRead + AsRawFd + Unpin + mlua::MaybeSend + 'static> UserData for ReadableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.0.as_ref().map(|x| x.get_ref().as_raw_fd()))
        });

        methods.add_method_mut("close", |_lua, file, ()| {
            file.0 = None;
            Ok(())
        });

        add_readable_methods(methods);
        add_bufreadable_methods(methods);
    }
}

pub struct WriteableFile<T>(pub Option<BufWriter<T>>);
impl<T: AsyncWrite> Writeable<BufWriter<T>> for WriteableFile<T> {
    fn get_writer(&mut self) -> Option<&mut BufWriter<T>> {
        self.0.as_mut()
    }
}

impl<T: AsyncWrite + AsRawFd + Unpin + mlua::MaybeSend + 'static> UserData for WriteableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.0.as_ref().map(|x| x.get_ref().as_raw_fd()))
        });

        methods.add_method_mut("close", |_lua, file, ()| {
            file.0 = None;
            Ok(())
        });

        add_writeable_methods(methods);
    }
}

pub struct ReadWriteFile{
    pub inner: Option<BufStream<File>>,
    pub is_tty_master: bool,
}
impl Readable<BufStream<File>> for ReadWriteFile {
    fn get_reader(&mut self) -> Option<&mut BufStream<File>> {
        self.inner.as_mut()
    }
    fn is_tty_master(&self) -> bool {
        self.is_tty_master
    }
}
impl Writeable<BufStream<File>> for ReadWriteFile {
    fn get_writer(&mut self) -> Option<&mut BufStream<File>> {
        self.inner.as_mut()
    }
}

impl UserData for ReadWriteFile {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.inner.as_ref().map(|x| x.get_ref().as_raw_fd()))
        });

        methods.add_method_mut("close", |_lua, file, ()| {
            file.inner = None;
            Ok(())
        });

        add_readable_methods(methods);
        add_bufreadable_methods(methods);
        add_writeable_methods(methods);
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
                Ok(Some(receiver.await.map_err(|e| LuaError::RuntimeError(format!("{e}")))?))
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

    tbl.set("sleep", ui.lua.create_async_function(|_, secs: f64| async move {
        tokio::time::sleep(std::time::Duration::from_secs_f64(secs)).await;
        Ok(())
    })?)?;

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
