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
use crate::shell::externs::GlobalState;
use anyhow::Result;

#[derive(Default)]
pub struct Cookie {
    pub dirty: bool,
    passthrough: bool,
    buffer: Option<BString>,
}

impl Cookie {

    fn new() -> Result<(Box<Self>, NonNull<FILE>)> {
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
    if size > 0 {
        unsafe {
            let buf = std::slice::from_raw_parts(buf as *const u8, size);
            let cookie = &mut *(cookie as *mut Cookie);
            if let Some(buffer) = &mut cookie.buffer {
                buffer.push_str(buf);
            }
            if cookie.passthrough {
                // okkkkkkkk
                let result = GlobalState::with(|ui| {
                    ui.try_borrow_mut()?.tui.add_zle_message(buf);
                    // draw immediately bc zsh may be prompting the user with a question
                    ui.clone().shell_loop(false, ui.draw())??;
                    anyhow::Ok(())
                });
                crate::log_if_err(result);
            }
        }
    }

    size as _
}


pub struct Sink {
    pub cookie: Box<Cookie>,
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
        if let Some(buffer) = &mut self.cookie.buffer {
            buffer.clear();
        }
    }

    pub fn read(&mut self) -> BString {
        self.cookie.buffer.take().unwrap_or_default()
    }

    pub fn override_file(&mut self, file: *mut *mut FILE, passthrough: bool, capture: bool) -> FileGuard<'_> {
        self.cookie.passthrough = passthrough;
        if capture {
            self.cookie.buffer.get_or_insert_default();
        } else {
            self.cookie.buffer = None;
        }
        FileGuard::new(self, file)
    }

    pub fn override_stdout(&mut self, passthrough: bool, capture: bool) -> FileGuard<'_> {
        self.override_file(&raw mut stdout, passthrough, capture)
    }

    pub fn override_stderr(&mut self, passthrough: bool, capture: bool) -> FileGuard<'_> {
        self.override_file(&raw mut stderr, passthrough, capture)
    }

    pub fn override_shout(&mut self, passthrough: bool, capture: bool) -> FileGuard<'_> {
        self.override_file((&raw mut zsh_sys::shout).cast(), passthrough, capture)
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
