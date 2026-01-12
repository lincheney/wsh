use std::ffi::{CString};
use std::os::raw::{c_int};
use bstr::{BString, BStr};
use std::ptr::null_mut;
use anyhow::Result;
use crate::unsafe_send::UnsafeSend;
use std::sync::{LazyLock};

pub static FUNCTIONS: LazyLock<UnsafeSend<zsh_sys::HashTable>> = LazyLock::new(|| {
    unsafe {
        let old_shfunctab = zsh_sys::shfunctab;
        zsh_sys::createshfunctable();
        let new_shfunctab = zsh_sys::shfunctab;
        zsh_sys::shfunctab = old_shfunctab;
        UnsafeSend::new(new_shfunctab)
    }
});

pub struct Function(pub(super) UnsafeSend<zsh_sys::shfunc>);

impl Function {
    pub fn new(code: &BStr) -> Result<Self> {
        let code = super::metafy(code.into());
        let lineno = 1;
        // prog is allocated on the zsh heap, so we dont need to free it
        // but we DO need to dup it
        let prog = unsafe{ zsh_sys::parse_string(code, lineno) };

        if prog.is_null() || prog == &raw mut zsh_sys::dummy_eprog {
            anyhow::bail!("invalid function definition: {code:?}");
        }

        let heap = 0;
        let prog = unsafe{ zsh_sys::dupeprog(prog, heap) };

        let mut func = zsh_sys::shfunc {
            node: zsh_sys::hashnode{
                next: null_mut(),
                #[allow(static_mut_refs)]
                nam: crate::EMPTY_STR.as_ptr().cast_mut(),
                flags: 0,
            },
            filename: null_mut(),
            lineno: 0,
            funcdef: prog,
            redir: null_mut(),
            sticky: null_mut(),
        };
        unsafe{ zsh_sys::shfunc_set_sticky(&raw mut func); }

        Ok(Self(unsafe{ UnsafeSend::new(func) }))
    }

    pub fn execute<'a, I: Iterator<Item=&'a BStr>>(&self, arg0: Option<&'a BStr>, args: I) -> c_int {
        let args = arg0.or(Some(b"".into())).into_iter()
            .chain(args)
            .map(|x| super::metafy(x).cast_const());

        // convert args to a linked list
        let args = super::linked_list::LinkedList::new_from_ptrs(args);

        let mut list = args.as_linkroot();
        let noreturnval = 1;
        let shfunc = self.0.as_ref();
        unsafe {
            zsh_sys::doshfunc(shfunc as *const _ as _, &raw mut list, noreturnval)
        }
    }

    pub fn get_source(&self) -> BString {
        let source = unsafe {
            CString::from_raw(zsh_sys::getpermtext(self.0.as_ref().funcdef, null_mut(), 1))
        };
        source.into_bytes().into()
    }
}

impl Drop for Function {
    fn drop(&mut self) {
        unsafe {
            zsh_sys::freeeprog(self.0.into_inner().funcdef);
        }
    }
}
