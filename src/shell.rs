use std::os::fd::AsRawFd;
use anyhow::Result;
use std::io::{Write};
use std::os::fd::{RawFd};
use std::ptr::NonNull;
use std::os::raw::{c_long, c_char, c_int};
use std::ffi::{CString, CStr};
use std::default::Default;
use std::sync::{Arc, Weak};
use std::ptr::null_mut;
use std::sync::Mutex;
use tokio::sync::{Semaphore, SemaphorePermit};
use bstr::{BStr, BString, ByteSlice, ByteVec};

mod externs;
mod zsh;
pub use zsh::{
    completion,
    history,
    variables,
    parser::Token,
};
use variables::Variable;

pub enum KeybindValue<'a, 'b> {
    String(BString),
    Widget(zsh::ZleWidget<'a, 'b>),
}


struct Private;

pub struct ShellInner<'a> {
    // pub closed: bool,
    parent: &'a Shell,
    _permit: SemaphorePermit<'a>,
    _private: Private,
}

struct Shout {
    reader: std::io::PipeReader,
    #[allow(dead_code)]
    writer: std::io::PipeWriter,
    writer_ptr: NonNull<nix::libc::FILE>,
}
unsafe impl Send for Shout {}

crate::strong_weak_wrapper! {
    pub struct Shell{
        // this needs to be a semaphore so i can add more permits
        inner: Arc::<Semaphore> [Weak::<Semaphore>],
        // many functions are ok to call and are re-entrant
        // but some are not e.g. completion
        exclusive_lock: Arc::<Mutex<()>> [Weak::<Mutex<()>>],
        // shout
        shout: Arc::<Mutex<Option<Shout>>> [Weak::<Mutex<Option<Shout>>>],
    }
}


impl Shell {
    pub fn new() -> Self {
        Self{
            inner: Arc::new(Semaphore::new(1)),
            exclusive_lock: Arc::new(Mutex::new(())),
            shout: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn lock(&self) -> ShellInner<'_> {
        ShellInner{
            parent: self,
            _permit: self.inner.acquire().await.unwrap(),
            _private: Private,
        }
    }

    pub fn is_locked(&self) -> bool {
        self.inner.available_permits() == 0
    }

    pub async fn with_tmp_permit<R, T: std::future::Future<Output=R>, F: FnOnce() -> T>(&self, f: F) -> R {
        self.inner.add_permits(1);
        let result = f().await;
        self.inner.acquire().await.unwrap().forget();
        result
    }

}

#[derive(Clone)]
pub struct Completer{
    inner: Arc<Mutex<zsh::completion::Streamer>>,
    exclusive_lock: Arc<Mutex<()>>,
}

impl Completer {
    pub fn run(&self, shell: &mut ShellInner) -> Result<BString> {
        let lock = self.exclusive_lock.lock().unwrap();
        let (msg, _) = shell.capture_shout(|_| zsh::completion::get_completions(&self.inner))?;
        drop(lock);
        Ok(msg)
    }

    pub fn cancel(&self) -> anyhow::Result<()> {
        self.inner.lock().unwrap().cancel()
    }

    pub fn get_completion_word_len(&self) -> usize {
        self.inner.lock().unwrap().completion_word_len
    }
}

impl<'a> ShellInner<'a> {

    pub fn init_interactive(&mut self) {
        unsafe {
            zsh_sys::opts[zsh_sys::INTERACTIVE as usize] = 1;
            zsh_sys::opts[zsh_sys::SHINSTDIN as usize] = 1;

            // zle_main runs these
            let keymap = CString::new("main").unwrap();
            zsh::selectkeymap(keymap.as_ptr().cast_mut(), 1);
            zsh::initundo();
        }
    }

    pub fn exec(&mut self, string: &BStr) -> c_long {
        zsh::execstring(string, Default::default())
    }

    pub fn exec_subshell<I: Iterator<Item=(RawFd, RawFd)>>(
        &mut self,
        string: &BStr,
        job_control: bool,
        redirections: I,
    ) -> Result<c_long> {

        // okkkkkkkk
        // so ideally, we would just fork() and execstring()
        // except that zsh will think that its still the group leader
        // and there's probably some settings buried somewhere to
        // tell it that it isn't and i can't be stuffed figuring it out,
        // so i'm going to do this instead

        let mut cmd = BString::new(vec![]);
        // apply all the redirections
        for (left, right) in redirections {
            write!(cmd, "{left}>{right} ").unwrap();
        }
        cmd.push_str(" ( eval '");
        // escape it
        string.replace_into(b"'", b"'\\''", &mut cmd);
        cmd.push_str("' ) &");
        if !job_control {
            cmd.push_str("!");
        }

        let code = zsh::execstring(cmd, Default::default());
        if code > 0 {
            // somehow failed to spawn subshell?
            anyhow::bail!("failed to start subshell with error code {code}");
        }

        Ok(unsafe{ zsh_sys::lastpid })
    }

    pub fn get_completions(&self, string: &BStr) -> (Arc<tokio::sync::Mutex<zsh::completion::StreamConsumer>>, Completer) {
        let (consumer, producer) = zsh::completion::get_completer(string);
        let completer = Completer{
            inner: producer,
            exclusive_lock: self.parent.exclusive_lock.clone(),
        };
        (consumer, completer)
    }

    pub fn clear_completion_cache(&self) {
        zsh::completion::clear_cache();
    }

    pub fn insert_completion(&self, string: &BStr, completion_word_len: usize, m: &zsh::cmatch) -> (BString, usize) {
        zsh::completion::insert_completion(string, completion_word_len, m)
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

    pub fn remove_invisible_chars(string: &CStr) -> std::borrow::Cow<'_, CStr> {
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

    pub fn get_history<'b>(&'b mut self) -> zsh::history::History<'b, 'a> {
        zsh::history::History::get(self)
    }

    pub fn get_histline(&mut self) -> c_int {
        unsafe{ zsh::histline }
    }

    pub fn set_histline(&mut self, histline: c_long) -> Option<zsh::history::EntryPtr<'_>> {
        let history = self.get_history();

        if let Some(entry) = history.closest_to(histline, std::cmp::Ordering::Greater) {
            // found a good enough match
            unsafe{ zsh::histline = entry.histnum() as _; }
            Some(entry)

        } else {
            // no history
            unsafe{ zsh::histline = 0; }
            None
        }
    }

    pub fn push_history(&mut self, string: &BStr) -> zsh::history::EntryPtr<'_> {
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

    pub fn get_var(&mut self, name: &BStr) -> anyhow::Result<Option<variables::Value>> {
        if let Some(mut v) = Variable::get(name) {
            Ok(Some(v.as_value()?))
        } else {
            Ok(None)
        }
    }

    pub fn startparamscope(&mut self) {
        unsafe{ zsh_sys::startparamscope() }
    }

    pub fn endparamscope(&mut self) {
        unsafe{ zsh_sys::endparamscope() }
    }

    pub fn start_zle_scope(&mut self) {
        zsh::start_zle_scope();
    }

    pub fn end_zle_scope(&mut self) {
        zsh::end_zle_scope();
    }

    pub fn set_var(&mut self, name: &BStr, value: variables::Value, local: bool) -> anyhow::Result<()> {
        Variable::set(name, value, local)
    }

    pub fn unset_var(&mut self, name: &BStr) {
        Variable::unset(name)
    }

    pub fn export_var(&mut self, name: &BStr) -> bool {
        if let Some(var) = Variable::get(name) {
            var.export();
            true
        } else {
            false
        }
    }

    pub fn expandhistory(&mut self, buffer: BString) -> Option<BString> {
        let cursor = buffer.len() as i64 + 1;
        self.set_zle_buffer(buffer, cursor);
        if unsafe{ zsh::expandhistory() } == 0 {
            Some(self.get_zle_buffer().0)
        } else {
            None
        }
    }

    pub fn get_cwd(&mut self) -> BString {
        unsafe {
            let ptr = zsh_sys::zgetcwd();
            CStr::from_ptr(ptr).to_bytes().into()
        }
    }

    pub fn set_zle_buffer(&mut self, buffer: BString, cursor: i64) {
        zsh::start_zle_scope();
        Variable::set(b"BUFFER", buffer.into(), true).unwrap();
        Variable::set(b"CURSOR", cursor.into(), true).unwrap();
        zsh::end_zle_scope();
    }

    pub fn get_zle_buffer(&mut self) -> (BString, Option<i64>) {
        zsh::start_zle_scope();
        let buffer = Variable::get("BUFFER").unwrap().as_bytes();
        let cursor = Variable::get("CURSOR").unwrap().try_as_int();
        zsh::end_zle_scope();
        match cursor {
            Ok(Some(cursor)) => (buffer, Some(cursor)),
            _ => (buffer, None),
        }
    }

    pub fn get_keybinding<'b>(&'b mut self, key: &BStr) -> Option<KeybindValue<'b, 'a>> {
        let mut strp: *mut c_char = std::ptr::null_mut();
        let key = zsh::metafy(key.into());

        let keymap = unsafe{ NonNull::new(zsh::localkeymap).map_or(zsh::curkeymap, |x| x.as_ptr()) };
        let keybind = unsafe{ zsh::keybind(keymap, key, &raw mut strp) };
        if let Some(keybind) = NonNull::new(keybind) {
            return Some(KeybindValue::Widget(zsh::ZleWidget::new(keybind, self)))
        }
        let strp = NonNull::new(strp)?;
        let strp = unsafe{ CStr::from_ptr(strp.as_ptr()) }.to_bytes();
        Some(KeybindValue::String(strp.into()))
    }

    pub fn set_lastchar(&mut self, char: &[u8]) {
        let char: u32 = match std::str::from_utf8(char) {
            Ok(c) => c.chars().next().unwrap().into(),
            Err(e) => if e.valid_up_to() == 0 {
                // invalid utf8, use the first byte? or space?
                char.first().copied().unwrap_or(b' ').into()
            } else {
                std::str::from_utf8(&char[..e.valid_up_to()]).unwrap().chars().next().unwrap().into()
            },
        };
        let char: i32 = char as _;
        unsafe {
            zsh::lastchar = char;
            zsh::lastchar_wide = char;
            zsh::lastchar_wide_valid = 1;
        }
    }

    pub fn acceptline(&mut self) {
        unsafe { zsh::acceptline(); }
    }

    pub fn has_accepted_line(&mut self) -> bool {
        unsafe{ zsh::done != 0 }
    }

    pub fn call_signal_handler(&mut self, signal: c_int, unqueue: bool) {
        unsafe {
            let old_value = zsh_sys::queueing_enabled;
            if unqueue {
                zsh_sys::queueing_enabled = 0;
            }
            zsh_sys::zhandler(signal);
            if unqueue {
                zsh_sys::queueing_enabled = old_value;
            }
        }
    }

    pub fn capture_shout<T, F: FnOnce(&mut Self) -> T>(&mut self, f: F) -> Result<(BString, T)> {
        let mut shout = self.parent.shout.lock().unwrap();

        let shout = if let Some(shout) = &mut *shout {
            shout
        } else {
            let (reader, writer) = std::io::pipe()?;
            let writer_ptr = unsafe{ nix::libc::fdopen(writer.as_raw_fd(), c"w".as_ptr()) };
            let Some(writer_ptr) = NonNull::new(writer_ptr)
            else {
                return Err(std::io::Error::last_os_error())?;
            };

            crate::utils::set_nonblocking_fd(&reader)?;
            shout.get_or_insert(
                Shout {
                    reader,
                    writer,
                    writer_ptr,
                }
            )
        };

        Ok(zsh::capture_shout(&mut shout.reader, shout.writer_ptr, || f(self)))
    }

}
