use std::os::fd::{AsRawFd};
use mlua::{prelude::*, UserData, UserDataMethods};
use tokio::io::{
    BufReader,
    BufWriter,
    AsyncRead,
    AsyncWrite,
    AsyncReadExt,
    AsyncWriteExt,
    AsyncBufReadExt,
};

trait Writeable<W: AsyncWrite> {
    fn get_writer(&mut self) -> &mut Option<W>;
}

trait Readable<R: AsyncRead> {
    fn get_reader(&mut self) -> Option<&mut R>;
    fn is_tty_master(&self) -> bool;
}

pub struct ReadableFile<T>(pub Option<BufReader<T>>, pub bool);
impl<T: AsyncRead> Readable<BufReader<T>> for ReadableFile<T> {
    fn get_reader(&mut self) -> Option<&mut BufReader<T>> {
        self.0.as_mut()
    }
    fn is_tty_master(&self) -> bool {
        self.1
    }
}

fn add_readable_methods<R: Send+AsyncRead+Unpin, T: 'static+Send+Readable<R>, M: UserDataMethods<T>>(methods: &mut M) {

    methods.add_async_method_mut("read", |lua, mut file, ()| async move {
        let is_tty_master = file.is_tty_master();
        if let Some(file) = file.get_reader() {
            let mut buf = [0; 4096];
            loop {
                match file.read(&mut buf).await {
                    Ok(0) => return Ok(None),
                    Ok(n) => return Ok(Some(lua.create_string(&buf[..n])?)),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    // master tty and EIO means EOF
                    Err(e) if is_tty_master && e.raw_os_error() == Some(nix::errno::Errno::EIO as _) => {
                        return Ok(None)
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
    fn get_writer(&mut self) -> &mut Option<BufWriter<T>> {
        &mut self.0
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
