use std::ffi::{CStr};
use std::os::raw::{c_char};
use bstr::BString;

pub struct CStringArray {
    pub ptr: *mut *mut c_char,
}

impl CStringArray {
    pub fn iter_ptr(&self) -> impl Iterator<Item=*mut c_char> {
        let mut ptr = self.ptr;
        std::iter::from_fn(move || {
            if ptr.is_null() {
                return None
            }

            let value = unsafe{ *ptr };
            if value.is_null() {
                None
            } else {
                ptr = unsafe{ ptr.offset(1) };
                Some(value)
            }
        })
    }

    pub fn iter(&self) -> impl Iterator<Item=&CStr> {
        self.iter_ptr().map(|ptr| unsafe{ CStr::from_ptr(ptr) })
    }

    pub fn to_vec(&self) -> Vec<BString> {
        self.iter().map(|s| s.to_bytes().to_owned()).map(BString::new).collect()
    }

    pub fn into_ptr(self) -> *mut *mut c_char {
        self.ptr
    }
}

impl Drop for CStringArray {
    fn drop(&mut self) {
        let mut len = 0;
        unsafe{
            for ptr in self.iter_ptr() {
                zsh_sys::zsfree(ptr);
                len += 1;
            }
            if !self.ptr.is_null() {
                zsh_sys::zfree(self.ptr as _, len);
            }
        }
    }
}

impl From<*mut *mut c_char> for CStringArray {
    fn from(ptr: *mut *mut c_char) -> Self {
        Self{ ptr }
    }
}

impl From<Vec<BString>> for CStringArray {
    fn from(vec: Vec<BString>) -> Self {
        unsafe {
            let ptr: *mut *mut c_char = zsh_sys::zalloc(std::mem::size_of::<*mut c_char>() * (vec.len() + 1)) as _;
            for (i, string) in vec.iter().enumerate() {
                *ptr.add(i) = zsh_sys::ztrduppfx(string.as_ptr() as _, string.len() as _);
            }
            *ptr.add(vec.len()) = std::ptr::null_mut();
            Self{ ptr }
        }
    }
}
