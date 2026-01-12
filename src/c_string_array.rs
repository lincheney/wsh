use std::ffi::{CStr, CString};
use std::ptr::null_mut;
use std::os::raw::{c_char};

fn iter_until_null(ptr: *mut *mut c_char) -> impl Iterator<Item=*mut *mut c_char> {
    (0..)
        .map(move |i| unsafe{ ptr.add(i) })
        .take_while(|&x| !x.is_null() && !unsafe{ *x }.is_null())
}

pub struct CStringArray {
    inner: *mut *mut c_char,
    start: *mut *mut c_char,
}

impl CStringArray {
    pub unsafe fn from_raw(ptr: *mut *mut c_char) -> Self {
        Self{ inner: ptr, start: ptr }
    }

    pub fn as_ptr(&self) -> *mut *mut c_char {
        self.inner
    }

    pub fn into_raw(self) -> *mut *mut c_char {
        let ptr = self.as_ptr();
        std::mem::forget(self);
        ptr
    }

    pub fn into_iter(mut self) -> impl Iterator<Item=CString> {
        iter_until_null(self.inner)
            .map(move |ptr| unsafe {
                // keep self alive while we have this iter
                let x = &mut self;
                let s = CString::from_raw((*ptr).cast());
                x.start = ptr.add(1);
                *ptr = null_mut();
                s
            })
    }
}

impl Drop for CStringArray {
    fn drop(&mut self) {
        unsafe {
            for ptr in iter_until_null(self.start) {
                drop(CString::from_raw(*ptr));
            }
            drop(Box::from_raw(self.inner));
        }
    }
}

impl FromIterator<CString> for CStringArray {
    fn from_iter<I: IntoIterator<Item=CString>>(iter: I) -> Self {
        let mut vec: Vec<*mut c_char> = iter.into_iter().map(|x| x.into_raw()).collect();
        vec.push(null_mut());
        unsafe{ Self::from_raw(Box::leak(vec.into_boxed_slice()) as *mut _ as _) }
    }
}

pub struct CStrArray {
    inner: *const *const c_char,
}

impl CStrArray {
    pub unsafe fn from_raw(ptr: *const *const c_char) -> Self {
        Self{ inner: ptr }
    }

    pub fn iter(&self) -> impl Iterator<Item=&CStr> {
        iter_until_null(self.inner as _).map(|x| unsafe{ CStr::from_ptr((*x).cast_const()) })
    }
}

impl std::fmt::Debug for CStrArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_list().entries(self.iter()).finish()
    }
}
