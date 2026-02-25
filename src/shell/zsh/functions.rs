use super::{MetaStr, MetaString};
use std::os::raw::{c_int};
use bstr::{BString};
use std::ptr::null_mut;
use anyhow::Result;
use crate::unsafe_send::UnsafeSend;
use std::sync::{LazyLock};
use crate::meta_str;

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
    pub fn new(code: &MetaStr) -> Result<Self> {
        let lineno = 1;
        // prog is allocated on the zsh heap, so we dont need to free it
        // but we DO need to dup it
        let prog = unsafe{ zsh_sys::parse_string(code.as_ptr().cast_mut(), lineno) };

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

    fn doshfunc<'a, I: Iterator<Item=&'a MetaStr>>(func: zsh_sys::Shfunc, arg0: &'a MetaStr, args: I) -> c_int {
        let args = std::iter::once(arg0).chain(args);
        let args = args.map(|x| x.as_ptr());
        // convert args to a linked list
        let args = super::linked_list::LinkedList::new_from_ptrs(args);

        let mut list = args.as_linkroot();
        let noreturnval = 1;
        unsafe {
            zsh_sys::doshfunc(func, &raw mut list, noreturnval)
        }
    }

    pub fn execute<'a, I: Iterator<Item=&'a MetaStr>>(&self, arg0: Option<&'a MetaStr>, args: I) -> c_int {
        Self::doshfunc(self.0.as_ref() as *const _ as _, arg0.unwrap_or(meta_str!(c"")), args)
    }

    pub fn execute_by_name<'a, I: Iterator<Item=&'a MetaStr>>(name: &'a MetaStr, args: I) -> Option<c_int> {
        let func = unsafe{ zsh_sys::getshfunc(name.as_ptr().cast_mut()) };
        if func.is_null() {
            return None
        }
        Some(Self::doshfunc(func, name, args))
    }

    pub fn get_source(&self) -> BString {
        unsafe {
            let ptr = zsh_sys::getpermtext(self.0.as_ref().funcdef, null_mut(), 1);
            MetaString::from_raw(ptr).unmetafy()
        }
    }
}

impl Drop for Function {
    fn drop(&mut self) {
        unsafe {
            zsh_sys::freeeprog(self.0.into_inner().funcdef);
        }
    }
}
