use std::os::fd::{RawFd};
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

    pub async fn exec(&mut self, string: &str, fds: Option<&[RawFd; 3]>) -> Result<()> {
        zsh::execstring(string, Default::default());
        Ok(())
    }

    pub async fn eval(&mut self, string: &str, capture_stderr: bool) -> Result<Vec<u8>> {
        zsh::execstring(string, Default::default());
        Ok(vec![])
    }

}
