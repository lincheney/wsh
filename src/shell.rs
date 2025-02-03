use std::ffi::{CString, CStr};
use std::os::fd::{AsRawFd, RawFd, OwnedFd};
use std::default::Default;
use std::ptr::null_mut;
use anyhow::Result;

struct ExecstringOpts<'a> {
    dont_change_job: bool,
    exiting: bool,
    context: Option<&'a str>,
}

impl Default for ExecstringOpts<'_> {
    fn default() -> Self {
        Self{ dont_change_job: true, exiting: false, context: None }
    }
}

fn execstring(cmd: &str, opts: ExecstringOpts) {
    let cmd = CString::new(cmd).unwrap();
    let context = opts.context.map(|c| CString::new(c).unwrap());
    unsafe{ zsh_sys::execstring(
        cmd.as_ptr() as *mut _,
        opts.dont_change_job.into(),
        opts.exiting.into(),
        context.map(|c| c.as_ptr() as *mut _).unwrap_or(null_mut()),
    )}
}

fn getsparam(varname: &str) -> Option<Vec<u8>> {
    let varname = CString::new(varname).unwrap();
    let str = unsafe{
        let var = zsh_sys::getsparam(varname.as_ptr() as *mut _);
        if var.is_null() {
            return None
        }
        CStr::from_ptr(var)
    };
    Some(str.to_bytes().to_owned())
}

pub struct Shell {
    pub closed: bool,
}

impl Shell {
    pub fn new() -> Result<Self> {
        Ok(Self{
            closed: false,
        })
    }

    pub async fn exec(&mut self, string: &str, fds: Option<&[RawFd; 3]>) -> Result<()> {
        execstring(string, Default::default());
        Ok(())
    }

    pub async fn eval(&mut self, string: &str, capture_stderr: bool) -> Result<Vec<u8>> {
        execstring(string, Default::default());
        Ok(vec![])
    }

    pub async fn get_var_string(&mut self, varname: &str) -> Result<Option<Vec<u8>>> {
        Ok(getsparam(varname))
    }

}
