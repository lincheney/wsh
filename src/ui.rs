use bstr::{BStr, BString};
use std::future::Future;
use std::sync::{Arc, Weak as WeakArc, atomic::{AtomicUsize, AtomicBool, Ordering}};
use std::collections::HashSet;
use std::default::Default;
use mlua::prelude::*;
use anyhow::Result;
use crate::keybind::parser::{Event, KeyEvent, Key, KeyModifiers};
use crate::fork_lock::{ForkLock, RawForkLock, ForkLockReadGuard};
use nix::sys::termios;

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::MoveToColumn,
    event,
    style,
    execute,
    queue,
};
use crate::tui::{
    MoveDown,
};

use crate::timed_lock::{RwLock};
use crate::shell::{ShellClient, KeybindValue};
use crate::lua::{EventCallbacks, HasEventCallbacks};

const UNHANDLED_OUTPUT: usize = 1;
const UI_FROZEN: usize = UNHANDLED_OUTPUT | 2;

fn lua_error<T>(msg: &str) -> Result<T, mlua::Error> {
    Err(mlua::Error::RuntimeError(msg.to_string()))
}

struct SetScrollRegion(u16, u16);
impl crossterm::Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{};{}r", self.0, self.1)
    }
}

enum KeybindOutput {
    String(BString),
    Value(Result<bool>),
}

pub struct UiInner {
    pub tui: crate::tui::Tui,
    pub cmdline: crate::tui::command_line::CommandLineState,

    pub dirty: bool,
    pub keybinds: Vec<crate::lua::KeybindMapping>,
    pub keybind_layer_counter: usize,

    pub buffer: crate::buffer::Buffer,
    pub status_bar: crate::tui::status_bar::StatusBar,

    pub stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    size: (u32, u32),
}

pub struct UnlockedUi {
    pub inner: RwLock<UiInner>,
    pub event_callbacks: std::sync::Mutex<EventCallbacks> ,
}

impl UnlockedUi {
    pub fn borrow(&self) -> tokio::sync::RwLockReadGuard<'_, UiInner> {
        self.inner.blocking_read()
    }

    pub fn borrow_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, UiInner> {
        self.inner.blocking_write()
    }
}


crate::strong_weak_wrapper! {
    pub struct Ui {
        pub unlocked: Arc::<ForkLock<'static, UnlockedUi>> [WeakArc::<ForkLock<'static, UnlockedUi>>],
        pub shell: Arc::<ShellClient> [WeakArc::<ShellClient>],
        pub lua: Arc::<Lua> [WeakArc::<Lua>],
        pub events: Arc::<ForkLock<'static, crate::event_stream::EventController>> [WeakArc::<ForkLock<'static, crate::event_stream::EventController>>],
        pub has_foreground_process: Arc::<tokio::sync::Mutex<()>> [WeakArc::<tokio::sync::Mutex<()>>],
        preparing_for_unhandled_output: Arc::<AtomicUsize> [WeakArc::<AtomicUsize>],
        threads: Arc::<ForkLock<'static, std::sync::Mutex<HashSet<nix::unistd::Pid>>>> [WeakArc::<ForkLock<'static, std::sync::Mutex<HashSet<nix::unistd::Pid>>>>],
        is_drawing: Arc::<AtomicBool> [WeakArc::<AtomicBool>],
    }
}

impl Ui {

    pub fn new(
        lock: &'static RawForkLock,
        events: crate::event_stream::EventController,
        shell: ShellClient,
    ) -> Result<Self> {

        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;

        let mut ui = UiInner{
            dirty: true,
            tui: Default::default(),
            cmdline: Default::default(),
            buffer: crate::buffer::Buffer::new(),
            status_bar: Default::default(),
            keybinds: Default::default(),
            keybind_layer_counter: Default::default(),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            size: (1, 1),
        };
        ui.keybinds.push(Default::default());

        ui.reset();

        let ui = UnlockedUi {
            inner: RwLock::new(ui),
            event_callbacks: Default::default(),
        };
        let ui = Self {
            unlocked: Arc::new(lock.wrap(ui)),
            lua: Arc::new(lua),
            events: Arc::new(lock.wrap(events)),
            shell: Arc::new(shell),
            has_foreground_process: Default::default(),
            preparing_for_unhandled_output: Default::default(),
            threads: Arc::new(lock.wrap(std::sync::Mutex::new(HashSet::new()))),
            is_drawing: Arc::new(false.into()),
        };

        Ok(ui)
    }


    pub fn get(&self) -> ForkLockReadGuard<'_, UnlockedUi> {
        self.unlocked.read()
    }

    pub fn get_lua_api(&self) -> LuaResult<LuaTable> {
        self.lua.globals().get("wish")
    }

    pub async fn start_cmd(&self) -> Result<()> {
        self.trigger_precmd_callbacks(()).await;
        self.draw().await
    }

    pub fn queue_draw(&self) {
        if !crate::is_forked() && !self.is_drawing.swap(true, Ordering::AcqRel) {
            self.events.read().queue_draw();
        }
    }

    pub async fn draw(&self) -> Result<()> {
        if self.preparing_for_unhandled_output.load(Ordering::Relaxed) != 0 {
            // the shell will draw it later
            return Ok(())
        }

        self.is_drawing.store(false, Ordering::Release);

        fn draw_internal(ui: &mut UiInner, shell_vars: Option<crate::tui::command_line::ShellVars>) -> Result<()> {
            if let Some(shell_vars) = shell_vars {
                ui.cmdline.shell_vars = shell_vars;
            }
            let cmdline = ui.cmdline.make_command_line(&mut ui.buffer);
            ui.tui.draw(
                &mut ui.stdout,
                ui.size,
                cmdline,
                &mut ui.status_bar,
                ui.dirty,
            )?;
            ui.dirty = false;
            Ok(())
        }

        let size;
        {
            let this = self.unlocked.read();
            let ui = &mut *this.borrow_mut();

            size = ui.size;
            let (width, height) = size;
            // take up at most 2/3 of the screen
            let height = (height * 2 / 3).max(1);
            // redraw all if dimensions have changed
            if height != ui.tui.max_height || width != ui.tui.get_size().0 as _ {
                ui.tui.max_height = height;
                ui.dirty = true;
            }

            if !(ui.dirty || ui.buffer.dirty || ui.tui.dirty || ui.status_bar.dirty) {
                return Ok(())
            }

            if !(ui.dirty || ui.cmdline.is_dirty()) {
                // don't need the shell vars, draw immediately
                return draw_internal(ui, None)
            }
        }

        // get the shell vars then reacquire the ui
        let shell_vars = crate::tui::command_line::CommandLineState::get_shell_vars(&self.shell, size.0).await;

        let this = self.unlocked.read();
        let ui = &mut *this.borrow_mut();
        draw_internal(ui, Some(shell_vars))
    }

    pub async fn call_lua_fn<T: IntoLuaMulti + mlua::MaybeSend + 'static>(&self, draw: bool, callback: mlua::Function, arg: T) {
        let result = callback.call_async::<LuaValue>(arg).await;
        let mut ui = self.clone();
        if !ui.report_error(result).await && draw {
            ui.queue_draw();
        }
    }

    pub async fn report_error<T, E: std::fmt::Display>(&mut self, result: std::result::Result<T, E>) -> bool {
        if let Err(err) = result {
            log::error!("{}", err);
            self.show_error_message(format!("ERROR: {err}")).await;
            true
        } else {
            false
        }
    }

    pub async fn show_error_message(&mut self, msg: String) {
        let this = self.get();
        let mut ui = this.borrow_mut();
        ui.tui.add_error_message(msg);
        self.queue_draw();
    }

    pub async fn handle_event(&mut self, event: Event, event_buffer: BString) -> Result<bool> {
        if let Event::Key(event) = event {
            self.trigger_key_callbacks(event.into()).await;
            if let Some(result) = self.handle_key(event, event_buffer.as_ref()).await {
                return result
            }

        }
        self.handle_key_default(event, event_buffer.as_ref()).await?;
        Ok(true)
    }

    pub async fn handle_window_resize(&self, width: u32, height: u32) -> Result<bool> {
        self.get().borrow_mut().size = (width, height);
        self.queue_draw();
        self.trigger_window_resize_callbacks((width, height)).await;
        Ok(true)
    }

    pub async fn pre_accept_line(&self) -> Result<()> {
        {
            let this = &*self.unlocked.read();
            let mut ui = this.borrow_mut();

            ui.tui.clear_non_persistent();

            // TODO handle errors here properly
        }
        self.events.read().pause();
        self.prepare_for_unhandled_output(None).await?;
        Ok(())
    }

    pub async fn prepare_for_unhandled_output(&self, flag: Option<usize>) -> Result<bool> {
        // TODO if forked and trashed, zsh will NOT recover
        // we're going go to end up with janky output
        // how do we solve this?
        if !crate::is_forked() && self.preparing_for_unhandled_output.fetch_or(flag.unwrap_or(UNHANDLED_OUTPUT), Ordering::Relaxed) == 0 {
            self.unlocked.read().borrow_mut().prepare_for_unhandled_output()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn recover_from_unhandled_output(&self, flag: Option<usize>) -> Result<bool> {
        let flag = flag.unwrap_or(UNHANDLED_OUTPUT);
        let old_flag = self.preparing_for_unhandled_output.fetch_and(!flag, Ordering::Relaxed);
        if old_flag == 0 {
            Ok(false)
        } else if old_flag & !flag != 0 || self.has_foreground_process.try_lock().is_err() {
            // foreground process, can't recover yet
            // reset it back
            self.preparing_for_unhandled_output.store(old_flag, Ordering::Relaxed);
            Ok(false)
        } else {

            {
                let this = self.unlocked.read();
                let ui = this.borrow();
                ui.activate()?;
            }

            // move down one line if not at start of line
            let cursor = self.events.read().get_cursor_position();
            let cursor = tokio::time::timeout(crate::timed_lock::DEFAULT_DURATION, cursor).await.unwrap().unwrap_or((0, 0));

            let this = self.unlocked.read();
            let mut ui = this.borrow_mut();
            if cursor.0 != 0 {
                queue!(ui.stdout, style::Print("\r\n"))?;
            }
            execute!(ui.stdout, style::ResetColor)?;
            ui.dirty = true;
            Ok(true)
        }
    }

    pub async fn post_accept_line(&self) -> Result<()> {
        {
            let this = &*self.unlocked.read();
            let mut ui = this.borrow_mut();
            ui.reset();
        }
        self.events.read().unpause();
        self.recover_from_unhandled_output(None).await?;
        Ok(())
    }

    pub async fn accept_line(&mut self) -> Result<bool> {
        if crate::is_forked() {
            return Ok(false)
        }

        let buffer = {
            let buffer = {
                let this = self.get();
                let ui = this.borrow();
                ui.buffer.get_contents().clone()
            };
            let (complete, _tokens) = self.shell.parse(buffer.clone(), Default::default()).await;
            if complete {
                Some(buffer)
            } else {
                None
            }
        };

        // time to execute
        if let Some(buffer) = buffer {
            self.trigger_accept_line_callbacks(()).await;
            {
                let lock = self.has_foreground_process.lock().await;
                // last draw
                crate::log_if_err(self.draw().await);
                self.pre_accept_line().await?;
                // acceptline doesn't actually accept the line right now
                // only when we return control to zle using the trampoline
                if self.shell.do_accept_line_trampoline(Some(buffer)).await.is_err() {
                    return Ok(false)
                }
                drop(lock);
            }
            self.post_accept_line().await?;
            self.start_cmd().await?;

        } else {
            self.get().borrow_mut().buffer.insert_at_cursor(b"\n");
            self.trigger_buffer_change_callbacks(()).await;
            self.draw().await?;
        }

        Ok(true)
    }

    pub fn set_lua_fn<F, A, R>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(&Self, &Lua, A) -> Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {

        let func = self.make_lua_fn(func)?;
        self.get_lua_api()?.set(name, func)
    }

    pub fn make_lua_fn<F, A, R>(&self, func: F) -> LuaResult<LuaFunction>
    where
        F: Fn(&Self, &Lua, A) -> Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
        let weak = self.downgrade();
        self.lua.create_function(move |lua, value| {
            let ui = weak.try_upgrade()?;
            func(&ui, lua, value)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })
    }

    pub fn set_lua_async_fn<F, A, R, T>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(Self, Lua, A) -> T + mlua::MaybeSend + 'static + Clone,
        A: FromLuaMulti + Send + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + mlua::MaybeSend + 'static,
    {
        let func = self.make_lua_async_fn(func)?;
        self.get_lua_api()?.set(name, func)
    }

    pub fn make_lua_async_fn<F, A, R, T>(&self, func: F) -> LuaResult<LuaFunction>
    where
        F: Fn(Self, Lua, A) -> T + mlua::MaybeSend + 'static + Clone,
        A: FromLuaMulti + Send + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + mlua::MaybeSend + 'static,
    {
        let weak = self.downgrade();
        self.lua.create_async_function(move |lua, value| {
            let weak = weak.clone();
            let func = func.clone();
            async move {
                let ui = weak.try_upgrade()?;
                func(ui, lua, value).await
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
            }
        })
    }

    pub fn init_lua(&self) -> Result<()> {
        crate::lua::init_lua(self)?;

        self.lua.load("package.path = '/home/qianli/Documents/wish/lua/?.lua;' .. package.path").exec()?;
        if let Err(err) = self.lua.load("require('wish')").exec() {
            log::error!("{}", err);
        }

        Ok(())
    }

    async fn handle_key(&mut self, event: KeyEvent, buf: &BStr) -> Option<Result<bool>> {
        let mut mapping = match self.handle_key_simple(event, buf).await? {
            KeybindOutput::Value(x) => return Some(x),
            KeybindOutput::String(string) => string,
        };

        // shucks, gotta do recursion
        let mut success = true;
        for _hop in 0..20 {
            let mut parser = crate::keybind::parser::Parser::new();
            parser.feed(mapping.as_ref());
            mapping.clear();
            for (event, buf) in parser.iter() {
                if let Event::Key(event) = event {
                    match self.handle_key_simple(event, buf.as_ref()).await {
                        Some(KeybindOutput::Value(x)) => {
                            if let Ok(x) = x {
                                success = success && x;
                                continue
                            }
                            return Some(x) // error
                        },
                        Some(KeybindOutput::String(mut string)) => {
                            mapping.append(&mut string);
                            continue
                        },
                        None => (),
                    }
                }

                let x = self.handle_key_default(event, buf.as_ref()).await;
                if let Ok(x) = x {
                    success = success && x;
                } else {
                    return Some(x) // error
                }
            }

            if mapping.is_empty() {
                return Some(Ok(success))
            }
        }

        // TODO we still have a mapping
        // let mut ui = self.inner.borrow_mut();
        // ui.buffer.insert_at_cursor(string.as_ref());
        // return Some(Ok(true))

        Some(Ok(true))
    }

    async fn handle_key_simple(&mut self, event: KeyEvent, buf: &BStr) -> Option<KeybindOutput> {
        // look for a lua callback
        let callback = self.get().borrow().keybinds
            .iter()
            .rev()
            .find_map(|k| k.inner.get(&(event.key, event.modifiers)))
            .cloned();
        if let Some(callback) = callback {
            self.call_lua_fn(true, callback, ()).await;
            return Some(KeybindOutput::Value(Ok(true)))
        }

        enum Value {
            String(BString),
            Widget{buffer: Option<BString>, cursor: Option<usize>, output: Option<BString>, accept_line: bool},
        }

        // look for a zle widget
        let ui = self.clone();
        let buf = buf.to_owned();
        let result = self.shell.do_run(move |shell| {
            match KeybindValue::find(shell, buf.as_ref()) {
                Some(KeybindValue::String(string)) => {
                    // recurse
                    Some(Value::String(string))
                },
                // skip not found or where we have our own impl
                Some(KeybindValue::Widget(widget)) if widget.is_self_insert() || widget.is_undefined_key() => None,
                Some(KeybindValue::Widget(widget)) if widget.is_accept_line() => {
                    Some(Value::Widget{accept_line: true, buffer: None, cursor: None, output: None})
                },
                Some(KeybindValue::Widget(mut widget)) => {
                    // execute the widget
                    // a widget may run subprocesses so lock the ui
                    let lock = ui.has_foreground_process.blocking_lock();
                    let this = ui.get();
                    let ui = this.inner.blocking_write();
                    let buffer = ui.buffer.get_contents();
                    let cursor = ui.buffer.get_cursor();

                    widget.shell.set_zle_buffer(buffer.clone(), cursor as _);

                    let mut lastchar = [0; 4];
                    let len = buf.len().min(lastchar.len());
                    lastchar[..len].copy_from_slice(&buf[..len]);

                    widget.shell.set_lastchar(lastchar);
                    // executing a widget may block
                    let (output, _) = tokio::task::block_in_place(|| widget.exec_and_get_output(None, [].into_iter())).unwrap();
                    let (new_buffer, new_cursor) = shell.get_zle_buffer();
                    let new_cursor = new_cursor.unwrap_or(new_buffer.len() as _) as _;
                    let new_buffer = (new_buffer != *buffer).then_some(new_buffer);
                    let new_cursor = (new_cursor != cursor).then_some(new_cursor);
                    let accept_line = shell.has_accepted_line();
                    drop(lock);

                    Some(Value::Widget{
                        buffer: new_buffer,
                        cursor: new_cursor,
                        output: if output.is_empty() { None } else { Some(output) },
                        accept_line,
                    })
                },
                None => None,
            }
        }).await;

        match result? {
            Value::String(string) => Some(KeybindOutput::String(string)),
            Value::Widget{buffer, cursor, output, accept_line} => {
                {
                    let this = self.get();
                    let mut ui = this.borrow_mut();

                    // check for any output e.g. zle -M
                    if let Some(output) = &output {
                        ui.tui.add_zle_message(output.as_ref());
                    }
                    ui.buffer.insert_or_set(buffer.as_ref().map(|x| x.as_ref()), cursor);
                    // anything could have happened, so trigger a redraw
                    ui.dirty = true;
                }

                if buffer.is_some() {
                    self.trigger_buffer_change_callbacks(()).await;
                }
                // anything could have happened, so trigger a redraw
                self.queue_draw();

                // this widget may have called accept-line somewhere inside
                if accept_line {
                    Some(KeybindOutput::Value(self.accept_line().await))
                } else {
                    Some(KeybindOutput::Value(Ok(true)))
                }
            },
        }
    }

    async fn handle_key_default(&mut self, event: Event, _buf: &BStr) -> Result<bool> {
        match event {

            Event::Key(KeyEvent{ key: Key::Escape, .. }) => {
                self.cancel()?;
            },

            Event::Key(KeyEvent{ key: Key::Char(c), modifiers }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                {
                    let mut buf = [0; 4];
                    let this = self.get();
                    let mut ui = this.borrow_mut();
                    ui.buffer.insert_at_cursor(c.encode_utf8(&mut buf).as_bytes());
                }
                self.trigger_buffer_change_callbacks(()).await;
                self.queue_draw();
            },

            Event::Key(KeyEvent{ key: Key::Enter, modifiers }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                return self.accept_line().await;
            },

            Event::BracketedPaste(data) => {
                let data = self.lua.create_string(data)?;
                self.trigger_paste_callbacks(data).await;
            },

            _ => (),
        }
        Ok(true)
    }

    pub fn add_thread(&self, id: nix::unistd::Pid) {
        self.threads.read().lock().unwrap().insert(id);
    }

    pub fn remove_thread(&self, id: nix::unistd::Pid) {
        self.threads.read().lock().unwrap().remove(&id);
    }

    fn cancel(&self) -> Result<()> {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(0), nix::sys::signal::Signal::SIGTERM)?;
        for pid in self.threads.read().lock().unwrap().iter() {
            nix::sys::signal::kill(*pid, nix::sys::signal::Signal::SIGINT)?;
        }
        Ok(())
    }

    pub async fn freeze_if<T, F: Future<Output = T>>(
        &self,
        condition: bool,
        freeze_events: bool,
        f: F,
    ) -> Result<T> {

        let mut lock = if condition && !crate::is_forked() {
            // this essentially locks ui
            if freeze_events {
                self.events.read().pause();
            }
            self.prepare_for_unhandled_output(Some(UI_FROZEN)).await?;
            Some(self.has_foreground_process.lock().await)
        } else {
            None
        };

        let result = f.await;

        if let Some(lock) = lock.take() {
            drop(lock);
            if freeze_events {
                self.events.read().unpause();
            }
            crate::log_if_err(self.recover_from_unhandled_output(Some(UI_FROZEN)).await);
        }

        Ok(result)
    }

}

impl UiInner {
    pub fn activate(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;

        // onlcr in case bg processes are outputting things
        let mut attrs = termios::tcgetattr(&self.stdout)?;
        attrs.output_flags.insert(termios::OutputFlags::OPOST | termios::OutputFlags::ONLCR);
        nix::sys::termios::tcsetattr(&self.stdout, termios::SetArg::TCSADRAIN, &attrs)?;

        if self.enhanced_keyboard {
            // queue!(
                // self.stdout.lock(),
                // event::PushKeyboardEnhancementFlags(
                    // event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    // | event::KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    // | event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                    // | event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                // )
            // )?;
        }

        execute!(
            self.stdout.lock(),
            event::EnableBracketedPaste,
            event::EnableFocusChange,
            // event::EnableMouseCapture,
        )?;
        Ok(())
    }

    pub fn deactivate(&mut self) -> Result<()> {
        if self.enhanced_keyboard {
            // queue!(self.stdout, event::PopKeyboardEnhancementFlags)?;
        }

        execute!(
            self.stdout,
            event::DisableBracketedPaste,
            event::DisableFocusChange,
            // event::DisableMouseCapture
        )?;

        crossterm::terminal::disable_raw_mode()?;
        Ok(())
    }

    fn reset(&mut self) {
        self.buffer.reset();
        self.tui.reset();
        self.status_bar.reset();
        self.dirty = true;
    }

    fn prepare_for_unhandled_output(&mut self) -> Result<()> {
        self.deactivate()?;
        self.dirty = true;
        // move to last line of buffer
        let y_offset = self.cmdline.y_offset_to_end();
        execute!(
            self.stdout,
            BeginSynchronizedUpdate,
            MoveDown(y_offset),
            style::Print('\n'),
            MoveToColumn(0),
            Clear(ClearType::FromCursorDown),
            EndSynchronizedUpdate,
        )?;
        Ok(())
    }

}

impl Drop for UiInner {
    fn drop(&mut self) {
        if let Err(err) = self.deactivate() {
            log::error!("{:?}", err);
        }
    }
}

impl WeakUi {
    pub fn try_upgrade(&self) -> LuaResult<Ui> {
        if let Some(ui) = self.upgrade() {
            Ok(ui)
        } else {
            lua_error("ui not running")
        }
    }
}
