use std::ptr::NonNull;
use std::ffi::{CStr};

pub struct Builtin {
    inner: NonNull<zsh_sys::builtin>,
}

impl Builtin {
    pub fn pop(name: &CStr) -> Option<Builtin> {
        let ptr = unsafe { zsh_sys::removehashnode(zsh_sys::builtintab, name.as_ptr()) };
        NonNull::new(ptr.cast()).map(|inner| Self{ inner })
    }

    pub fn add(self) {
        unsafe {
            let name = self.inner.as_ref().node.nam;
            zsh_sys::addhashnode(zsh_sys::builtintab, name, self.inner.as_ptr().cast());
        }
    }
}
crate::impl_deref_helper!(self: Builtin, unsafe{ self.inner.as_ref() } => zsh_sys::builtin);
crate::impl_deref_helper!(mut self: Builtin, unsafe{ self.inner.as_mut() } => zsh_sys::builtin);

impl Clone for Builtin {
    fn clone(&self) -> Self {
        unsafe {
            let mut inner = *self.inner.as_ref();
            // the name and optstr need to be cloned as well
            inner.node.nam = zsh_sys::ztrdup(inner.node.nam);
            inner.optstr = zsh_sys::ztrdup(inner.optstr);
            let inner = NonNull::new(Box::into_raw(Box::new(inner))).unwrap();
            Self{ inner }
        }
    }
}

// impl Drop for Builtin {
    // fn drop(&mut self) {
        // unimplemented!()
    // }
// }
