#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::unreadable_literal)]

use nix::libc::{FILE, ssize_t, size_t};
use std::os::raw::*;
include!(concat!(env!("OUT_DIR"), "/fopencookie.rs"));

use std::ptr::{NonNull};
use bstr::{BString, ByteVec};
use anyhow::Result;

#[derive(Default)]
struct Cookie {
    file: Option<*mut FILE>,
    buffer: BString,
}

impl Cookie {

    pub fn new() -> Result<(Box<Self>, NonNull<FILE>)> {
        let mut cookie = Box::new(Self::default());
        let funcs = cookie_io_functions_t {
            read: None,
            write: Some(cookie_write),
            seek: None,
            close: None,
        };
        let ptr = &raw mut *cookie;
        let file = unsafe{ fopencookie(ptr.cast(), c"w".as_ptr(), funcs) };

        if let Some(file) = NonNull::new(file) {
            Ok((cookie, file))
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    }

}

unsafe extern "C" fn cookie_write(cookie: *mut c_void, buf: *const c_char, size: size_t) -> ssize_t {
    unsafe {
        let cookie = &mut *(cookie as *mut Cookie);
        let ret = if let Some(file) = cookie.file {
            let ret = nix::libc::fwrite(buf as _, 1, size, file) as _;
            nix::libc::fflush(file);
            ret
        } else {
            size
        };
        if ret > 0 {
            cookie.buffer.push_str(std::slice::from_raw_parts(buf as *const u8, ret));
        }
        ret as _
    }
}


pub struct Sink {
    cookie: Box<Cookie>,
    file: NonNull<FILE>,
}

impl Sink {

    pub fn new() -> Result<Self> {
        let (cookie, file) = Cookie::new()?;
        Ok(Self {
            cookie,
            file,
        })
    }

    pub fn clear(&mut self) {
        self.cookie.buffer.clear();
    }

    pub fn read(&mut self) -> BString {
        std::mem::take(&mut self.cookie.buffer)
    }

    pub fn override_file(&mut self, file: *mut *mut FILE, passthrough: bool) -> FileGuard<'_> {
        self.cookie.file = passthrough.then_some(unsafe{ *file });
        FileGuard::new(self, file)
    }

    pub fn override_stdout(&mut self, passthrough: bool) -> FileGuard<'_> {
        self.override_file(&raw mut stdout, passthrough)
    }

    pub fn override_stderr(&mut self, passthrough: bool) -> FileGuard<'_> {
        self.override_file(&raw mut stderr, passthrough)
    }

    pub fn override_shout(&mut self, passthrough: bool) -> FileGuard<'_> {
        self.override_file((&raw mut zsh_sys::shout).cast(), passthrough)
    }
}

pub struct FileGuard<'a> {
    pub _inner: &'a mut Sink,
    dest: *mut *mut FILE,
    old_file: *mut FILE,
}

impl<'a> FileGuard<'a> {
    pub fn new(parent: &'a mut Sink, file: *mut *mut FILE) -> FileGuard<'a> {
        unsafe {
            let old_file = *file;
            *file = parent.file.as_ptr();
            FileGuard{
                _inner: parent,
                dest: file,
                old_file,
            }
        }
    }
}

impl Drop for FileGuard<'_> {
    fn drop(&mut self) {
        unsafe{
            nix::libc::fflush(*self.dest);
            *self.dest = self.old_file;
        }
    }
}
