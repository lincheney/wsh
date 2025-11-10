use bstr::{BStr, BString, ByteSlice};
use std::time::Duration;
use std::future::Future;
use std::sync::{Arc, Weak as WeakArc};
use std::collections::HashSet;
use std::default::Default;
use mlua::prelude::*;
use tokio::sync::{RwLock, Notify, mpsc};
use anyhow::Result;
use crate::keybind::parser::{Event, KeyEvent, Key, KeyModifiers};
use crate::fork_lock::{ForkLock, RawForkLock, ForkLockReadGuard};

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

use crate::shell::{Shell, ShellInner, WeakShell, UpgradeShell, KeybindValue, DowngradeShell};
use crate::lua::{EventCallbacks, HasEventCallbacks};

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

    pub events: crate::event_stream::EventController,
    pub dirty: bool,
    pub keybinds: Vec<crate::lua::KeybindMapping>,
    pub keybind_layer_counter: usize,

    pub buffer: crate::buffer::Buffer,
    pub prompt: crate::prompt::Prompt,
    pub status_bar: crate::tui::status_bar::StatusBar,

    pub threads: HashSet<nix::unistd::Pid>,
    stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    size: (u16, u16),
}

pub trait ThreadsafeUiInner {
    async fn borrow(&self) -> tokio::sync::RwLockReadGuard<'_, UiInner>;
    async fn borrow_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, UiInner>;
}

impl ThreadsafeUiInner for RwLock<UiInner> {
    async fn borrow(&self) -> tokio::sync::RwLockReadGuard<'_, UiInner> {
        self.read().await
    }

    async fn borrow_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, UiInner> {
        self.write().await
    }
}

struct TrampolineOut {
    out_sender: mpsc::UnboundedSender<BString>,
    in_notify: Arc<Notify>,
}

pub struct TrampolineIn {
    out_receiver: mpsc::UnboundedReceiver<BString>,
    in_notify: Arc<Notify>,
}

impl TrampolineOut {
    pub async fn jump_out(&self, shell: &mut ShellInner<'_>, string: BString) -> Result<()> {
        self.out_sender.send(string)?;
        self.in_notify.notified().await;
        let _ = shell;
        Ok(())
    }
}

impl TrampolineIn {
    pub async fn jump_in(&mut self, notify: bool) -> Option<BString> {
        if notify {
            self.in_notify.notify_one();
        }
        self.out_receiver.recv().await
    }
}

fn new_trampoline() -> (TrampolineIn, TrampolineOut) {
    let (out_sender, out_receiver) = mpsc::unbounded_channel();
    let in_notify = Arc::new(Notify::new());
    (TrampolineIn{out_receiver, in_notify: in_notify.clone()}, TrampolineOut{out_sender, in_notify})
}

pub struct UnlockedUi {
    pub inner: RwLock<UiInner>,
    pub event_callbacks: std::sync::Mutex<EventCallbacks> ,
}


crate::strong_weak_wrapper! {
    pub struct Ui {
        pub unlocked: Arc::<ForkLock<'static, UnlockedUi>> [WeakArc::<ForkLock<'static, UnlockedUi>>],
        pub shell: Shell [WeakShell],
        pub lua: Arc::<Lua> [WeakArc::<Lua>],
        pub has_foreground_process: Arc::<tokio::sync::Mutex<()>> [WeakArc::<tokio::sync::Mutex<()>>],
        // trampoline should not be locked
        trampoline: Arc::<TrampolineOut> [WeakArc::<TrampolineOut>],
    }
}

impl Ui {

    pub async fn new(
        lock: &'static RawForkLock,
        events: crate::event_stream::EventController,
    ) -> Result<(Self, TrampolineIn)> {

        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;

        let mut ui = UiInner{
            events,
            dirty: true,
            tui: Default::default(),
            threads: HashSet::new(),
            buffer: Default::default(),
            prompt: crate::prompt::Prompt::new(None),
            status_bar: Default::default(),
            keybinds: Default::default(),
            keybind_layer_counter: Default::default(),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            size: crossterm::terminal::size()?,
        };
        ui.keybinds.push(Default::default());

        let shell = Shell::new();
        ui.reset(&mut shell.lock().await);

        let trampoline = new_trampoline();
        let ui = UnlockedUi {
            inner: RwLock::new(ui),
            event_callbacks: Default::default(),
        };
        let ui = Self {
            unlocked: Arc::new(lock.wrap(ui)),
            lua: Arc::new(lua),
            shell,
            has_foreground_process: Default::default(),
            trampoline: Arc::new(trampoline.1),
        };

        ui.init_lua().await?;

        Ok((ui, trampoline.0))
    }


    pub fn get(&self) -> ForkLockReadGuard<'_, UnlockedUi> {
        self.unlocked.read()
    }

    pub async fn activate(&self) -> Result<()> {
        self.get().inner.borrow().await.activate()
    }

    pub async fn deactivate(&mut self) -> Result<()> {
        self.get().inner.borrow_mut().await.deactivate()
    }

    pub fn get_lua_api(&self) -> LuaResult<LuaTable> {
        self.lua.globals().get("wish")
    }

    pub async fn start_cmd(&mut self) -> Result<()> {
        self.trigger_precmd_callbacks(()).await;
        self.draw().await
    }

    pub async fn draw(&mut self) -> Result<()> {
        // do NOT render ui elements if there is a foreground process
        if crate::is_forked() {
            return Ok(())
        }

        let Ok(_lock) = self.has_foreground_process.try_lock()
        else {
            return Ok(())
        };

        let this = &*self.unlocked.read();
        let mut ui = this.inner.borrow_mut().await;
        let ui = &mut *ui;

        if !(ui.dirty || ui.buffer.dirty || ui.prompt.dirty || ui.tui.dirty) {
            return Ok(())
        }

        crossterm::terminal::disable_raw_mode()?;
        ui.size = crossterm::terminal::size()?;
        ui.tui.draw(
            &mut ui.stdout,
            ui.size,
            &self.shell,
            &mut ui.prompt,
            &mut ui.buffer,
            &mut ui.status_bar,
            ui.dirty,
        ).await?;
        crossterm::terminal::enable_raw_mode()?;

        ui.dirty = false;
        Ok(())
    }

    pub async fn call_lua_fn<T: IntoLuaMulti + mlua::MaybeSend + 'static>(&self, draw: bool, callback: mlua::Function, arg: T) {
        let result = callback.call_async::<LuaValue>(arg).await;
        let mut ui = self.clone();
        tokio::task::spawn(async move {
            ui.report_error(draw, result).await;
        });
    }

    pub async fn report_error<T, E: std::fmt::Display>(&mut self, draw: bool, result: std::result::Result<T, E>) {
        if let Err(err) = result {
            log::error!("{}", err);
            self.show_error_message(format!("ERROR: {err}")).await;
        } else if draw && let Err(err) = self.draw().await {
            log::error!("{:?}", err);
        }
    }

    pub async fn show_error_message(&mut self, msg: String) {
        {
            let this = self.get();
            let mut ui = this.inner.borrow_mut().await;
            ui.tui.add_error_message(msg);
        }

        if let Err(err) = self.draw().await {
            log::error!("{:?}", err);
        }
    }

    pub async fn handle_event(&mut self, event: Event, event_buffer: BString) -> Result<bool> {
        if let Event::Key(event) = event {
            self.trigger_key_callbacks(event.into()).await;
            if let Some(result) = self.handle_key(event, event_buffer.as_ref()).await {
                let result = result?;
                self.trigger_buffer_change_callbacks(()).await;
                self.draw().await?;
                return Ok(result)
            }

        }
        self.handle_key_default(event, event_buffer.as_ref()).await?;
        Ok(true)
    }

    pub async fn pre_accept_line(&self, shell: &mut ShellInner<'_>) -> Result<()> {
        let this = &*self.unlocked.read();
        let mut ui = this.inner.borrow_mut().await;

        ui.tui.clear_non_persistent();

        // TODO handle errors here properly
        ui.events.pause().await;
        shell.clear_completion_cache();
        ui.prepare_for_unhandled_output()?;
        Ok(())
    }

    pub async fn post_accept_line(&self, shell: &mut ShellInner<'_>) -> Result<()> {
        let this = &*self.unlocked.read();
        let mut ui = this.inner.borrow_mut().await;
        ui.events.resume().await;
        ui.reset(shell);
        ui.recover_from_unhandled_output().await?;
        Ok(())
    }

    pub async fn accept_line(&mut self) -> Result<bool> {
        if crate::is_forked() {
            return Ok(false)
        }

        let buffer = {
            let this = self.get();
            let ui = this.inner.borrow().await;
            let buffer = ui.buffer.get_contents();
            let (complete, _tokens) = self.shell.lock().await.parse(buffer.as_ref(), false);
            if complete {
                Some(buffer.clone())
            } else {
                None
            }
        };

        // time to execute
        if let Some(buffer) = buffer {
            self.trigger_accept_line_callbacks(()).await;
            let lock = self.has_foreground_process.lock().await;
            let mut shell = self.shell.lock().await;
            self.pre_accept_line(&mut shell).await?;
            // acceptline doesn't actually accept the line right now
            // only when we return control to zle using the trampoline
            self.trampoline.jump_out(&mut shell, buffer).await?;
            self.post_accept_line(&mut shell).await?;

            // prefer the result error over the activate error
            // result.and(ui.activate())?;
            drop(shell);
            drop(lock);
            self.start_cmd().await?;

        } else {
            self.get().inner.borrow_mut().await.buffer.insert_at_cursor(b"\n");
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
                .map_err(|e| mlua::Error::RuntimeError(format!("{e}")))
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
                    .map_err(|e| mlua::Error::RuntimeError(format!("{e}")))
            }
        })
    }

    async fn init_lua(&self) -> Result<()> {
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
        // let mut ui = self.inner.borrow_mut().await;
        // ui.buffer.insert_at_cursor(string.as_ref());
        // return Some(Ok(true))

        Some(Ok(true))
    }

    async fn handle_key_simple(&mut self, event: KeyEvent, buf: &BStr) -> Option<KeybindOutput> {
        // look for a lua callback
        let callback = self.get().inner.borrow().await.keybinds
            .iter()
            .rev()
            .find_map(|k| k.inner.get(&(event.key, event.modifiers)))
            .cloned();
        if let Some(callback) = callback {
            self.call_lua_fn(true, callback, ()).await;
            return Some(KeybindOutput::Value(Ok(true)))
        }

        let guard = self.unlocked.read();
        let this = &*guard;
        let mut shell = self.shell.lock().await;
        // look for a zle widget
        match shell.get_keybinding(buf)? {
            KeybindValue::String(string) => {
                // recurse
                Some(KeybindOutput::String(string))
            },
            // skip not found or where we have our own impl
            KeybindValue::Widget(widget) if widget.is_self_insert() || widget.is_undefined_key() => None,
            KeybindValue::Widget(widget) if widget.is_accept_line() => {
                drop(shell);
                drop(guard);
                Some(KeybindOutput::Value(self.accept_line().await))
            },
            KeybindValue::Widget(mut widget) => {
                // execute the widget
                // a widget may run subprocesses so lock the ui
                let lock = self.has_foreground_process.lock().await;
                let mut ui = this.inner.borrow_mut().await;
                let buffer = ui.buffer.get_contents();
                let cursor = ui.buffer.get_cursor();

                widget.shell.set_zle_buffer(buffer.clone(), cursor as _);
                widget.shell.set_lastchar(buf);
                // executing a widget may block
                let (output, _) = tokio::task::block_in_place(|| widget.exec_and_get_output(None, [].into_iter())).unwrap();
                let (buffer, cursor) = shell.get_zle_buffer();

                ui.buffer.set(Some(buffer.as_ref()), Some(cursor.unwrap_or(buffer.len() as _) as _));
                // check for any output e.g. zle -m
                let output = output.trim();
                if !output.is_empty() {
                    ui.tui.add_message(BStr::new(output).to_string());
                }
                drop(lock);

                // this widget may have called accept-line somewhere inside
                if shell.has_accepted_line() {
                    drop(shell);
                    drop(ui);
                    drop(guard);
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
                nix::sys::signal::kill(nix::unistd::Pid::from_raw(0), nix::sys::signal::Signal::SIGTERM)?;
                for pid in &self.get().inner.borrow().await.threads {
                    nix::sys::signal::kill(*pid, nix::sys::signal::Signal::SIGINT)?;
                }
            },

            Event::Key(KeyEvent{ key: Key::Char(c), modifiers }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                {
                    let mut buf = [0; 4];
                    let this = &*self.get();
                    let mut ui = this.inner.borrow_mut().await;
                    ui.buffer.insert_at_cursor(c.encode_utf8(&mut buf).as_bytes());
                }
                self.trigger_buffer_change_callbacks(()).await;
                self.draw().await?;
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

}

impl UiInner {
    pub fn activate(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;

        if self.enhanced_keyboard {
            queue!(
                self.stdout.lock(),
                event::PushKeyboardEnhancementFlags(
                    event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | event::KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    | event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                    | event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                )
            )?;
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
            queue!(self.stdout, event::PopKeyboardEnhancementFlags)?;
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

    fn reset(&mut self, _shell: &mut ShellInner) {
        self.buffer.reset();
        self.dirty = true;
        // shell.set_curhist(i64::MAX);
    }

    pub fn prepare_for_unhandled_output(&mut self) -> Result<()> {
        self.deactivate()?;
        self.dirty = true;
        // move to last line of buffer
        let y_offset = self.buffer.draw_end_pos.1 - self.buffer.cursor_coord.1;
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

    pub async fn recover_from_unhandled_output(&mut self) -> Result<()> {
        self.activate()?;
        // move down one line if not at start of line
        let cursor = self.events.get_cursor_position().await.unwrap_or((0, 0));
        if cursor.0 != 0 {
            self.size = crossterm::terminal::size()?;
            queue!(self.stdout, style::Print("\r\n"))?;
        }
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
    fn try_upgrade(&self) -> LuaResult<Ui> {
        if let Some(ui) = self.upgrade() {
            Ok(ui)
        } else {
            lua_error("ui not running")
        }
    }
}

// Resize events can occur in batches.
// With a simple loop they can be flushed.
// This function will keep the first and last resize event.
fn flush_resize_events(first_resize: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    let mut last_resize = first_resize;
    while let Ok(true) = event::poll(Duration::from_millis(50)) {
        if let Ok(event::Event::Resize(x, y)) = event::read() {
            last_resize = (x, y);
        }
    }

    (first_resize, last_resize)
}
