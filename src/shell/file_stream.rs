#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::unreadable_literal)]

use nix::libc::{FILE, ssize_t, size_t};
use std::os::raw::*;
include!(concat!(env!("OUT_DIR"), "/fopencookie.rs"));

use std::ptr::{NonNull};
use std::time::{Instant, Duration};
use bstr::{BString, ByteVec};
use crate::shell::externs::GlobalState;
use anyhow::Result;

const MAX_DRAW_DURATION: Duration = Duration::from_millis(50);

pub struct Cookie {
    pub dirty: bool,
    passthrough: bool,
    buffer: Option<BString>,
    last_draw: Instant,
}

impl Cookie {

    fn new() -> Result<(Box<Self>, NonNull<FILE>)> {
        let mut cookie = Box::new(Self {
            dirty: false,
            passthrough: false,
            buffer: None,
            last_draw: Instant::now() - MAX_DRAW_DURATION,
        });
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

unsafe extern "C" fn cookie_write(cookie: *mut c_void, buf: *const c_char, mut size: size_t) -> ssize_t {
    if size > 0 {
        unsafe {
            let cookie = &mut *(cookie as *mut Cookie);

            // we're going to cheat a bit to throttle the drawing
            // the max buf size is 8192
            // if we get that much data, chances are there is more coming
            // and we should delay drawing until then
            // but it could be there isn't
            // so we consume 1 byte less and essentially force
            // the caller to resend that 1 byte immediately
            // so there should always be a follow up call
            let draw = if cookie.passthrough && size == 8192 {
                size -= 1;
                cookie.last_draw.elapsed() >= MAX_DRAW_DURATION
            } else {
                cookie.passthrough
            };

            let buf = std::slice::from_raw_parts(buf as *const u8, size);
            if let Some(buffer) = &mut cookie.buffer {
                buffer.push_str(buf);
            }
            if draw {
                cookie.last_draw = Instant::now();
                // okkkkkkkk
                let result = GlobalState::with(|ui| {
                    ui.try_borrow_mut()?.tui.add_zle_message(buf);
                    // draw immediately bc zsh may be prompting the user with a question
                    // faster non blocking draw with no cursor checking
                    ui.draw_blocking(true)?;
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

    pub fn read(&mut self) -> Option<BString> {
        self.cookie.buffer.take_if(|b| !b.is_empty())
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
