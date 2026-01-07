use std::collections::HashMap;
use tokio::sync::{mpsc};
use std::os::fd::AsRawFd;
use anyhow::Result;
use std::any::Any;
use std::io::{Write};
use std::os::fd::{RawFd};
use std::ptr::NonNull;
use std::os::raw::{c_long, c_char, c_int};
use std::ffi::{CString, CStr};
use std::default::Default;
use std::sync::{Arc};
use std::ptr::null_mut;
use std::sync::Mutex;
use bstr::{BStr, BString, ByteSlice, ByteVec};

mod externs;
mod file_stream;
mod zsh;
#[macro_use]
mod actor_macro;
pub use zsh::{
    completion,
    history,
    variables,
    functions::Function,
    parser::{Token},
    ZptyOpts,
    Zpty,
};
pub use externs::{weak_main, with_runtime};
pub use externs::signals::{wait_for_pid};
use variables::Variable;

pub enum KeybindValue<'a> {
    String(BString),
    Widget(zsh::ZleWidget<'a>),
}

impl<'a> KeybindValue<'a> {
    pub fn find(shell: &'a Shell, key: &BStr) -> Option<Self> {
        let mut strp: *mut c_char = std::ptr::null_mut();
        let key = zsh::metafy(key.into());

        let keymap = unsafe{ NonNull::new(zsh::localkeymap).map_or(zsh::curkeymap, |x| x.as_ptr()) };
        let keybind = unsafe{ zsh::keybind(keymap, key, &raw mut strp) };
        if let Some(keybind) = NonNull::new(keybind) {
            return Some(KeybindValue::Widget(zsh::ZleWidget::new(keybind, shell)))
        }
        let strp = NonNull::new(strp)?;
        let strp = unsafe{ CStr::from_ptr(strp.as_ptr()) }.to_bytes();
        Some(KeybindValue::String(strp.into()))
    }
}


struct Shout {
    reader: std::io::PipeReader,
    #[allow(dead_code)]
    writer: std::io::PipeWriter,
    writer_ptr: NonNull<nix::libc::FILE>,
}
unsafe impl Send for Shout {}

impl Shout {
    fn new() -> Result<Self> {
        let (reader, writer) = std::io::pipe()?;
        let writer_ptr = unsafe{ nix::libc::fdopen(writer.as_raw_fd(), c"w".as_ptr()) };
        let Some(writer_ptr) = NonNull::new(writer_ptr)
        else {
            return Err(std::io::Error::last_os_error())?;
        };

        crate::utils::set_nonblocking_fd(&reader)?;
        Ok(Shout {
            reader,
            writer,
            writer_ptr,
        })
    }

    fn capture<T, F: FnOnce() -> T>(&mut self, f: F) -> Result<(BString, T)> {
        Ok(zsh::capture_shout(&mut self.reader, self.writer_ptr, f))
    }
}

#[derive(Clone)]
pub struct Shell {
    is_waiting: Arc<std::sync::atomic::AtomicBool>,
    shout: Arc<Mutex<Option<Shout>>>,
    main_thread: std::thread::ThreadId,
    trampoline: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl std::default::Default for Shell {
    fn default() -> Self {
        Self {
            is_waiting: Arc::default(),
            shout: Arc::default(),
            trampoline: Arc::default(),
            main_thread: std::thread::current().id(),
        }
    }
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

pub fn control_c() -> nix::Result<()> {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(0), nix::sys::signal::Signal::SIGINT)
}

crate::TokioActor! {
    impl Shell {

        pub fn run(&self, func: Box<dyn Send + Fn(&Shell) -> Box<dyn Any + Send>>) -> Box<dyn Any + Send> {
            func(self)
        }

        pub fn init_interactive(&self) {
            unsafe {
                zsh_sys::opts[zsh_sys::INTERACTIVE as usize] = 1;
                zsh_sys::opts[zsh_sys::SHINSTDIN as usize] = 1;

                // zle_main runs these
                let keymap = CString::new("main").unwrap();
                zsh::selectkeymap(keymap.as_ptr().cast_mut(), 1);
                zsh::initundo();
            }
        }

        pub fn exec(&self, string: BString) -> c_long {
            zsh::execstring(string, Default::default())
        }

        pub fn exec_subshell(
            &self,
            string: BString,
            job_control: bool,
            redirections: Vec<(RawFd, RawFd)>
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

        pub fn zpty(&self, name: BString, cmd: BString, opts: ZptyOpts) -> Result<Zpty> {
            let cmd = CString::new(cmd).unwrap();
            let name = CString::new(name).unwrap();
            zsh::zpty(name, &cmd, opts)
        }

        pub fn zpty_delete(&self, name: BString) -> c_long {
            let name = CString::new(name).unwrap();
            let mut cmd = zsh::shell_quote(&name);
            cmd.insert_str(0, "zpty -d ");
            zsh::execstring(cmd, Default::default())
        }

        pub fn get_completions(&self, line: BString, sender: mpsc::UnboundedSender<Vec<zsh::completion::Match>>) -> Result<BString> {
            let mut shout = self.shout.lock().unwrap();
            let shout = if let Some(shout) = &mut *shout {
                shout
            } else {
                shout.get_or_insert(Shout::new()?)
            };
            // this may block for a long time
            let (msg, _) = shout.capture(|| zsh::completion::get_completions(line, sender))?;
            Ok(msg)
        }

        pub fn insert_completion(&self, string: BString, m: Arc<zsh::completion::Match>) -> (BString, usize) {
            zsh::completion::insert_completion(string, &m)
        }

        pub fn parse(&self, string: BString, options: zsh::parser::ParserOptions) -> (bool, Vec<zsh::parser::Token>) {
            zsh::parser::parse(string, options)
        }

        pub fn get_prompt(&self, prompt: Option<BString>, escaped: bool) -> Option<CString> {
            zsh::get_prompt(prompt.as_ref().map(|p| p.as_ref()), escaped)
        }

        pub fn get_prompt_size(&self, prompt: CString) -> (usize, usize) {
            let (width, height) = zsh::get_prompt_size(&prompt);
            (width as _, height as _)
        }

        pub fn readhistfile(&self) {
            unsafe{ zsh_sys::readhistfile(null_mut(), 0, zsh_sys::HFILE_USE_OPTIONS as _); }
        }

        pub fn get_histline(&self) -> c_int {
            unsafe{ zsh::histline }
        }

        pub fn add_pid(&self, pid: i32) {
            zsh::add_pid(pid);
        }

        pub fn find_process_status(&self, pid: i32, pop_if_done: bool) -> Option<c_int> {
            unsafe{
                let job = zsh_sys::jobtab.add(*zsh::JOB as usize);
                let mut prev: *mut zsh_sys::process = null_mut();
                let mut proc = (*job).auxprocs;
                while let Some(p) = proc.as_ref() {
                    // found it
                    if p.pid == pid {
                        let status = p.status;
                        if pop_if_done && status >= 0 {
                            if prev.is_null() {
                                (*job).auxprocs = p.next;
                            } else {
                                (*prev).next = p.next;
                            }
                            zsh_sys::zfree(proc.cast(), std::mem::size_of::<zsh_sys::process>() as _);
                        }
                        return Some(status);
                    }
                    prev = proc;
                    proc = p.next;
                }
            }
            None
        }

        pub fn get_var(&self, name: BString) -> anyhow::Result<Option<variables::Value>> {
            if let Some(mut v) = Variable::get(CString::new(name)?) {
                Ok(Some(v.as_value()?))
            } else {
                Ok(None)
            }
        }

        pub fn startparamscope(&self) {
            unsafe{ zsh_sys::startparamscope() }
        }

        pub fn endparamscope(&self) {
            unsafe{ zsh_sys::endparamscope() }
        }

        pub fn start_zle_scope(&self) {
            zsh::start_zle_scope();
        }

        pub fn end_zle_scope(&self) {
            zsh::end_zle_scope();
        }

        pub fn set_var(&self, name: BString, value: variables::Value, local: bool) -> anyhow::Result<()> {
            Variable::set(&name, value, local)
        }

        pub fn unset_var(&self, name: BString) {
            Variable::unset(&name);
        }

        pub fn export_var(&self, name: BString) -> bool {
            if let Ok(name) = CString::new(name) && let Some(var) = Variable::get(name) {
                var.export();
                true
            } else {
                false
            }
        }

        pub fn create_dynamic_string_var(
            &self,
            name: BString,
            get: Box<dyn Send + Fn() -> BString>,
            set: Option<Box<dyn Send + Fn(BString)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) -> Result<()> {
            Variable::create_dynamic(&name, get, set, unset)
        }

        pub fn create_dynamic_integer_var(
            &self,
            name: BString,
            get: Box<dyn Send + Fn() -> c_long>,
            set: Option<Box<dyn Send + Fn(c_long)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) -> Result<()> {
            Variable::create_dynamic(&name, get, set, unset)
        }

        pub fn create_dynamic_float_var(
            &self,
            name: BString,
            get: Box<dyn Send + Fn() -> f64>,
            set: Option<Box<dyn Send + Fn(f64)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) -> Result<()> {
            Variable::create_dynamic(&name, get, set, unset)
        }

        pub fn create_dynamic_array_var(
            &self,
            name: BString,
            get: Box<dyn Send + Fn() -> Vec<BString>>,
            set: Option<Box<dyn Send + Fn(Vec<BString>)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) -> Result<()> {
            Variable::create_dynamic(&name, get, set, unset)
        }

        pub fn create_dynamic_hash_var(
            &self,
            name: BString,
            get: Box<dyn Send + Fn() -> HashMap<BString, BString>>,
            set: Option<Box<dyn Send + Fn(HashMap<BString, BString>)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) -> Result<()> {
            Variable::create_dynamic(&name, get, set, unset)
        }

        pub fn goto_history(&self, index: history::HistoryIndex, skipdups: bool) {
            history::History::goto(index, skipdups)
        }

        pub fn expandhistory(&self, buffer: BString) -> Option<BString> {
            let cursor = buffer.len() as i64 + 1;
            self.set_zle_buffer(buffer, cursor);
            if unsafe{ zsh::expandhistory() } == 0 {
                Some(self.get_zle_buffer().0)
            } else {
                None
            }
        }

        pub fn get_cwd(&self) -> BString {
            unsafe {
                let ptr = zsh_sys::zgetcwd();
                CStr::from_ptr(ptr).to_bytes().into()
            }
        }

        pub fn set_zle_buffer(&self, buffer: BString, cursor: i64) {
            zsh::start_zle_scope();
            Variable::set(b"BUFFER", buffer.into(), true).unwrap();
            Variable::set(b"CURSOR", cursor.into(), true).unwrap();
            zsh::end_zle_scope();
        }

        pub fn get_zle_buffer(&self) -> (BString, Option<i64>) {
            zsh::start_zle_scope();
            let buffer = Variable::get(c"BUFFER").unwrap().as_bytes();
            let cursor = Variable::get(c"CURSOR").unwrap().try_as_int();
            zsh::end_zle_scope();
            match cursor {
                Ok(Some(cursor)) => (buffer, Some(cursor)),
                _ => (buffer, None),
            }
        }

        pub fn set_lastchar(&self, char: [u8; 4]) {
            let char: u32 = match std::str::from_utf8(&char) {
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

        pub fn accept_line_trampoline(&self, line: Option<BString>) {
            unreachable!("{:?}", line)
        }

        pub fn acceptline(&self) {
            unsafe { zsh::acceptline(); }
        }

        pub fn has_accepted_line(&self) -> bool {
            unsafe{ zsh::done != 0 }
        }

        pub fn make_function(&self, code: BString) -> Result<Arc<zsh::functions::Function>> {
            let func = zsh::functions::Function::new(code.as_ref())?;
            Ok(Arc::new(func))
        }

        pub fn exec_function(
            &self,
            function: Arc<zsh::functions::Function>,
            arg0: Option<BString>,
            args: Vec<BString>
        ) -> c_int {
            function.execute(arg0.as_ref().map(|x| x.as_ref()), args.iter().map(|x| x.as_ref()))
        }

        pub fn get_function_source(&self, function: Arc<zsh::functions::Function>) -> BString {
            function.get_source()
        }

        pub fn queue_signals(&self) {
            zsh::queue_signals();
        }

        pub fn unqueue_signals(&self) -> nix::Result<()> {
            zsh::unqueue_signals()
        }

        pub fn call_hook_func(&self, name: BString, args: Vec<BString>) -> Option<c_int> {
            zsh::call_hook_func(CString::new(name).unwrap(), args.iter().map(|x| x.as_ref()))
        }

    }

}
