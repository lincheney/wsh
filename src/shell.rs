use std::borrow::Cow;
use std::collections::HashMap;
use tokio::sync::{mpsc};
use anyhow::Result;
use std::any::Any;
use std::io::{Write};
use std::os::fd::{RawFd};
use std::ptr::NonNull;
use std::os::raw::{c_long, c_char, c_int};
use std::default::Default;
use std::sync::{Arc};
use std::ptr::null_mut;
use std::sync::{Mutex};
use bstr::{BString, ByteSlice, ByteVec};

mod externs;
mod file_stream;
mod zsh;
#[macro_use]
mod actor_macro;
use crate::meta_str;
pub use zsh::{
    completion,
    bin_zle,
    history,
    variables,
    process,
    signals,
    functions::Function,
    parser::{Token},
    ZptyOpts,
    Zpty,
    MetaStr,
    MetaString,
    MetaArray,
};
pub use externs::{run_with_shell};
pub use variables::Variable;

pub struct Shell {
    inner: ShellInternal,
    trampoline: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    queue: std::sync::mpsc::Receiver<ShellMsg>,
}

pub type ShellMsg = ShellInternalMsg;

impl Shell {
    pub fn make() -> (Self, ShellClient) {
        let (sender, receiver) = std::sync::mpsc::channel();

        let shell = ShellInternal {
            sink: Arc::new(Mutex::new(file_stream::Sink::new().unwrap())),
            main_thread: std::thread::current().id(),
        };
        let client = ShellClient {
            inner: ShellInternalClient {
                inner: shell.clone(),
                queue: sender,
            },
        };
        let shell = Shell {
            inner: shell.clone(),
            trampoline: Arc::default(),
            queue: receiver,
        };

        (shell, client)
    }

    pub fn recv_from_queue(&self) -> Result<Result<ShellMsg, std::sync::mpsc::RecvError>> {
        // let signals run while we are waiting for the next cmd
        self.inner.unqueue_signals()?;
        let msg = self.queue.recv();
        self.inner.queue_signals();
        Ok(msg)
    }

    pub fn handle_one_message(&self, msg: ShellMsg) {
        self.inner.handle_one_message(msg);
    }
}

pub struct ShellClient {
    inner: ShellInternalClient,
}
crate::impl_deref_helper!(self: ShellClient, &self.inner => ShellInternalClient);

impl ShellClient {
    pub async fn accept_line_trampoline(&self, line: Option<BString>) -> Result<(), tokio::sync::oneshot::error::RecvError> {
        let (sender, receiver) = ::tokio::sync::oneshot::channel();
        self.queue.send(ShellInternalMsg::accept_line_trampoline{line, returnvalue: sender}).unwrap();
        receiver.await
    }

    pub async fn run<T: 'static + Send, F: 'static + Sync + Send + FnOnce(&ShellInternal) -> T>(&self, func: F) -> T {
        *self.inner.run(Box::new(move |shell| Box::new(func(shell)))).await.downcast().unwrap()
    }
}


pub enum KeybindValue<'a> {
    String(BString),
    Widget(zsh::ZleWidget<'a>),
}

impl<'a> KeybindValue<'a> {
    pub fn find(shell: &'a ShellInternal, key: &MetaStr) -> Option<Self> {
        let mut strp: *mut c_char = std::ptr::null_mut();

        let keymap = unsafe{ NonNull::new(zsh::localkeymap).map_or(zsh::curkeymap, |x| x.as_ptr()) };
        let keybind = unsafe{ zsh::keybind(keymap, key.as_ptr().cast_mut(), &raw mut strp) };
        if let Some(keybind) = NonNull::new(keybind) {
            return Some(KeybindValue::Widget(zsh::ZleWidget::new(keybind, shell)))
        }
        let strp = NonNull::new(strp)?;
        let strp = unsafe{ MetaStr::from_ptr(strp.as_ptr()) }.to_bytes();
        Some(KeybindValue::String(strp.into()))
    }
}


pub fn remove_invisible_chars(string: Cow<'_, MetaStr>) -> Cow<'_, MetaStr> {
    let bytes = string.to_bytes();
    if bytes.contains(&(zsh::Inpar as _)) || bytes.contains(&(zsh::Outpar as _)) || bytes.contains(&(zsh::Meta as _)) {
        let mut string = string.into_owned();
        string.modify(|buf| buf.retain(|c| *c != zsh::Inpar as _ && *c != zsh::Outpar as _));
        Cow::Owned(string)
    } else {
        string
    }
}

pub fn shell_quote(mut string: MetaString) -> MetaString {
    string.modify(|string| {
        let mut start = 0;
        while let Some(found) = string[start..].find_byteset(b"\\'") {
            // insert an escape here
            string.insert_str(start + found, b"\\");
            start += found + 2;
        }
        string.insert_str(0, b"$'");
        string.push_str(b"'");
    });
    string
}

pub fn control_c() -> nix::Result<()> {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(0), nix::sys::signal::Signal::SIGINT)
}

pub fn get_var(_shell: &ShellInternal, string: &zsh::MetaStr) -> Option<Variable> {
    Variable::get(string)
}

#[derive(Clone)]
pub struct ShellInternal {
    sink: Arc<Mutex<file_stream::Sink>>,
    main_thread: std::thread::ThreadId,
}

crate::TokioActor! {
    impl ShellInternal {

        pub fn run(&self, func: Box<dyn Send + FnOnce(&ShellInternal) -> Box<dyn Any + Send>>) -> Box<dyn Any + Send> {
            func(self)
        }

        pub fn init_interactive(&self) {
            unsafe {
                zsh_sys::opts[zsh_sys::INTERACTIVE as usize] = 1;
                zsh_sys::opts[zsh_sys::SHINSTDIN as usize] = 1;

                // zle_main runs these
                let keymap = meta_str!(c"main");
                zsh::selectkeymap(keymap.as_ptr().cast_mut(), 1);
                zsh::initundo();
            }
        }

        pub fn exec(&self, string: MetaString) -> c_long {
            zsh::execstring(string.as_ref(), Default::default())
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

            let cmd: MetaString = cmd.into();
            let code = zsh::execstring(cmd.as_ref(), Default::default());
            if code > 0 {
                // somehow failed to spawn subshell?
                anyhow::bail!("failed to start subshell with error code {code}");
            }

            Ok(unsafe{ zsh_sys::lastpid })
        }

        pub fn zpty(&self, name: MetaString, cmd: MetaString, opts: ZptyOpts) -> Result<Zpty> {
            zsh::zpty(name, cmd.as_ref(), opts)
        }

        pub fn zpty_delete(&self, name: MetaString) -> c_long {
            let mut cmd = shell_quote(name);
            cmd.insert_str(0, meta_str!(c"zpty -d "));
            zsh::execstring(cmd.as_ref(), Default::default())
        }

        pub fn get_completions(&self, line: BString, sender: mpsc::UnboundedSender<Vec<zsh::completion::Match>>) -> Result<BString> {
            // this may block for a long time
            let sink = &mut *self.sink.lock().unwrap();
            let (msg, _) = zsh::capture_shout(sink, || zsh::completion::get_completions(line, sender));
            Ok(msg)
        }

        pub fn insert_completion(&self, string: BString, m: Arc<zsh::completion::Match>) -> (BString, usize) {
            zsh::completion::insert_completion(string, &m)
        }

        pub fn parse(&self, string: BString, options: zsh::parser::ParserOptions) -> (bool, Vec<zsh::parser::Token>) {
            zsh::parser::parse(string, options)
        }

        pub fn get_prompt(&self, prompt: Option<MetaString>, escaped: bool) -> Option<MetaString> {
            zsh::get_prompt(prompt.as_ref().map(|p| p.as_ref()), escaped)
        }

        pub fn get_prompt_size(&self, prompt: Cow<'static, MetaStr>, term_width: Option<c_long>) -> (usize, usize) {
            let (width, height) = zsh::get_prompt_size(prompt.as_ref(), term_width);
            (width as _, height as _)
        }

        pub fn readhistfile(&self) {
            unsafe{ zsh_sys::readhistfile(null_mut(), 0, zsh_sys::HFILE_USE_OPTIONS as _); }
        }

        pub fn get_histline(&self) -> c_int {
            unsafe{ zsh::histline }
        }

        // pub fn add_pid(&self, pid: i32) {
            // zsh::add_pid(pid);
        // }

        pub fn find_process_status(&self, pid: i32, pop_if_done: bool) -> Option<c_int> {
            zsh::find_process_status(pid, pop_if_done)
        }

        pub fn check_pid_status(&self, pid: i32) -> Option<c_int> {
            zsh::process::check_pid_status(pid)
        }

        pub fn get_var(&self, name: MetaString, zle: bool) -> anyhow::Result<Option<variables::Value>> {
            if zle {
                self.start_zle_scope();
            }
            let result = if let Some(mut v) = Variable::get(name.as_ref()) {
                v.as_value().map(Some)
            } else {
                Ok(None)
            };
            if zle {
                self.end_zle_scope();
            }
            result
        }

        pub fn get_var_as_string(&self, name: MetaString, zle: bool) -> anyhow::Result<Option<BString>> {
            if zle {
                self.start_zle_scope();
            }
            let result = Variable::get(name.as_ref()).map(|mut v| v.as_bytes());
            if zle {
                self.end_zle_scope();
            }
            Ok(result)
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

        pub fn set_var(&self, name: MetaString, value: variables::Value, local: bool) -> anyhow::Result<()> {
            Variable::set(name.as_ref(), value, local)
        }

        pub fn unset_var(&self, name: MetaString) {
            Variable::unset(name.as_ref());
        }

        pub fn export_var(&self, name: MetaString) -> bool {
            if let Some(var) = Variable::get(name.as_ref()) {
                var.export();
                true
            } else {
                false
            }
        }

        pub fn create_dynamic_string_var(
            &self,
            name: MetaString,
            get: Box<dyn Send + Fn() -> BString>,
            set: Option<Box<dyn Send + Fn(BString)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) {
            Variable::create_dynamic(name.as_ref(), get, set, unset);
        }

        pub fn create_dynamic_integer_var(
            &self,
            name: MetaString,
            get: Box<dyn Send + Fn() -> c_long>,
            set: Option<Box<dyn Send + Fn(c_long)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) {
            Variable::create_dynamic(name.as_ref(), get, set, unset);
        }

        pub fn create_dynamic_float_var(
            &self,
            name: MetaString,
            get: Box<dyn Send + Fn() -> f64>,
            set: Option<Box<dyn Send + Fn(f64)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) {
            Variable::create_dynamic(name.as_ref(), get, set, unset);
        }

        pub fn create_dynamic_array_var(
            &self,
            name: MetaString,
            get: Box<dyn Send + Fn() -> Vec<BString>>,
            set: Option<Box<dyn Send + Fn(Vec<BString>)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) {
            Variable::create_dynamic(name.as_ref(), get, set, unset);
        }

        pub fn create_dynamic_hash_var(
            &self,
            name: MetaString,
            get: Box<dyn Send + Fn() -> HashMap<BString, BString>>,
            set: Option<Box<dyn Send + Fn(HashMap<BString, BString>)>>,
            unset: Option<Box<dyn Send + Fn(bool)>>
        ) {
            Variable::create_dynamic(name.as_ref(), get, set, unset);
        }

        pub fn goto_history(&self, index: history::HistoryIndex, skipdups: bool) {
            history::History::goto(self, index, skipdups);
        }

        pub fn append_history(&self, text: BString) -> Result<()> {
            history::History::append(self, text)
        }

        pub fn append_history_words(&self, words: Vec<BString>) -> Result<()> {
            history::History::append_words(self, words)
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
                MetaStr::from_ptr(zsh_sys::pwd).unmetafy().into_owned()
            }
        }

        pub fn set_zle_buffer(&self, buffer: BString, cursor: i64) {
            zsh::set_zle_buffer(buffer, cursor);
        }

        pub fn get_zle_buffer(&self) -> (BString, Option<i64>) {
            zsh::get_zle_buffer()
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

        pub fn exit(&self, code: i32) {
            zsh::exit(code);
        }

        pub fn acceptline(&self) {
            unsafe { zsh::acceptline(); }
        }

        pub fn has_accepted_line(&self) -> bool {
            unsafe{ zsh::done != 0 }
        }

        pub fn make_function(&self, code: MetaString) -> Result<Arc<zsh::functions::Function>> {
            let func = zsh::functions::Function::new(code.as_ref())?;
            Ok(Arc::new(func))
        }

        pub fn exec_function(
            &self,
            function: Arc<zsh::functions::Function>,
            arg0: Option<MetaString>,
            args: Vec<MetaString>
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

        pub fn call_hook_func(&self, name: Cow<'static, MetaStr>, args: Vec<MetaString>) -> Option<c_int> {
            // needs metafy
            zsh::call_hook_func(name.as_ref(), args.iter().map(|x| x.as_ref()))
        }

        pub fn run_watch_fd(&self, hook: Arc<bin_zle::FdChangeHook>, fd: RawFd, error: Option<std::io::Error>) {
            hook.run(self, fd, error);
        }

    }

}
