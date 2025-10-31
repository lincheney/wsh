use bstr::{BStr, BString};
use std::time::Duration;
use std::future::Future;
use std::sync::{Arc, Weak as WeakArc, atomic::{AtomicBool, Ordering}};
use std::ops::DerefMut;
use std::collections::HashSet;
use std::default::Default;
use serde::{Deserialize};
use mlua::prelude::*;
use tokio::sync::RwLock;
use anyhow::Result;
use crate::keybind::parser::{Event, KeyEvent, Key, KeyModifiers};

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::{self, position, MoveToColumn},
    event,
    // event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style,
    execute,
    queue,
};

use crate::shell::{Shell, ShellInner, UpgradeShell, KeybindValue};
use crate::utils::*;
use crate::lua::EventCallbacks;

fn lua_error<T>(msg: &str) -> Result<T, mlua::Error> {
    Err(mlua::Error::RuntimeError(msg.to_string()))
}

struct SetScrollRegion(u16, u16);
impl crossterm::Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{};{}r", self.0, self.1)
    }
}

pub struct MoveUp(pub u16);
impl crossterm::Command for MoveUp {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        if self.0 > 0 {
            cursor::MoveUp(self.0).write_ansi(f)
        } else {
            Ok(())
        }
    }
}

pub struct MoveDown(pub u16);
impl crossterm::Command for MoveDown {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        if self.0 > 0 {
            cursor::MoveDown(self.0).write_ansi(f)
        } else {
            Ok(())
        }
    }
}

enum KeybindOutput {
    String(BString),
    Value(Result<bool>),
}

pub struct UiInner {
    pub tui: crate::tui::Tui,

    pub events: crate::event_stream::EventController,
    dirty: bool,
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
    async fn borrow_mut(&mut self) -> tokio::sync::RwLockWriteGuard<'_, UiInner>;
}

impl ThreadsafeUiInner for Arc<RwLock<UiInner>> {
    async fn borrow(&self) -> tokio::sync::RwLockReadGuard<'_, UiInner> {
        self.read().await
    }

    async fn borrow_mut(&mut self) -> tokio::sync::RwLockWriteGuard<'_, UiInner> {
        self.write().await
    }
}

#[derive(Default)]
pub struct Trampoline {
    out_notify: tokio::sync::Notify,
    in_notify: tokio::sync::Notify,
}

impl Trampoline {
    pub async fn jump_out(&self) {
        self.out_notify.notify_one();
        self.in_notify.notified().await;
    }

    pub async fn jump_in(&self, notify: bool) {
        if notify {
            self.in_notify.notify_one();
        }
        self.out_notify.notified().await;
    }
}

crate::strong_weak_wrapper! {
    pub struct Ui {
        pub inner: Arc::<RwLock<UiInner>> [WeakArc::<RwLock<UiInner>>],
        pub lua: Arc::<Lua> [WeakArc::<Lua>],
        pub shell: Shell [crate::shell::WeakShell],
        pub event_callbacks: ArcMutex::<EventCallbacks> [WeakArc::<std::sync::Mutex<EventCallbacks>>],
        is_running_process: Arc::<AtomicBool> [WeakArc::<AtomicBool>],
        pub trampoline: Arc::<Trampoline> [WeakArc::<Trampoline>],
    }
}

impl Ui {

    pub async fn new(events: crate::event_stream::EventController) -> Result<Self> {
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

        let start = std::time::Instant::now();
        let shell = Shell::new();
        {
            // let mut shell = shell.lock().await;
            // shell.readhistfile();
            // shell.init_interactive();
        }
        log::info!("loaded history in {:?}", start.elapsed());
        ui.reset(&mut shell.lock().await);

        let ui = Self{
            inner: Arc::new(RwLock::new(ui)),
            lua: Arc::new(lua),
            shell,
            event_callbacks: Default::default(),
            is_running_process: Arc::new(false.into()),
            trampoline: Arc::new(Trampoline::default()),
        };
        ui.init_lua().await?;

        Ok(ui)
    }

    pub async fn activate(&self) -> Result<()> {
        self.inner.borrow().await.activate()
    }

    pub async fn deactivate(&mut self) -> Result<()> {
        self.inner.borrow_mut().await.deactivate()
    }

    pub fn get_lua_api(&self) -> LuaResult<LuaTable> {
        self.lua.globals().get("wish")
    }

    pub async fn start_cmd(&mut self) -> Result<()> {
        EventCallbacks::trigger_precmd_callbacks(self, ()).await;
        self.draw().await
    }

    pub fn is_running_process(&self) -> bool {
        self.is_running_process.load(Ordering::Relaxed)
    }

    pub async fn draw(&mut self) -> Result<()> {
        // do NOT render ui elements if there is a foreground process
        if self.is_running_process() {
            return Ok(())
        }

        let shell = self.shell.clone();
        let mut ui = self.inner.borrow_mut().await;
        let ui = ui.deref_mut();

        if !(ui.dirty || ui.buffer.dirty || ui.prompt.dirty || ui.tui.dirty) {
            return Ok(())
        }

        crossterm::terminal::disable_raw_mode()?;
        ui.size = crossterm::terminal::size()?;
        ui.tui.draw(
            &mut ui.stdout,
            ui.size,
            &shell,
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
            self.show_error_message(format!("ERROR: {}", err)).await;
        } else if draw && let Err(err) = self.draw().await {
            log::error!("{:?}", err);
        }
    }

    pub async fn show_error_message(&mut self, msg: String) {
        {
            let mut ui = self.inner.borrow_mut().await;
            ui.tui.add_error_message(msg);
        }

        if let Err(err) = self.draw().await {
            log::error!("{:?}", err);
        }
    }

    pub async fn handle_event(&mut self, event: Event, event_buffer: BString) -> Result<bool> {
        if let Event::Key(event) = event {
            EventCallbacks::trigger_key_callbacks(self, event.into()).await;
            if let Some(result) = self.handle_key(event, event_buffer.as_ref()).await {
                let result = result?;
                EventCallbacks::trigger_buffer_change_callbacks(self, ()).await;
                self.draw().await?;
                return Ok(result)
            }

        }
        self.handle_key_default(event, event_buffer.as_ref()).await?;
        Ok(true)
    }

    async fn accept_line(&mut self) -> Result<bool> {
        let (complete, _tokens) = {
            let ui = self.inner.borrow().await;
            let buffer = ui.buffer.get_contents().as_ref();
            self.shell.lock().await.parse(buffer, false)
        };

        // time to execute
        if complete {
            EventCallbacks::trigger_accept_line_callbacks(self, ()).await;

            self.is_running_process.store(true, Ordering::Relaxed);
            let mut ui = self.inner.borrow_mut().await;
            let mut shell = self.shell.lock().await;

            ui.tui.clear_non_persistent();

            let mut result: Result<()> = (|| {
                ui.deactivate()?;

                // move to last line of buffer
                let y_offset = ui.prompt.height + ui.buffer.height - 1 - ui.buffer.cursor_coord.1 - 1;
                execute!(
                    ui.stdout,
                    BeginSynchronizedUpdate,
                    MoveDown(y_offset),
                    style::Print('\n'),
                    MoveToColumn(0),
                    Clear(ClearType::FromCursorDown),
                )?;

                let buffer = ui.buffer.get_contents();
                let cursor = buffer.len() as i64 + 1;
                shell.set_zle_buffer(buffer.clone(), cursor);
                // acceptline doesn't actually accept the line right now
                // only when we return control to zle using the trampoline
                shell.acceptline();
                Ok(())
            })();

            if result.is_ok() {
                // separate this bc it has an await
                ui.events.pause().await;
                tokio::task::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    execute!(std::io::stdout(), EndSynchronizedUpdate)
                });

                shell.clear_completion_cache();
                self.trampoline.jump_out().await;
                ui.events.resume().await;
                ui.reset(&mut shell);
                self.is_running_process.store(false, Ordering::Relaxed);

                let cursor = ui.events.get_cursor_position().await;
                result = (|| {
                    // move down one line if not at start of line
                    if cursor.0 != 0 {
                        ui.size = crossterm::terminal::size()?;
                        queue!(ui.stdout, style::Print("\r\n"))?;
                    }
                    Ok(())
                })();
            }

            self.is_running_process.store(false, Ordering::Relaxed);
            // prefer the result error over the activate error
            result.and(ui.activate())?;

            drop(ui);
            drop(shell);
            self.start_cmd().await?;

        } else {
            self.inner.borrow_mut().await.buffer.insert_at_cursor(b"\n");
            EventCallbacks::trigger_buffer_change_callbacks(self, ()).await;
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
                .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))
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
                    .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))
            }
        })
    }

    async fn init_lua(&self) -> Result<()> {
        self.set_lua_async_fn("get_cursor", |ui, _lua, ()| async move {
            Ok(ui.inner.borrow().await.buffer.get_cursor())
        })?;
        self.set_lua_async_fn("get_buffer", |ui, lua, ()| async move {
            Ok(lua.create_string(ui.inner.borrow().await.buffer.get_contents())?)
        })?;

        self.set_lua_async_fn("set_cursor", |mut ui, _lua, val: usize| async move {
            ui.inner.borrow_mut().await.buffer.set_cursor(val);
            Ok(())
        })?;

        self.set_lua_async_fn("set_buffer", |mut ui, _lua, (val, replace_len): (mlua::String, Option<usize>)| async move {
            {
                let mut ui = ui.inner.borrow_mut().await;
                ui.buffer.splice_at_cursor(&val.as_bytes(), replace_len);
            }
            EventCallbacks::trigger_buffer_change_callbacks(&mut ui, ()).await;
            Ok(())
        })?;

        self.set_lua_async_fn("undo_buffer", |mut ui, _lua, ()| async move {
            if ui.inner.borrow_mut().await.buffer.move_in_history(false) {
                EventCallbacks::trigger_buffer_change_callbacks(&mut ui, ()).await;
            }
            Ok(())
        })?;
        self.set_lua_async_fn("redo_buffer", |mut ui, _lua, ()| async move {
            if ui.inner.borrow_mut().await.buffer.move_in_history(true) {
                EventCallbacks::trigger_buffer_change_callbacks(&mut ui, ()).await;
            }
            Ok(())
        })?;

        self.set_lua_async_fn("accept_line", |mut ui, _lua, _val: ()| async move {
            ui.accept_line().await
        })?;

        #[derive(Debug, Default, Deserialize)]
        #[serde(default)]
        struct RedrawOptions {
            prompt: bool,
            buffer: bool,
            messages: bool,
            status_bar: bool,
            all: bool,
        }
        self.set_lua_async_fn("redraw", |mut ui, lua, val: Option<LuaValue>| async move {
            if let Some(val) = val {
                let val: RedrawOptions = lua.from_value(val)?;
                let mut ui = ui.inner.borrow_mut().await;
                if val.all { ui.dirty = true; }
                if val.prompt { ui.prompt.dirty = true; }
                if val.buffer { ui.buffer.dirty = true; }
                if val.messages { ui.tui.dirty = true; }
                if val.status_bar { ui.status_bar.dirty = true; }
            }

            ui.draw().await
        })?;

        self.set_lua_async_fn("eval", |ui, lua, (cmd, stderr): (mlua::String, bool)| async move {
            let data = ui.shell.lock().await.eval((*cmd.as_bytes()).into(), stderr).unwrap();
            Ok(lua.create_string(data)?)
        })?;

        self.set_lua_async_fn("allocate_height", |mut ui, _lua, height: u16| async move {
            Ui::allocate_height(&mut ui.inner.borrow_mut().await.stdout, height)
        })?;

        self.set_lua_async_fn("exit", |mut ui, _lua, code: Option<i32>| async move {
            let mut ui = ui.inner.borrow_mut().await;
            ui.events.exit(code.unwrap_or(0)).await;
            Ok(())
        })?;

        self.set_lua_async_fn("get_cwd", |ui, _lua, _: ()| async move {
            Ok(ui.shell.lock().await.get_cwd())
        })?;

        crate::lua::init_lua(self).await?;

        let lua = self.lua.clone();
        lua.load("package.path = '/home/qianli/Documents/wish/lua/?.lua;' .. package.path").exec()?;
        if let Err(err) = lua.load("require('wish')").exec() {
            log::error!("{}", err);
        }

        Ok(())
    }

    pub fn allocate_height(stdout: &mut std::io::Stdout, height: u16) -> Result<()> {
        for _ in 0 .. height {
            // vertical tab, this doesn't change x
            queue!(stdout, style::Print("\x0b"))?;
        }
        queue!(stdout, MoveUp(height))?;
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
                            } else {
                                return Some(x) // error
                            }
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
        let callback = self.inner.borrow().await.keybinds
            .iter()
            .rev()
            .find_map(|k| k.inner.get(&(event.key, event.modifiers)))
            .cloned();
        if let Some(callback) = callback {
            self.call_lua_fn(true, callback, ()).await;
            return Some(KeybindOutput::Value(Ok(true)))
        }

        let mut shell = self.shell.lock().await;
        // look for a zle widget
        match shell.get_keybinding(buf)? {
            KeybindValue::String(string) => {
                // recurse
                return Some(KeybindOutput::String(string))
            },
            KeybindValue::Widget(widget) => {
                // skip not found or where we have our own impl
                if widget.is_self_insert() || widget.is_undefined_key() {
                    return None
                }
                // need to run our accept_line with a trampoline
                if widget.is_accept_line() {
                    drop(shell);
                    return Some(KeybindOutput::Value(self.accept_line().await));
                }

                // execute the widget

                let mut ui = self.inner.borrow_mut().await;
                let buffer = ui.buffer.get_contents();
                let cursor = buffer.len() + 1;

                shell.set_zle_buffer(buffer.clone(), cursor as _);
                shell.set_lastchar(buf);
                shell.exec_zle_widget(widget, [].into_iter());
                let (buffer, cursor) = shell.get_zle_buffer();

                ui.buffer.set(Some(buffer.as_ref()), Some(cursor.unwrap_or(buffer.len() as _) as _));

                // this widget may have called accept-line somewhere inside
                if shell.has_accepted_line() {
                    drop(shell);
                    drop(ui);
                    return Some(KeybindOutput::Value(self.accept_line().await))
                }

                return Some(KeybindOutput::Value(Ok(true)))
            },
        }
    }

    async fn handle_key_default(&mut self, event: Event, _buf: &BStr) -> Result<bool> {
        match event {

            Event::Key(KeyEvent{ key: Key::Escape, .. }) => {
                nix::sys::signal::kill(nix::unistd::Pid::from_raw(0), nix::sys::signal::Signal::SIGTERM)?;
                for pid in self.inner.borrow().await.threads.iter() {
                    nix::sys::signal::kill(*pid, nix::sys::signal::Signal::SIGINT)?;
                }
            },

            Event::Key(KeyEvent{ key: Key::Char(c), modifiers }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                {
                    let mut ui = self.inner.borrow_mut().await;
                    let mut buf = [0; 4];
                    ui.buffer.insert_at_cursor(c.encode_utf8(&mut buf).as_bytes());
                }
                EventCallbacks::trigger_buffer_change_callbacks(self, ()).await;
                self.draw().await?;
            },

            Event::Key(KeyEvent{ key: Key::Enter, modifiers }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                return self.accept_line().await;
            },

            Event::BracketedPaste(data) => {
                let data = self.lua.create_string(data)?;
                EventCallbacks::trigger_paste_callbacks(self, data).await;
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

    fn deactivate(&mut self) -> Result<()> {
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

}

impl Drop for UiInner {
    fn drop(&mut self) {
        if let Err(err) = self.deactivate() {
            log::error!("{:?}", err);
        };
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
