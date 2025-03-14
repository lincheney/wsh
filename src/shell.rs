use std::os::raw::{c_long};
use std::ffi::{CString, CStr};
use std::default::Default;
use std::sync::Arc;
use std::ptr::null_mut;
use tokio::sync::{Mutex, MutexGuard};
use bstr::{BStr, BString};

use crate::zsh;

pub struct ShellInner {
    // pub closed: bool,
}

#[derive(Clone)]
pub struct Shell(pub Arc<Mutex<ShellInner>>);

impl Shell {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(ShellInner{
            // closed: false,
        })))
    }

    pub async fn lock(&self) -> MutexGuard<ShellInner> {
        self.0.lock().await
    }
}

pub struct CompletionStarter(Arc<std::sync::Mutex<zsh::completion::Streamer>>);

impl CompletionStarter {
    pub fn start(&self, _shell: &ShellInner) {
        zsh::completion::_get_completions(&self.0);
    }
}

impl ShellInner {

    pub fn exec(&mut self, string: &BStr) -> Result<(), c_long> {
        zsh::execstring(string, Default::default());
        let code = zsh::get_return_code();
        if code > 0 { Err(code) } else { Ok(()) }
    }

    pub fn eval(&mut self, string: &BStr, _capture_stderr: bool) -> Result<BString, c_long> {
        zsh::execstring(string, Default::default());
        let code = zsh::get_return_code();
        if code > 0 { Err(code) } else { Ok(BString::new(vec![])) }
    }

    pub fn get_completions(&self, string: &BStr) -> anyhow::Result<(Arc<Mutex<zsh::completion::StreamConsumer>>, CompletionStarter)> {
        let (consumer, producer) = zsh::completion::get_completions(string)?;
        Ok((consumer, CompletionStarter(producer)))
    }

    pub fn clear_completion_cache(&self) {
        zsh::completion::clear_cache()
    }

    pub fn insert_completion(&self, string: &BStr, m: &zsh::cmatch) -> (BString, usize) {
        zsh::completion::insert_completion(string, m)
    }

    pub fn parse(&mut self, string: &BStr, recursive: bool) -> (bool, Vec<zsh::parser::Token>) {
        zsh::parser::parse(string, recursive)
    }

    pub fn get_prompt(&mut self, prompt: Option<&str>, escaped: bool) -> Option<CString> {
        zsh::get_prompt(prompt.map(|p| p.into()), escaped)
    }

    pub fn get_prompt_size(&mut self, prompt: &CStr) -> (usize, usize) {
        let (width, height) = zsh::get_prompt_size(prompt);
        (width as _, height as _)
    }

    pub fn remove_invisible_chars(string: &CStr) -> std::borrow::Cow<CStr> {
        let bytes = string.to_bytes();
        if bytes.contains(&(zsh::Inpar as _)) || bytes.contains(&(zsh::Outpar as _)) || bytes.contains(&(zsh::Meta as _)) {
            let mut bytes = bytes.to_owned();
            bytes.retain(|c| *c != zsh::Inpar as _ && *c != zsh::Outpar as _);
            let bytes = CString::new(bytes).unwrap();
            zsh::unmetafy(bytes.as_ptr() as _);
            std::borrow::Cow::Owned(bytes)
        } else {
            std::borrow::Cow::Borrowed(string)
        }
    }

    pub fn readhistfile(&mut self) {
        unsafe{ zsh_sys::readhistfile(null_mut(), 0, zsh_sys::HFILE_USE_OPTIONS as _); }
    }

    pub fn get_history(&mut self) -> zsh::history::EntryIter {
        zsh::history::get_history()
    }

    pub fn get_curhist(&mut self) -> (c_long, Option<&zsh_sys::histent>) {
        let curhist = unsafe{ zsh_sys::curhist };
        self.set_curhist(curhist)
    }

    pub fn set_curhist(&mut self, curhist: c_long) -> (c_long, Option<&zsh_sys::histent>) {
        let history = self.get_history();

        let value = history
            .enumerate()
            .take_while(|(h, _)| *h >= curhist)
            .last();

        if let Some((h, e)) = value {
            // found a good enough match
            unsafe{ zsh_sys::curhist = h; }
            (h, Some(e))

        } else if let Some(h) = history.iter().next().map(|h| h.histnum) {
            // after all history
            unsafe{ zsh_sys::curhist = h + 1; }
            (h + 1, None)

        } else {
            // no history
            unsafe{ zsh_sys::curhist = 0; }
            (0, None)
        }
    }

    pub fn push_history(&mut self, string: &BStr) -> zsh::history::EntryIter {
        zsh::history::push_history(string)
    }

    pub fn add_pid(&mut self, pid: i32) {
        unsafe{
            let aux = 1;
            let bgtime = null_mut(); // this can be NULL if aux is 1
            zsh_sys::addproc(pid, null_mut(), aux, bgtime, -1, -1);
        }
    }

    pub fn find_pid(&mut self, pid: i32) -> Option<&zsh_sys::process> {
        unsafe{
            for i in 1..=zsh_sys::maxjob {
                let mut proc = (*zsh_sys::jobtab.add(i as _)).auxprocs;
                while let Some(p) = proc.as_ref() {
                    if p.pid == pid {
                        return Some(p);
                    }
                    proc = p.next;
                }

            }
        }
        None
    }

    pub fn get_var(&mut self, name: &BStr) -> anyhow::Result<Option<zsh::Value>> {
        zsh::get_var(name)
    }

    pub fn set_var(&mut self, name: &BStr, value: zsh::Value) -> anyhow::Result<()> {
        zsh::set_var(name, value)
    }

}
