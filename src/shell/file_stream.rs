use std::os::raw::c_char;
use std::ptr::{null_mut, NonNull};
use bstr::{BString};
use anyhow::Result;

unsafe extern "C" {
    static mut stdout: *mut nix::libc::FILE;
    static mut stderr: *mut nix::libc::FILE;
}

pub struct Sink {
    file: NonNull<nix::libc::FILE>,
    buffer: Box<*mut c_char>,
    bufsize: Box<nix::libc::size_t>,
}

impl Sink {
    pub fn new() -> Result<Self> {
        unsafe {
            // allocate stable holders on heap and keep raw pointers to them
            let mut buffer: Box<*mut c_char> = Box::new(null_mut());
            let mut bufsize = Box::new(0);

            if let Some(file) = NonNull::new(nix::libc::open_memstream(buffer.as_mut() as _, &raw mut *bufsize)) {
                Ok(Self {
                    file,
                    buffer,
                    bufsize,
                })
            } else {
                Err(std::io::Error::last_os_error().into())
            }
        }
    }

    fn check_err(error: i32) -> Result<(), std::io::Error> {
        if error == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(error))
        }
    }

    pub fn clear(&mut self) -> Result<(), std::io::Error> {
        // nothing buffered in-memory; nothing to clear
        let buffer = *self.buffer.as_ref();
        if !buffer.is_null() {
            unsafe {
                Self::check_err(nix::libc::fseek(self.file.as_ptr(), 0, nix::libc::SEEK_SET))?;
                Self::check_err(nix::libc::fflush(self.file.as_ptr()))?;
            }
        }
        Ok(())
    }

    pub fn read(&mut self) -> Result<BString, std::io::Error> {
        unsafe {
            Self::check_err(nix::libc::fflush(self.file.as_ptr()))?;

            if *self.bufsize == 0 {
                Ok("".into())
            } else if self.buffer.is_null() {
                Err(std::io::Error::other("buffer is null"))
            } else {
                // i only need to clear if there was any data
                let result = std::slice::from_raw_parts(*self.buffer as *const u8, *self.bufsize).into();
                self.clear()?;
                Ok(result)
            }
        }
    }

    pub fn override_file(&mut self, file: *mut *mut nix::libc::FILE) -> FileGuard<'_> {
        FileGuard::new(self, file)
    }

    pub fn override_stdout(&mut self) -> FileGuard<'_> {
        self.override_file(&raw mut stdout)
    }

    pub fn override_stderr(&mut self) -> FileGuard<'_> {
        self.override_file(&raw mut stderr)
    }

    pub fn override_shout(&mut self) -> FileGuard<'_> {
        self.override_file((&raw mut zsh_sys::shout).cast())
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        unsafe {
            nix::libc::fclose(self.file.as_ptr());

            if !self.buffer.is_null() {
                nix::libc::free(self.buffer.cast());
            }
        }
    }
}

pub struct FileGuard<'a> {
    #[allow(dead_code)]
    pub inner: &'a mut Sink,
    dest: *mut *mut nix::libc::FILE,
    old_file: *mut nix::libc::FILE,
}

impl<'a> FileGuard<'a> {
    pub fn new(parent: &'a mut Sink, file: *mut *mut nix::libc::FILE) -> FileGuard<'a> {
        unsafe {
            let old_file = *file;
            *file = parent.file.as_ptr();
            FileGuard{
                inner: parent,
                dest: file,
                old_file,
            }
        }
    }
}

impl Drop for FileGuard<'_> {
    fn drop(&mut self) {
        unsafe{
            *self.dest = self.old_file;
        }
    }
}
