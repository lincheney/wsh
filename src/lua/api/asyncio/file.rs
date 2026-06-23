use std::num::NonZeroU16;
use std::os::fd::{AsRawFd, RawFd};
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
use tokio::sync::{RwLock, RwLockWriteGuard};

trait Writeable<W: 'static + AsyncWrite> {
    async fn get_writer(&self) -> RwLockWriteGuard<'_, Option<W>>;
}

trait Readable<R: 'static + AsyncRead> {
    async fn get_reader(&self) -> RwLockWriteGuard<'_, Option<R>>;
    fn is_tty_master(&self) -> bool;
}

pub struct ReadableFile<T> {
    pub inner: RwLock<Option<BufReader<T>>>,
    pub is_tty_master: bool,
    pub fd: RawFd,
}
impl<T: 'static + AsyncRead> Readable<BufReader<T>> for ReadableFile<T> {
    async fn get_reader(&self) -> RwLockWriteGuard<'_, Option<BufReader<T>>> {
        self.inner.write().await
    }
    fn is_tty_master(&self) -> bool {
        self.is_tty_master
    }
}

fn add_readable_methods<R: 'static+AsyncRead+Unpin, T: 'static+Readable<R>, M: UserDataMethods<T>>(methods: &mut M) {

    methods.add_async_method("read", |lua, file, ()| async move {
        let is_tty_master = file.is_tty_master();
        if let Some(file) = &mut *file.get_reader().await {
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

    methods.add_async_method("read_to_end", |lua, file, ()| async move {
        if let Some(file) = &mut *file.get_reader().await {
            let mut buf = vec![];
            file.read_to_end(&mut buf).await?;
            return Ok(Some(lua.create_string(&buf)?));
        }
        Ok(None)
    });

}

fn add_bufreadable_methods<R: 'static+AsyncRead+AsyncBufReadExt+Unpin, T: 'static+Readable<R>, M: UserDataMethods<T>>(methods: &mut M) {

    methods.add_async_method("read_until", |lua, file, val: u8| async move {
        if let Some(file) = &mut *file.get_reader().await {
            let mut buf = vec![];
            let n = file.read_until(val, &mut buf).await?;
            if n != 0 {
                return Ok(Some(lua.create_string(&buf[..n])?))
            }
        }
        Ok(None)
    });


    methods.add_async_method("read_line", |lua, file, ()| async move {
        if let Some(file) = &mut *file.get_reader().await {
            let mut buf = vec![];
            let n = file.read_until(b'\n', &mut buf).await?;
            if n != 0 {
                return Ok(Some(lua.create_string(&buf[..n])?))
            }
        }
        Ok(None)
    });

}

fn add_writeable_methods<R: 'static+AsyncWrite+Unpin, T: 'static+Writeable<R>, M: UserDataMethods<T>>(methods: &mut M) {
    methods.add_async_method("write", |_lua, file, val: LuaString| async move {
        if let Some(file) = &mut *file.get_writer().await {
            file.write_all(&val.as_bytes()).await?;
            file.flush().await?;
        }
        Ok(())
    });
}

impl<T: AsyncRead + AsRawFd> ReadableFile<T> {
    pub fn as_raw_fd(&self) -> Option<RawFd> {
        Some(self.fd)
    }
}

impl<T: AsyncRead + AsRawFd + Unpin + 'static> UserData for ReadableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.as_raw_fd())
        });

        methods.add_async_method("close", |_lua, file, ()| async move {
            *file.inner.write().await = None;
            Ok(())
        });

        methods.add_method("set_tty_size", |_lua, file, (rows, cols): (Option<u16>, Option<u16>)| {
            if !file.is_tty_master() {
                return Err(LuaError::RuntimeError("not a tty master".into()))
            }

            let rows = if let Some(rows) = rows {
                Some(NonZeroU16::new(rows).ok_or(LuaError::RuntimeError("rows must be > 0".into()))?)
            } else {
                None
            };
            let cols = if let Some(cols) = cols {
                Some(NonZeroU16::new(cols).ok_or(LuaError::RuntimeError("cols must be > 0".into()))?)
            } else {
                None
            };

            if let Some(fd) = file.as_raw_fd() {
                crate::shell::set_zpty_size(fd, None, rows, cols).map_err(|e| LuaError::RuntimeError(e.to_string()))?;
            }
            Ok(())
        });

        add_readable_methods(methods);
        add_bufreadable_methods(methods);
    }
}

pub struct WriteableFile<T>{
    pub inner: RwLock<Option<BufWriter<T>>>,
    pub fd: RawFd,
}
impl<T: 'static + AsyncWrite> Writeable<BufWriter<T>> for WriteableFile<T> {
    async fn get_writer(&self) -> RwLockWriteGuard<'_, Option<BufWriter<T>>> {
        self.inner.write().await
    }
}

impl<T: AsyncWrite + AsRawFd> WriteableFile<T> {
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl<T: AsyncWrite + AsRawFd + Unpin + 'static> UserData for WriteableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("as_fd", |_lua, file, ()| async move {
            Ok(file.as_raw_fd())
        });

        methods.add_async_method("close", |_lua, file, ()| async move {
            *file.inner.write().await = None;
            Ok(())
        });

        add_writeable_methods::<BufWriter<T>, Self, M>(methods);
    }
}
