use std::os::raw::{c_char};
use std::ffi::{CString, CStr};
use std::os::raw::*;
use std::default::Default;
use std::ptr::null_mut;
use bstr::{BStr, BString};

mod string;
mod bindings;
pub mod variables;
mod widget;
pub use widget::ZleWidget;
pub mod history;
pub mod completion;
pub mod parser;
pub use string::ZString;
pub(crate) use bindings::*;
use variables::{Variable};

// pub type HandlerFunc = unsafe extern "C" fn(name: *mut c_char, argv: *mut *mut c_char, options: *mut zsh_sys::options, func: c_int) -> c_int;

#[derive(Clone, Copy)]
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

pub fn execstring<S: AsRef<BStr>>(cmd: S, opts: ExecstringOpts) -> c_long {
    let cmd = cmd.as_ref().to_vec();
    let context = opts.context.map(|c| ZString::from(c).into_raw());
    unsafe{
        zsh_sys::execstring(
            metafy(&cmd),
            opts.dont_change_job.into(),
            opts.exiting.into(),
            context.unwrap_or(null_mut()),
        );
    }
    return get_return_code()
}

pub fn get_return_code() -> c_long {
    unsafe{ zsh_sys::lastval }
}

pub fn pop_builtin(name: &str) -> Option<zsh_sys::Builtin> {
    let name = CString::new(name).unwrap();
    let ptr = unsafe { zsh_sys::removehashnode(zsh_sys::builtintab, name.as_ptr().cast()) };
    if ptr.is_null() { None } else { Some(ptr.cast()) }
}

pub fn add_builtin(cmd: &str, builtin: zsh_sys::Builtin) {
    let cmd: ZString = cmd.into();
    unsafe { zsh_sys::addhashnode(zsh_sys::builtintab, cmd.into_raw(), builtin.cast()) };
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
    let glitch = escaped.into();
    unsafe {
        let ptr = zsh_sys::promptexpand(prompt.as_ptr().cast_mut(), glitch, r, R, null_mut());
        Some(CString::from_raw(ptr))
    }
}

pub fn get_prompt_size(prompt: &CStr) -> (c_int, c_int) {
    let mut width = 0;
    let mut height = 0;
    let overflow = 0;
    unsafe {
        zsh_sys::countprompt(prompt.as_ptr().cast_mut(), &raw mut width, &raw mut height, overflow);
    }
    (width, height)
}

pub fn metafy(value: &[u8]) -> *mut c_char {
    unsafe {
        if value.is_empty() {
            // make an empty string on the arena
            let ptr = zsh_sys::zhalloc(1).cast();
            *ptr = 0;
            ptr
        } else {
            // metafy will ALWAYS write a terminating null no matter what
            zsh_sys::metafy(value.as_ptr() as _, value.len() as _, zsh_sys::META_HEAPDUP as _)
        }
    }
}

pub fn unmetafy<'a>(ptr: *mut u8) -> &'a [u8] {
    // threadsafe!
    let mut len = 0i32;
    unsafe {
        zsh_sys::unmetafy(ptr.cast(), &raw mut len);
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
        zsh_sys::unmetafy(value.as_mut_ptr().cast(), &raw mut len);
    }
    value.truncate(len as _);
}

pub fn start_zle_scope() {
    unsafe {
        zsh_sys::startparamscope();
        bindings::makezleparams(0);
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

pub enum ErrorVerbosity {
    Normal = 0,
    Quiet = 1,
    Ignore = 2,
}

pub fn set_error_verbosity(verbosity: ErrorVerbosity) -> ErrorVerbosity {
    unsafe {
        let old_value = zsh_sys::noerrs;
        zsh_sys::noerrs = verbosity as _;
        if old_value <= 0 {
            ErrorVerbosity::Normal
        } else if old_value >= 2 {
            ErrorVerbosity::Ignore
        } else {
            ErrorVerbosity::Quiet
        }
    }
}
