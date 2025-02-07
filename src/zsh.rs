use std::ffi::{CString};
use std::os::raw::{c_char, c_int, c_long};
use std::default::Default;
use std::ptr::null_mut;

mod types;
mod variables;
pub use self::variables::*;

pub type HandlerFunc = unsafe extern "C" fn(name: *mut c_char, argv: *mut *mut c_char, options: *mut zsh_sys::options, func: c_int) -> c_int;

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
    let cmd = CString::new(cmd).unwrap();
    unsafe { zsh_sys::addhashnode(zsh_sys::builtintab, cmd.into_raw(), builtin as _) };
}
