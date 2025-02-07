use std::os::fd::{RawFd};
use std::os::raw::{c_long};
use std::default::Default;
use crate::zsh;

pub struct Shell {
    pub closed: bool,
}

impl Shell {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self{
            closed: false,
        })
    }

    pub async fn exec(&mut self, string: &str, _fds: Option<&[RawFd; 3]>) -> Result<(), c_long> {
        zsh::execstring(string, Default::default());
        let code = zsh::get_return_code();
        if code > 0 { Err(code) } else { Ok(()) }
    }

    pub async fn eval(&mut self, string: &str, _capture_stderr: bool) -> Result<Vec<u8>, c_long> {
        zsh::execstring(string, Default::default());
        let code = zsh::get_return_code();
        if code > 0 { Err(code) } else { Ok(vec![]) }
    }

    pub async fn get_completions(&self, string: &str) -> anyhow::Result<()> {
        zsh::completion::get_completions(string);
        Ok(())
    }

}
