use std::ffi::CString;
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

pub struct Function(UnsafeSend<zsh_sys::shfunc>);

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
                nam: unsafe{ crate::EMPTY_STR.as_mut_ptr() }.cast(),
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

    pub fn execute(&self, args: &[CString]) -> c_int {
        // convert args to a linked list
        const EMPTY_NODE: zsh_sys::linknode = zsh_sys::linknode{
            next: null_mut(),
            prev: null_mut(),
            dat: null_mut(),
        };

        let mut nodes = vec![EMPTY_NODE; args.len() + 1];
        // arg0
        nodes[0].dat = self.0.as_ref().node.nam.cast();
        for (arg, node) in args.iter().zip(&mut nodes[1..]) {
            node.dat = arg.as_ptr() as _;
        }
        for i in 0..nodes.len()-1 {
            nodes[i].next = &raw const nodes[i+1] as _;
            nodes[i+1].prev = &raw const nodes[i] as _;
        }

        let mut list = zsh_sys::linkroot{
            list: zsh_sys::linklist{
                first: &raw const nodes[0] as _,
                last: &raw const nodes[nodes.len()-1] as _,
                flags: 0,
            }
        };

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
