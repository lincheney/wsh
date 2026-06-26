use std::rc::Rc;
use crate::ui::Ui;
use std::ops::ControlFlow;
use std::cell::{RefCell, Cell};
use std::borrow::Cow;
use std::collections::HashMap;
use anyhow::Result;
use std::io::{Write};
use std::os::fd::{RawFd};
use std::ptr::NonNull;
use std::os::raw::{c_long, c_char, c_int};
use std::default::Default;
use std::ptr::null_mut;
use bstr::{BString, BStr, ByteSlice, ByteVec};
use tokio::sync::oneshot;

mod externs;
mod file_stream;
mod zsh;
use crate::meta_str;
pub use zsh::{
    completion,
    history,
    variables,
    signals,
    functions::Function,
    parser::{Token, ParserOptions},
    ZptyOpts,
    Zpty,
    set_zpty_size,
    MetaStr,
    MetaString,
    MetaSlice,
    ZleWidget,
};
pub use variables::Variable;

pub struct TrampolineToken(());
type TrampolinePayload = (Box<dyn FnOnce(Ui, TrampolineToken)>, TrampolineToken);
enum Trampoline<T=TrampolinePayload> {
    Resumed(oneshot::Sender<T>),
    Paused(oneshot::Sender<()>),
}

pub struct Shell {
    trampoline: RefCell<Vec<Option<Trampoline>>>,
    accept_line_trampoline: Cell<Option<Trampoline<Option<BString>>>>,
    sink: RefCell<file_stream::Sink>,
}

pub enum KeybindValue {
    String(BString),
    Widget(zsh::ZleWidget),
}

impl KeybindValue {
    pub fn find(key: &MetaStr) -> Option<Self> {
        let mut strp: *mut c_char = std::ptr::null_mut();

        let keymap = unsafe{ NonNull::new(zsh::localkeymap).map_or(zsh::curkeymap, |x| x.as_ptr()) };
        let keybind = unsafe{ zsh::keybind(keymap, key.as_ptr().cast_mut(), &raw mut strp) };
        if let Some(keybind) = NonNull::new(keybind) {
            return Some(KeybindValue::Widget(zsh::ZleWidget::new(keybind)))
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

pub enum FdAction {
    RedirectFrom(RawFd, Option<RawFd>),
    RedirectTo(RawFd, Option<RawFd>),
    Close(RawFd),
}

impl Shell {
    pub fn new() -> Self {
        Shell {
            sink: RefCell::new(file_stream::Sink::new().unwrap()),
            trampoline: RefCell::new(vec![None]),
            accept_line_trampoline: Cell::new(None),
        }
    }

    pub fn trampoline_push(&self) {
        self.trampoline.borrow_mut().push(None);
    }

    pub fn trampoline_pop(&self) {
        self.trampoline.borrow_mut().pop();
    }

    pub fn trampoline_in(&self) -> oneshot::Receiver<TrampolinePayload> {
        let (sender, receiver) = oneshot::channel();

        let previous = {
            let mut trampoline = self.trampoline.borrow_mut();
            let trampoline = trampoline.last_mut().unwrap();
            trampoline.replace(Trampoline::Resumed(sender))
        };

        if let Some(Trampoline::Paused(previous)) = previous {
            let _ = previous.send(());
        }

        receiver
    }

    pub async fn trampoline_out(&self, payload: TrampolinePayload) -> Result<(), oneshot::error::RecvError> {
        let (sender, receiver) = oneshot::channel();

        let previous = {
            let mut trampoline = self.trampoline.borrow_mut();
            let trampoline = trampoline.last_mut().unwrap();
            trampoline.replace(Trampoline::Paused(sender))
        };

        let Some(Trampoline::Resumed(previous)) = previous
            else { panic!("expected Some(Trampoline::Resumed(..))"); };
        assert!(previous.send(payload).is_ok(), "failed to trampoline out");

        receiver.await
    }

    pub async fn trampoline_out_callback<F: 'static + FnOnce(Ui, TrampolineToken) -> T, T: 'static>(
        &self,
        callback: F,
    ) -> Result<T, oneshot::error::RecvError> {
        let (sender, receiver) = oneshot::channel();
        let callback = Box::new(move |ui, token| {
            let _ = sender.send(callback(ui, token));
        });
        self.trampoline_out((callback, TrampolineToken(()))).await?;
        receiver.await
    }

    pub fn wait_for_accept_line(&self) -> oneshot::Receiver<Option<BString>> {
        let (sender, receiver) = oneshot::channel();

        let previous = {
            self.accept_line_trampoline.replace(Some(Trampoline::Resumed(sender)))
        };

        if let Some(Trampoline::Paused(previous)) = previous {
            let _ = previous.send(());
        }

        receiver
    }

    pub fn accept_line(&self, line: Option<BString>) -> Option<oneshot::Receiver<()>> {
        let (sender, receiver) = oneshot::channel();

        let previous = {
            self.accept_line_trampoline.replace(Some(Trampoline::Paused(sender)))
        };

        if let Some(Trampoline::Resumed(previous)) = previous && previous.send(line).is_err() {
            // unable to trampoline out, return early
            self.accept_line_trampoline.take();
            None
        } else {
            Some(receiver)
        }
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

    pub fn exec(&self, _token: TrampolineToken, string: MetaString) -> c_long {
        zsh::execstring(string.as_ref(), Default::default())
    }

    pub fn exec_subshell<I: Iterator<Item=FdAction>>(
        &self,
        _token: TrampolineToken,
        string: &BStr,
        job_control: bool,
        fd_mapping: I,
    ) -> Result<c_long> {

        // okkkkkkkk
        // so ideally, we would just fork() and execstring()
        // except that zsh will think that its still the group leader
        // and there's probably some settings buried somewhere to
        // tell it that it isn't and i can't be stuffed figuring it out,
        // so i'm going to do this instead

        let mut cmd = BString::new(vec![]);
        cmd.push_str("( ");
        // apply all the fd mappings
        for action in fd_mapping {
            match action {
                FdAction::RedirectFrom(fd, Some(other)) => write!(cmd, "exec {fd}</dev/fd/{other};").unwrap(),
                FdAction::RedirectTo(fd, Some(other)) => write!(cmd, "exec {fd}>/dev/fd/{other};").unwrap(),
                FdAction::RedirectFrom(fd, None) => write!(cmd, "exec {fd}</dev/null;").unwrap(),
                FdAction::RedirectTo(fd, None) => write!(cmd, "exec {fd}>/dev/null;").unwrap(),
                FdAction::Close(fd) => write!(cmd, "__fd={fd}; exec {{__fd}}<&-;").unwrap(),
            }
        }
        cmd.push_str("; eval '");
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

    pub fn zpty(&self, name: MetaString, cmd: &MetaStr, opts: ZptyOpts) -> Result<Zpty> {
        zsh::zpty(name, cmd, opts)
    }

    pub fn zpty_delete(&self, name: MetaString) -> c_long {
        let mut cmd = shell_quote(name);
        cmd.insert_str(0, meta_str!(c"zpty -d "));
        zsh::execstring(cmd.as_ref(), Default::default())
    }

    pub fn get_completions(
        &self,
        _token: TrampolineToken,
        line: BString,
        callback: Box<dyn FnMut(std::iter::Peekable<zsh::completion::MatchIter>) -> ControlFlow<()>>,
    ) -> Result<BString> {
        // this may block for a long time
        let sink = &mut *self.sink.try_borrow_mut()?;
        let (msg, _) = zsh::capture_shout(sink, || zsh::completion::get_completions(line, callback));
        Ok(msg)
    }

    pub fn insert_completion(&self, string: BString, m: &completion::Match) -> (BString, usize) {
        zsh::completion::insert_completion(string, m)
    }

    pub fn parse(&self, string: BString, options: zsh::parser::ParserOptions) -> (bool, Vec<zsh::parser::Token>) {
        zsh::parser::parse(string, options)
    }

    pub fn get_prompt(&self, prompt: Option<&MetaStr>, escaped: bool) -> Option<MetaString> {
        zsh::get_prompt(prompt, escaped)
    }

    pub fn get_prompt_size(&self, prompt: &MetaStr, term_width: Option<c_long>) -> (usize, usize) {
        let (width, height) = zsh::get_prompt_size(prompt, term_width);
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
        zsh::signals::sigchld::check_pid_status(pid)
    }

    pub fn get_var(&self, name: &MetaStr, zle: bool) -> anyhow::Result<Option<variables::Value>> {
        if zle {
            self.start_zle_scope();
        }
        let result = if let Some(mut v) = Variable::get(name) {
            v.as_value().map(Some)
        } else {
            Ok(None)
        };
        if zle {
            self.end_zle_scope();
        }
        result
    }

    pub fn get_vars<'a, T: 'a + AsRef<MetaStr>, I: Iterator<Item=&'a T>>(
        &self,
        names: I,
        zle: bool,
    ) -> anyhow::Result<Vec<Option<variables::Value>>> {
        if zle {
            self.start_zle_scope();
        }
        let results = names.map(|name| {
            if let Some(mut v) = Variable::get(name.as_ref()) {
                v.as_value().map(Some)
            } else {
                Ok(None)
            }
        }).collect::<Result<Vec<_>>>();
        if zle {
            self.end_zle_scope();
        }
        results
    }

    pub fn get_var_as_string(&self, name: &MetaStr, zle: bool) -> anyhow::Result<Option<BString>> {
        if zle {
            self.start_zle_scope();
        }
        let result = Variable::get(name).map(|mut v| v.as_bytes());
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

    pub fn set_var(&self, name: &MetaStr, value: variables::Value, local: bool) -> anyhow::Result<()> {
        Variable::set(name, value, local)
    }

    pub fn unset_var(&self, name: &MetaStr) {
        Variable::unset(name);
    }

    pub fn export_var(&self, name: &MetaStr) -> bool {
        if let Some(var) = Variable::get(name) {
            var.export();
            true
        } else {
            false
        }
    }

    pub fn create_dynamic_string_var(
        &self,
        name: &MetaStr,
        get: Box<dyn Fn() -> BString>,
        set: Option<Box<dyn Fn(BString)>>,
        unset: Option<Box<dyn Fn(bool)>>
    ) -> Result<()> {
        Variable::create_dynamic(name, get, set, unset)
    }

    pub fn create_dynamic_integer_var(
        &self,
        name: &MetaStr,
        get: Box<dyn Fn() -> c_long>,
        set: Option<Box<dyn Fn(c_long)>>,
        unset: Option<Box<dyn Fn(bool)>>
    ) -> Result<()> {
        Variable::create_dynamic(name, get, set, unset)
    }

    pub fn create_dynamic_float_var(
        &self,
        name: &MetaStr,
        get: Box<dyn Fn() -> f64>,
        set: Option<Box<dyn Fn(f64)>>,
        unset: Option<Box<dyn Fn(bool)>>
    ) -> Result<()> {
        Variable::create_dynamic(name, get, set, unset)
    }

    pub fn create_dynamic_array_var(
        &self,
        name: &MetaStr,
        get: Box<dyn Fn() -> Vec<BString>>,
        set: Option<Box<dyn Fn(Vec<BString>)>>,
        unset: Option<Box<dyn Fn(bool)>>
    ) -> Result<()> {
        Variable::create_dynamic(name, get, set, unset)
    }

    pub fn create_dynamic_hash_var(
        &self,
        name: &MetaStr,
        get: Box<dyn Fn() -> HashMap<BString, BString>>,
        set: Option<Box<dyn Fn(HashMap<BString, BString>)>>,
        unset: Option<Box<dyn Fn(bool)>>
    ) -> Result<()> {
        Variable::create_dynamic(name, get, set, unset)
    }

    pub fn goto_history(&self, index: history::HistoryIndex, skipdups: bool) {
        history::History::goto(index, skipdups);
    }

    pub fn append_history(&self, text: BString) -> Result<()> {
        history::History::append(text)
    }

    pub fn append_history_words(&self, words: Vec<BString>) -> Result<()> {
        history::History::append_words(words)
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

    pub fn exit(&self, code: i32) {
        zsh::exit(code);
    }

    pub fn acceptline(&self) {
        unsafe { zsh::acceptline(); }
    }

    pub fn has_accepted_line(&self) -> bool {
        unsafe{ zsh::done != 0 }
    }

    pub fn make_function(&self, code: &MetaStr) -> Result<Rc<zsh::functions::Function>> {
        let func = zsh::functions::Function::new(code)?;
        Ok(Rc::new(func))
    }

    pub fn exec_function<'a, T: 'a + AsRef<MetaStr>, I: Iterator<Item=&'a T>>(
        &self,
        _token: TrampolineToken,
        function: Rc<zsh::functions::Function>,
        arg0: Option<&'a MetaStr>,
        args: I,
    ) -> c_int {
        function.execute(arg0, args)
    }

    pub fn exec_function_by_name<'a, T: 'a + AsRef<MetaStr>, I: Iterator<Item=&'a T>>(
        &self,
        _token: TrampolineToken,
        function: &'a MetaStr,
        args: I,
    ) -> Option<c_int> {
        zsh::functions::Function::execute_by_name(function, args)
    }

    pub fn get_function_source(&self, function: Rc<zsh::functions::Function>) -> BString {
        function.get_source()
    }

    pub fn queue_signal_level(&self) -> i32 {
        zsh::queue_signal_level()
    }

    pub fn with_queued_signals<T, F: FnOnce() -> T>(&self, func: F) -> (T, nix::Result<()>) {
        zsh::with_queued_signals(func)
    }

    pub fn dont_queue_signals(&self) -> nix::Result<()> {
        zsh::dont_queue_signals()
    }

    pub fn restore_queue_signals(&self, level: i32) {
        zsh::restore_queue_signals(level);
    }

    pub fn call_hook_func<'a, T: 'a + AsRef<MetaStr>, I: Iterator<Item=&'a T>>(
        &self,
        name: &'a MetaStr,
        args: I,
    ) -> Option<c_int> {
        // needs metafy
        zsh::call_hook_func(name, args)
    }

    pub fn winch_block(&self) {
        zsh::winch_block()
    }

    pub fn winch_unblock(&self) {
        zsh::winch_unblock()
    }

}
