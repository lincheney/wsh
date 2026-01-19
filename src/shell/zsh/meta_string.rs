use std::os::raw::c_char;
use std::ffi::{CStr, CString};
use bstr::{BStr, BString, ByteVec};

fn imeta(c: u8) -> bool {
    super::zistype(c as _, zsh_sys::IMETA as _)
}

fn metafied_len(string: &[u8]) -> usize {
    // unsafe {
        // zsh_sys::metalen(string.as_ptr().cast(), string.len() as _) as _
    // }
    string.len() + string.iter().filter(|&&c| imeta(c)).count()
}

fn needs_meta(string: &BStr) -> bool {
    string.iter().any(|&c| imeta(c))
}

const fn is_trivially_meta(string: &[u8]) -> bool {
    // basic meta check
    let mut i = 0;
    while i < string.len() {
        // anything ascii and not null does not need meta
        if !(0 < string[i] && string[i] < 128) {
            return false
        }
        i += 1;
    }
    true
}

#[derive(Clone, Debug)]
pub struct MetaString {
    inner: CString,
}
crate::impl_deref_helper!(self: MetaString, &self.inner => CString);

impl MetaString {

    pub fn from_vec(mut val: Vec<u8>) -> Self {
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
        let inner = CString::from_vec_with_nul(val.into()).unwrap();
        Self { inner }
    }

    pub fn as_str(&self) -> MetaStr<'_> {
        MetaStr{ inner: self.inner.as_ref() }
    }

    pub fn into_inner(self) -> CString {
        self.inner
    }

    pub fn into_raw(self) -> *mut c_char {
        self.inner.into_raw()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.inner.into_bytes()
    }

    pub fn push_str(&mut self, str: MetaStr<'_>) {
        let mut buf = std::mem::take(&mut self.inner).into_bytes();
        buf.push_str(str.to_bytes());
        self.inner = CString::new(buf).unwrap();
    }

    pub fn unmetafy(self) -> BString {
        // threadsafe!
        let mut len = 0i32;
        let mut bytes: BString = self.inner.into_bytes_with_nul().into();
        unsafe {
            zsh_sys::unmetafy(bytes.as_mut_ptr().cast(), &raw mut len);
        }
        bytes.truncate(len as _);
        bytes
    }

}

impl From<Vec<u8>> for MetaString {
    fn from(val: Vec<u8>) -> MetaString {
        Self::from_vec(val)
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

pub struct MetaStr<'a> {
    inner: &'a CStr,
}
crate::impl_deref_helper!(self: MetaStr<'a>, &self.inner => &'a CStr);

impl<'a> MetaStr<'a> {
    pub const fn new(inner: &'a CStr) -> Self {
        match Self::try_new(inner) {
            Ok(x) => x,
            Err(e) => panic!("{}", e),
        }
    }

    pub const fn try_new(inner: &'a CStr) -> Result<Self, &'static str> {
        if is_trivially_meta(inner.to_bytes()) {
            Ok(Self{ inner })
        } else {
            Err("string is not metafied")
        }
    }

    pub const fn from_bytes(bytes: &'a [u8]) -> Self {
        match Self::try_from_bytes(bytes) {
            Ok(x) => x,
            Err(e) => panic!("{}", e),
        }
    }

    pub const fn try_from_bytes(bytes: &'a [u8]) -> Result<Self, &'static str> {
        match CStr::from_bytes_with_nul(bytes) {
            Ok(x) => Self::try_new(x),
            Err(_) => Err("bytes does not end with null"),
        }
    }

    pub unsafe fn from_ptr(ptr: *mut c_char) -> Self {
        Self{ inner: unsafe{ CStr::from_ptr(ptr) } }
    }

    pub fn to_string(&self) -> MetaString {
        MetaString{ inner: self.inner.to_owned() }
    }
}
