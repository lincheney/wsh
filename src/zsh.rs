use std::ffi::{CString};
use std::os::raw::*;
use std::default::Default;
use std::ptr::null_mut;
use bstr::BStr;

mod string;
mod bindings;
mod variables;
pub mod completion;
pub mod parser;
pub use variables::*;
pub use string::ZString;
pub use bindings::{cmatch};

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

pub fn execstring(cmd: &str, opts: ExecstringOpts) {
    let cmd = CString::new(cmd).unwrap();
    let context = opts.context.map(|c| CString::new(c).unwrap());
    unsafe{ zsh_sys::execstring(
        cmd.as_ptr() as _,
        opts.dont_change_job.into(),
        opts.exiting.into(),
        context.map(|c| c.as_ptr() as _).unwrap_or(null_mut()),
    )}
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

pub fn get_prompt(prompt: Option<&BStr>) -> Option<CString> {
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
    unsafe {
        let ptr = zsh_sys::promptexpand(prompt.as_ptr() as _, 0, r, R, null_mut());
        Some(CString::from_raw(ptr))
    }
}
