use super::{MetaStr, MetaString};
use std::ptr::null_mut;
use std::os::raw::{c_char};

pub struct MetaArray {
    inner: Box<[*mut c_char]>,
}

impl MetaArray {
    pub fn as_ptr(&self) -> *const *mut c_char {
        if self.inner.is_empty() {
            #[allow(static_mut_refs)]
            unsafe { super::super::zlenoargs.as_ptr() }
        } else {
            self.inner.as_ptr()
        }
    }

    fn take_inner(&mut self) -> Box<[*mut c_char]> {
        std::mem::replace(&mut self.inner, Box::new([]))
    }

    pub unsafe fn from_raw(ptr: *mut *mut c_char) -> Self {
        unsafe {
            let len = MetaSlice::iter_ptr(ptr.cast()).count();
            let ptr = std::ptr::slice_from_raw_parts_mut(ptr, len);
            Self{ inner: Box::from_raw(ptr) }
        }
    }

    pub fn into_raw(mut self) -> *mut *mut c_char {
        if self.inner.is_empty() {
            self.as_ptr().cast_mut()
        } else {
            Box::leak(self.take_inner()) as *mut _ as _
        }
    }

    pub fn into_iter(mut self) -> impl Iterator<Item=MetaString> {
        self.take_inner()
            .into_iter()
            .filter(|x| !x.is_null())
            .map(|x| unsafe{ MetaString::from_raw(x) })
    }
}

impl FromIterator<MetaString> for MetaArray {
    fn from_iter<I: IntoIterator<Item=MetaString>>(iter: I) -> Self {
        let mut inner: Vec<*mut c_char> = iter.into_iter().map(|x| x.into_raw()).collect();
        if !inner.is_empty() {
            inner.push(null_mut());
        }
        Self{ inner: inner.into_boxed_slice() }
    }
}

impl Drop for MetaArray {
    fn drop(&mut self) {
        // free the pointers
        for ptr in self.take_inner() {
            if !ptr.is_null() {
                unsafe{ MetaString::from_raw(ptr) };
            }
        }
    }
}

pub struct MetaSlice<'a> {
    inner: &'a [*mut c_char],
}

impl MetaSlice<'_> {
    pub unsafe fn iter_ptr<'a>(ptr: *const *const c_char) -> impl Iterator<Item=&'a MetaStr> {
        (0..)
            .map(move |i| unsafe{ ptr.add(i) })
            .take_while(|&x| !x.is_null() && !unsafe{ *x }.is_null())
            .map(|x| unsafe{ MetaStr::from_ptr(*x) })
    }
}
