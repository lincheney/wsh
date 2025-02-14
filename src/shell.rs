use std::os::fd::{RawFd};
use std::os::raw::{c_long};
use std::default::Default;
use std::sync::Arc;
use async_std::sync::Mutex;
use async_lock::futures::Lock;

use crate::zsh;

pub struct ShellInner {
    pub closed: bool,
}

#[derive(Clone)]
pub struct Shell(pub Arc<Mutex<ShellInner>>);

impl Shell {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(ShellInner{
            closed: false,
        })))
    }

    pub fn lock(&self) -> Lock<ShellInner> {
        self.0.lock()
    }
}

pub struct CompletionStarter(Arc<std::sync::Mutex<zsh::completion::Streamer>>);

impl CompletionStarter {
    pub fn start(&self, _shell: &ShellInner) {
        zsh::completion::_get_completions(&*self.0);
    }
}

impl ShellInner {

    pub fn exec(&mut self, string: &str, _fds: Option<&[RawFd; 3]>) -> Result<(), c_long> {
        zsh::execstring(string, Default::default());
        let code = zsh::get_return_code();
        if code > 0 { Err(code) } else { Ok(()) }
    }

    pub fn eval(&mut self, string: &str, _capture_stderr: bool) -> Result<Vec<u8>, c_long> {
        zsh::execstring(string, Default::default());
        let code = zsh::get_return_code();
        if code > 0 { Err(code) } else { Ok(vec![]) }
    }

    pub fn get_completions(&self, string: &str) -> anyhow::Result<(Arc<Mutex<zsh::completion::StreamConsumer>>, CompletionStarter)> {
        let (consumer, producer) = zsh::completion::get_completions(string)?;
        Ok((consumer, CompletionStarter(producer)))
    }

    pub fn clear_completion_cache(&self) {
        zsh::completion::clear_cache()
    }

    pub fn insert_completion(&self, string: &str, m: &zsh::cmatch) -> (Vec<u8>, usize) {
        zsh::completion::insert_completion(string, m)
    }

}
