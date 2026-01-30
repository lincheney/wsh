use std::os::raw::c_char;
use std::borrow::{Cow, Borrow};
use std::ffi::{CStr, CString};
use bstr::{BStr, BString, ByteVec};
pub mod array;

#[macro_export]
macro_rules! meta_str {
    ($arg:literal) => {
        crate::shell::MetaStr::new($arg)
    }
}

fn imeta(c: u8) -> bool {
    super::zistype(c as _, zsh_sys::IMETA as _)
}

fn metafied_len(string: &[u8]) -> usize {
    string.len() + string.iter().filter(|&&c| imeta(c)).count()
}

// fn needs_meta(string: &BStr) -> bool {
    // string.iter().any(|&c| imeta(c))
// }

const fn is_trivially_meta(string: &[u8]) -> bool {
    // basic meta check
    let mut i = 0;
    while i < string.len() {
        // anything not ascii or null needs meta
        if !(0 < string[i] && string[i] < 128) {
            return false
        }
        i += 1;
    }
    true
}

pub fn unmetafy(bytes: &BStr) -> Cow<'_, BStr> {
    if is_trivially_meta(bytes) {
        Cow::Borrowed(bytes)
    } else {
        Cow::Owned(MetaString::from(bytes.to_owned()).unmetafy())
    }
}

#[derive(Clone, Debug)]
pub struct MetaString {
    inner: CString,
}
crate::impl_deref_helper!(self: MetaString, &self.inner => CString);

impl MetaString {

    pub fn into_inner(self) -> CString {
        self.inner
    }

    pub fn into_raw(self) -> *mut c_char {
        self.inner.into_raw()
    }

    pub unsafe fn from_raw(ptr: *mut c_char) -> Self {
        Self{ inner: unsafe{ CString::from_raw(ptr) } }
    }

    pub fn modify<F: Fn(&mut Vec<u8>)>(&mut self, callback: F) {
        let mut buf = std::mem::take(&mut self.inner).into_bytes();
        callback(&mut buf);
        self.inner = CString::new(buf).unwrap();
    }

    pub fn push_str(&mut self, str: &MetaStr) {
        self.modify(|buf| buf.push_str(str.to_bytes()));
    }

    pub fn insert_str(&mut self, pos: usize, str: &MetaStr) {
        self.modify(|buf| buf.insert_str(pos, str.to_bytes()));
    }

    pub fn unmetafy(self) -> BString {
        let mut len = 0i32;
        let mut bytes: BString = self.inner.into_bytes_with_nul().into();
        unsafe {
            // threadsafe!
            zsh_sys::unmetafy(bytes.as_mut_ptr().cast(), &raw mut len);
        }
        bytes.truncate(len as _);
        bytes
    }

}

impl From<Vec<u8>> for MetaString {
    fn from(mut val: Vec<u8>) -> MetaString {
        let old_len = val.len();
        let new_len = metafied_len(val.as_ref());
        // extra 1 for the terminating null byte
        val.resize(new_len + 1, 0);
        // metafy it only if necessary
        if new_len != old_len {
            unsafe {
                // tell zsh to reuse the same buffer
                let ptr = val.as_mut_ptr().cast();
                // since we have already allocated memory,
                // this should be safe to use outside the main thread
                let ret = zsh_sys::metafy(ptr, val.len() as _, zsh_sys::META_NOALLOC as _);
                debug_assert_eq!(ret, ptr);
            }
        }
        let inner = CString::from_vec_with_nul(val).unwrap();
        Self { inner }
    }
}

impl From<CString> for MetaString {
    fn from(val: CString) -> MetaString {
        let val: Vec<u8> = val.into_bytes();
        val.into()
    }
}

impl From<&CStr> for MetaString {
    fn from(val: &CStr) -> MetaString {
        val.to_owned().into()
    }
}

impl From<BString> for MetaString {
    fn from(val: BString) -> MetaString {
        let val: Vec<u8> = val.into();
        val.into()
    }
}

impl From<String> for MetaString {
    fn from(val: String) -> MetaString {
        let val: Vec<u8> = val.into();
        val.into()
    }
}

impl AsRef<MetaStr> for MetaString {
    fn as_ref(&self) -> &MetaStr {
        unsafe{ MetaStr::new_unchecked(self.inner.as_ref()) }
    }
}

impl Borrow<MetaStr> for MetaString {
    fn borrow(&self) -> &MetaStr {
        self.as_ref()
    }
}

#[derive(PartialEq, Debug)]
#[repr(transparent)]
pub struct MetaStr {
    inner: CStr,
}
crate::impl_deref_helper!(self: MetaStr, &self.inner => CStr);

impl MetaStr {

    pub const unsafe fn new_unchecked(inner: &CStr) -> &Self {
        unsafe{ &*(inner as *const _ as *const Self) }
    }

    pub const fn new(inner: &CStr) -> &Self {
        match Self::try_new(inner) {
            Ok(x) => x,
            Err(e) => panic!("{}", e),
        }
    }

    pub const fn try_new(inner: &CStr) -> Result<&Self, &'static str> {
        if is_trivially_meta(inner.to_bytes()) {
            Ok(unsafe{ Self::new_unchecked(inner) })
        } else {
            Err("string is not metafied")
        }
    }

    pub const fn from_bytes(bytes: &[u8]) -> &Self {
        match Self::try_from_bytes(bytes) {
            Ok(x) => x,
            Err(e) => panic!("{}", e),
        }
    }

    pub const fn try_from_bytes(bytes: &[u8]) -> Result<&Self, &'static str> {
        match CStr::from_bytes_with_nul(bytes) {
            Ok(x) => Self::try_new(x),
            Err(_) => Err("bytes does not end with null"),
        }
    }

    pub unsafe fn from_ptr<'a>(ptr: *const c_char) -> &'a Self {
        unsafe {
            let str = CStr::from_ptr(ptr);
            &*(str as *const _ as *const Self)
        }
    }

    pub fn unmetafy(&self) -> Cow<'_, BStr> {
        if is_trivially_meta(self.inner.to_bytes()) {
            Cow::Borrowed(self.inner.to_bytes().into())
        } else {
            Cow::Owned(self.to_owned().unmetafy())
        }
    }
}

impl ToOwned for MetaStr {
    type Owned = MetaString;
    fn to_owned(&self) -> Self::Owned {
        MetaString{ inner: self.inner.to_owned() }
    }
}
