use std::ffi::{CString, CStr};
use std::os::raw::*;
use std::default::Default;
use std::ptr::null_mut;
use bstr::{BStr, BString};

mod string;
mod bindings;
mod variables;
pub mod history;
pub mod completion;
pub mod parser;
pub use variables::*;
pub use string::ZString;
pub use bindings::{cmatch, Inpar, Outpar, Meta, expandhistory, selectkeymap, initundo};

// pub type HandlerFunc = unsafe extern "C" fn(name: *mut c_char, argv: *mut *mut c_char, options: *mut zsh_sys::options, func: c_int) -> c_int;

pub struct ExecstringOpts<'a> {
    dont_change_job: bool,
    exiting: bool,
    context: Option<&'a str>,
}

impl Default for ExecstringOpts<'_> {
    fn default() -> Self {
        Self{ dont_change_job: true, exiting: false, context: None }
    }
}

pub fn execstring<S: AsRef<BStr>>(cmd: S, opts: ExecstringOpts) {
    let cmd = cmd.as_ref().to_vec();
    let context = opts.context.map(|c| CString::new(c).unwrap());
    unsafe{
        zsh_sys::execstring(
            metafy(&cmd),
            opts.dont_change_job.into(),
            opts.exiting.into(),
            context.map(|c| c.as_ptr() as _).unwrap_or(null_mut()),
        )
    }
}

pub fn get_return_code() -> c_long {
    unsafe{ zsh_sys::lastval }
}

pub fn pop_builtin(name: &str) -> Option<zsh_sys::Builtin> {
    let name = CString::new(name).unwrap();
    let ptr = unsafe { zsh_sys::removehashnode(zsh_sys::builtintab, name.as_ptr() as _) };
    if ptr.is_null() { None } else { Some(ptr as _) }
}

pub fn add_builtin(cmd: &str, builtin: zsh_sys::Builtin) {
    let cmd: ZString = cmd.into();
    unsafe { zsh_sys::addhashnode(zsh_sys::builtintab, cmd.into_raw(), builtin as _) };
}

pub(crate) fn iter_linked_list(list: zsh_sys::LinkList) -> impl Iterator<Item=*mut c_void> {
    unsafe {
        let mut node = list.as_mut().and_then(|list| list.list.first.as_mut());
        std::iter::from_fn(move || {
            let n = node.take()?;
            node = n.next.as_mut();
            Some(n.dat)
        })
    }
}

pub fn get_prompt(prompt: Option<&BStr>, escaped: bool) -> Option<CString> {
    let prompt = if let Some(prompt) = prompt {
        CString::new(prompt.to_vec()).unwrap()
    } else {
        let prompt = variables::Variable::get("PROMPT")?.as_bytes();
        CString::new(prompt).unwrap()
    };

    // The prompt used for spelling correction.  The sequence `%R' expands to the string which presumably needs  spelling  correction,  and
    // `%r' expands to the proposed correction.  All other prompt escapes are also allowed.
    let r = null_mut();
    #[allow(non_snake_case)]
    let R = null_mut();
    let glitch = if escaped { 1 } else { 0 };
    unsafe {
        let ptr = zsh_sys::promptexpand(prompt.as_ptr() as _, glitch, r, R, null_mut());
        Some(CString::from_raw(ptr))
    }
}

pub fn get_prompt_size(prompt: &CStr) -> (c_int, c_int) {
    let mut width = 0;
    let mut height = 0;
    let overflow = 0;
    unsafe {
        zsh_sys::countprompt(prompt.as_ptr() as _, &mut width as _, &mut height as _, overflow);
    }
    (width, height)
}

pub fn metafy(value: &[u8]) -> *mut c_char {
    unsafe {
        if value.is_empty() {
            // make an empty string on the arena
            let ptr = zsh_sys::zhalloc(1) as *mut c_char;
            *ptr = 0;
            ptr
        } else {
            zsh_sys::metafy(value.as_ptr() as _, value.len() as _, zsh_sys::META_USEHEAP as _)
        }
    }
}

pub fn unmetafy<'a>(ptr: *mut u8) -> &'a [u8] {
    // threadsafe!
    let mut len = 0i32;
    unsafe {
        zsh_sys::unmetafy(ptr as _, &mut len as _);
        std::slice::from_raw_parts(ptr, len as _)
    }
}

pub fn unmetafy_owned(value: &mut Vec<u8>) {
    // threadsafe!
    let mut len = 0i32;
    // MUST end with null byte
    if value.last().is_none_or(|c| *c != 0) {
        value.push(0);
    }
    unsafe {
        zsh_sys::unmetafy(value.as_mut_ptr() as _, &mut len as _);
    }
    value.truncate(len as _);
}

pub fn start_zle_scope() {
    unsafe {
        zsh_sys::startparamscope();
        crate::zsh::bindings::makezleparams(0);
        zsh_sys::startparamscope();
    }
}

pub fn end_zle_scope() {
    unsafe {
        zsh_sys::endparamscope();
        zsh_sys::endparamscope();
    }
}

pub fn set_zle_buffer(buffer: BString, cursor: i64) {
    start_zle_scope();
    Variable::set(b"BUFFER", buffer.into(), true).unwrap();
    Variable::set(b"CURSOR", cursor.into(), true).unwrap();
    end_zle_scope();
}

pub fn get_zle_buffer() -> (BString, Option<i64>) {
    start_zle_scope();
    let buffer = Variable::get("BUFFER").unwrap().as_bytes();
    let cursor = Variable::get("CURSOR").unwrap().try_as_int();
    end_zle_scope();
    match cursor {
        Ok(Some(cursor)) => (buffer, Some(cursor)),
        _ => (buffer, None),
    }
}

pub fn get_cwd() -> BString {
    unsafe {
        let ptr = zsh_sys::zgetcwd();
        CStr::from_ptr(ptr).to_bytes().into()
    }
}
