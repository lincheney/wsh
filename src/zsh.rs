use std::ffi::{CString};
use std::default::Default;
use std::ptr::null_mut;

mod variables;
pub use self::variables::*;

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
        cmd.as_ptr() as *mut _,
        opts.dont_change_job.into(),
        opts.exiting.into(),
        context.map(|c| c.as_ptr() as *mut _).unwrap_or(null_mut()),
    )}
}
