use std::os::raw::*;

pub struct ZString {
    ptr: *mut c_char,
}

impl ZString {
    pub fn into_raw(mut self) -> *mut c_char {
        let ptr = self.ptr;
        self.ptr = std::ptr::null_mut();
        ptr
    }
}

impl From<&[u8]> for ZString {
    fn from(s: &[u8]) -> Self {

        // TODO check for interior null byte
        // if s.iter().any(|c| c == 0) {
        // }

        let ptr = unsafe {
            let ptr = zsh_sys::zalloc(s.len() + 1) as *mut u8;
            std::ptr::copy_nonoverlapping(s.as_ptr(), ptr, s.len());
            *ptr.add(s.len()) = b'\0' as _;
            ptr
        };
        Self{ ptr: ptr as *mut _ }
    }
}

impl From<&str> for ZString {
    fn from(s: &str) -> Self {
        s.as_bytes().into()
    }
}

impl Drop for ZString {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe{ zsh_sys::zsfree(self.ptr); }
        }
    }
}
