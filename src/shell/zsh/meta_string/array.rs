use super::{MetaStr, MetaString};
use std::ptr::null_mut;
use std::os::raw::{c_char};

pub struct MetaArray {
    inner: Box<[*mut c_char]>,
}

impl MetaArray {
    pub unsafe fn iter_ptr<'a>(ptr: *const *const c_char) -> impl Iterator<Item=&'a MetaStr> {
        (0..)
            .map(move |i| unsafe{ ptr.add(i) })
            .take_while(|&x| !x.is_null() && !unsafe{ *x }.is_null())
            .map(|x| unsafe{ MetaStr::from_ptr(*x) })
    }

    pub fn as_ptr(&self) -> *const *mut c_char {
        if self.inner.is_empty() {
            #[allow(static_mut_refs)]
            unsafe { super::super::zlenoargs.as_ptr() }
        } else {
            self.inner.as_ptr()
        }
    }

    pub fn into_raw(self) -> *mut *mut c_char {
        if self.inner.is_empty() {
            self.as_ptr().cast_mut()
        } else {
            Box::leak(self.inner) as *mut _ as _
        }
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
