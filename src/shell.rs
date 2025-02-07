use std::os::fd::{RawFd};
use std::os::raw::{c_long};
use std::default::Default;
use anyhow::Result;
use crate::zsh;

pub struct Shell {
    pub closed: bool,
}

impl Shell {
    pub fn new() -> Result<Self> {
        Ok(Self{
            closed: false,
        })
    }

    pub async fn exec(&mut self, string: &str, _fds: Option<&[RawFd; 3]>) -> std::result::Result<(), c_long> {
        zsh::execstring(string, Default::default());
        let code = zsh::get_return_code();
        if code > 0 { Err(code) } else { Ok(()) }
    }

    pub async fn eval(&mut self, string: &str, _capture_stderr: bool) -> std::result::Result<Vec<u8>, c_long> {
        zsh::execstring(string, Default::default());
        let code = zsh::get_return_code();
        if code > 0 { Err(code) } else { Ok(vec![]) }
    }

}
