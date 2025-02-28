use std::time::Duration;
use std::future::Future;
use std::sync::{Arc, Weak};
use std::ops::DerefMut;
use std::collections::HashSet;
use std::default::Default;
use serde::{Deserialize};
use mlua::prelude::*;
use tokio::sync::RwLock;
use anyhow::Result;

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::{self, position},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style::{self, ContentStyle, Attributes, Stylize},
    execute,
    queue,
};

use crate::keybind;
use crate::completion;
use crate::shell::{Shell, ShellInner};

fn lua_error<T>(msg: &str) -> Result<T, mlua::Error> {
    Err(mlua::Error::RuntimeError(msg.to_string()))
}

struct SetScrollRegion(u16, u16);
impl crossterm::Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{};{}r", self.0, self.1)
    }
}

struct MoveUp(u16);
impl crossterm::Command for MoveUp {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        if self.0 > 0 {
            cursor::MoveUp(self.0).write_ansi(f)
        } else {
            Ok(())
        }
    }
}

struct MoveDown(u16);
impl crossterm::Command for MoveDown {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        if self.0 > 0 {
            cursor::MoveDown(self.0).write_ansi(f)
        } else {
            Ok(())
        }
    }
}

pub struct UiInner {
    pub lua: Lua,
    pub lua_api: mlua::Table,

    pub tui: crate::tui::Tui,

    pub events: crate::event_stream::EventLocker,
    is_running_process: bool,
    dirty: bool,
    y_offset: u16,
    pub keybinds: keybind::KeybindMapping,
    pub event_callbacks: crate::events::EventCallbacks,

    pub buffer: crate::buffer::Buffer,
    pub prompt: crate::prompt::Prompt,

    pub threads: HashSet<nix::unistd::Pid>,
    stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    size: (u16, u16),
}

#[derive(Clone)]
pub struct Ui(Arc<RwLock<UiInner>>);

impl Ui {

    pub async fn new(shell: &Shell, events: crate::event_stream::EventLocker) -> Result<Self> {
        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;

        let mut ui = UiInner{
            lua,
            lua_api,
            events,
            is_running_process: false,
            dirty: true,
            y_offset: 0,
            tui: Default::default(),
            threads: HashSet::new(),
            buffer: Default::default(),
            prompt: crate::prompt::Prompt::new(None),
            keybinds: Default::default(),
            event_callbacks: Default::default(),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            size: crossterm::terminal::size()?,
        };

        ui.buffer.highlights.push(crate::buffer::Highlight{
            start: 1,
            end: 5,
            style: ContentStyle::new().on_dark_yellow(),
            attribute_mask: Attributes::default(),
        });

        let start = std::time::Instant::now();
        shell.lock().await.readhistfile();
        log::info!("loaded history in {:?}", start.elapsed());
        ui.reset(&mut *shell.lock().await);

        let ui = Self(Arc::new(RwLock::new(ui)));
        ui.init_lua(shell).await?;

        Ok(ui)
    }

    pub async fn borrow(&self) -> tokio::sync::RwLockReadGuard<UiInner> {
        self.0.read().await
    }

    pub async fn borrow_mut(&mut self) -> tokio::sync::RwLockWriteGuard<UiInner> {
        self.0.write().await
    }

    pub async fn activate(&self) -> Result<()> {
        self.borrow().await.activate()
    }

    pub async fn deactivate(&mut self) -> Result<()> {
        self.borrow_mut().await.deactivate()
    }

    pub async fn draw(&mut self, shell: &Shell) -> Result<()> {
        let mut ui = self.borrow_mut().await;
        let ui = ui.deref_mut();

        // if ui.dirty it means redraw everything from scratch
        if ui.dirty || ui.is_running_process {
            queue!(ui.stdout, Clear(ClearType::FromCursorDown))?;
        }

        // do NOT render ui elements if there is a foreground process
        if !(ui.dirty || ui.buffer.dirty || ui.prompt.dirty || ui.tui.dirty) || ui.is_running_process {
            return Ok(())
        }

        crossterm::terminal::disable_raw_mode()?;
        queue!(ui.stdout, BeginSynchronizedUpdate)?;
        ui.size = crossterm::terminal::size()?;

        if ui.dirty || ui.prompt.dirty {
            // move back to top of drawing area and redraw prompt
            queue!(ui.stdout, MoveUp(ui.y_offset))?;
            ui.dirty = ui.prompt.draw(&mut ui.stdout, &mut *shell.lock().await, ui.size)? || ui.dirty;
        } else {
            // move back to prompt line
            queue!(ui.stdout, MoveUp(ui.buffer.y_offset as _))?;
        }

        if ui.dirty || ui.buffer.dirty {
            // MUST start on same line as prompt
            ui.dirty = ui.buffer.draw(&mut ui.stdout, ui.size, ui.prompt.width)? || ui.dirty;
        } else {
            // move to cursor
            queue!(ui.stdout, MoveDown(ui.buffer.y_offset as _))?;
        }

        ui.y_offset = (ui.prompt.height + ui.buffer.y_offset - 1) as u16;

        if ui.dirty || ui.tui.dirty {
            // move to last line of buffer
            let y_offset = (ui.buffer.height - ui.buffer.y_offset - 1) as u16;
            execute!(ui.stdout, MoveDown(y_offset))?;
            ui.tui.draw(&mut ui.stdout, ui.size, ui.dirty)?;
            // then move back
            queue!(ui.stdout, MoveUp(y_offset))?;
        }

        execute!(ui.stdout, EndSynchronizedUpdate)?;
        crossterm::terminal::enable_raw_mode()?;

        ui.dirty = false;
        Ok(())
    }

    pub fn call_lua_fn<T: IntoLuaMulti + mlua::MaybeSend + 'static>(&self, shell: Shell, draw: bool, callback: mlua::Function, arg: T) {
        let mut ui = self.clone();
        tokio::task::spawn(async move {
            if let Err(err) = callback.call_async::<LuaValue>(arg).await {
                log::error!("{}", err);
                ui.show_error_message(&shell, format!("ERROR: {}", err)).await;
            } else if draw {
                if let Err(err) = ui.draw(&shell).await {
                    log::error!("{:?}", err);
                }
            }
        });
    }

    pub async fn show_error_message(&mut self, shell: &Shell, msg: String) {
        {
            let mut ui = self.borrow_mut().await;
            ui.tui.add_error_message(msg, None);
        }

        if let Err(err) = self.draw(&shell).await {
            log::error!("{:?}", err);
        }
    }

    pub async fn handle_event(&mut self, event: Event, shell: &Shell) -> Result<bool> {

        if let Event::Key(key @ KeyEvent{code, modifiers, kind: event::KeyEventKind::Press, ..}) = event {
            let ui = self.borrow().await;

            if ui.event_callbacks.has_key_callbacks() {
                ui.event_callbacks.trigger_key_callbacks(self, shell, &ui.lua, key.into());
            }

            let callback = ui.keybinds.get(&(code, modifiers)).cloned();
            if let Some(callback) = callback {
                self.call_lua_fn(shell.clone(), true, callback, ());
                return Ok(true)
            }
        }

        match event {

            Event::Key(KeyEvent{
                code: KeyCode::Esc,
                modifiers: _,
                kind: event::KeyEventKind::Press,
                state: _,
            }) => {
                nix::sys::signal::kill(nix::unistd::Pid::from_raw(0), nix::sys::signal::Signal::SIGTERM)?;
                for pid in self.borrow().await.threads.iter() {
                    nix::sys::signal::kill(*pid, nix::sys::signal::Signal::SIGINT)?;
                }
            },

            Event::Key(KeyEvent{
                code: KeyCode::Char(c),
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                {
                    let clone = self.clone();
                    let mut ui = self.borrow_mut().await;
                    let mut buf = [0; 4];
                    ui.buffer.insert(c.encode_utf8(&mut buf).as_bytes());
                    if ui.event_callbacks.has_buffer_change_callbacks() {
                        ui.event_callbacks.trigger_buffer_change_callbacks(&clone, shell, &ui.lua, ());
                    }
                }

                self.draw(shell).await?;
            },

            Event::Key(KeyEvent{
                code: KeyCode::Enter,
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                return self.accept_line(shell).await;
            },

            Event::Key(KeyEvent{
                code: KeyCode::F(11),
                modifiers: _,
                kind: event::KeyEventKind::Press,
                state: _,
            }) => {
                // eprintln!("DEBUG(feign) \t{}\t= {:?}", stringify!(shell.lock().await.get_history().collect::<Vec<_>>()), shell.lock().await.get_history().collect::<Vec<_>>());
                // let (complete, tokens) = shell.lock().await.parse("echo $(");
            },

            Event::Paste(data) => {
                let ui = self.borrow().await;
                if ui.event_callbacks.has_paste_callbacks() {
                    ui.event_callbacks.trigger_paste_callbacks(self, shell, &ui.lua, ui.lua.create_string(data.as_bytes())?);
                }
            },

            _ => {},
        }

        Ok(true)
    }

    async fn accept_line(&mut self, shell: &Shell) -> Result<bool> {
        self.borrow_mut().await.is_running_process = true;
        self.draw(shell).await?;

        {
            let clone = self.clone();
            let mut ui = self.borrow_mut().await;
            let ui = ui.deref_mut();

            {
                // time to execute
                let (complete, _tokens) = shell.lock().await.parse(ui.buffer.get_contents().as_ref());
                if complete {

                    if ui.event_callbacks.has_accept_line_callbacks() {
                        ui.event_callbacks.trigger_accept_line_callbacks(&clone, shell, &ui.lua, ());
                    }

                    let mut shell = shell.lock().await;
                    shell.clear_completion_cache();
                    shell.push_history(ui.buffer.get_contents().as_ref());

                    ui.tui.clear_non_persistent();
                    ui.deactivate()?;
                    // new line
                    execute!(ui.stdout, style::Print("\r\n"))?;

                    if let Err(code) = shell.exec(ui.buffer.get_contents().as_ref(), None) {
                        eprintln!("DEBUG(atlas) \t{}\t= {:?}", stringify!(code), code);
                    }
                    ui.reset(&mut shell);
                    ui.is_running_process = false;

                    // move down one line if not at start of line
                    let cursor = ui.events.lock().await.get_cursor_position()?;
                    if cursor.0 != 0 {
                        ui.size = crossterm::terminal::size()?;
                        queue!(ui.stdout, style::Print("\r\n"))?;
                    }

                } else {
                    ui.buffer.insert(b"\n");
                    if ui.event_callbacks.has_buffer_change_callbacks() {
                        ui.event_callbacks.trigger_buffer_change_callbacks(&clone, shell, &ui.lua, ());
                    }
                }
                ui.is_running_process = false;
            }

            ui.activate()?;
        }

        self.draw(shell).await?;
        Ok(true)
    }

    fn try_upgrade(ui: &Weak<RwLock<UiInner>>) -> LuaResult<Self> {
        if let Some(ui) = ui.upgrade() {
            Ok(Ui(ui))
        } else {
            lua_error("ui not running")
        }
    }

    pub async fn set_lua_fn<F, A, R>(&self, name: &str, shell: &Shell, func: F) -> LuaResult<()>
    where
        F: Fn(&Self, &Shell, &Lua, A) -> Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {

        let func = self.make_lua_fn(shell, func).await?;
        self.borrow().await.lua_api.set(name, func)
    }

    pub async fn make_lua_fn<F, A, R>(&self, shell: &Shell, func: F) -> LuaResult<LuaFunction>
    where
        F: Fn(&Self, &Shell, &Lua, A) -> Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
        let weak = Arc::downgrade(&self.0);
        let ui = self.borrow().await;
        let shell = Arc::downgrade(&shell.0);
        ui.lua.create_function(move |lua, value| {
            let ui = Ui::try_upgrade(&weak)?;
            func(&ui, &Shell(shell.upgrade().unwrap()), lua, value)
                .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))
        })
    }

    pub async fn set_lua_async_fn<F, A, R, T>(&self, name: &str, shell: &Shell, func: F) -> LuaResult<()>
    where
        F: Fn(Self, Shell, Lua, A) -> T + mlua::MaybeSend + 'static + Clone,
        A: FromLuaMulti + Send + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + mlua::MaybeSend + 'static,
    {
        let func = self.make_lua_async_fn(shell, func).await?;
        self.borrow().await.lua_api.set(name, func)
    }

    pub async fn make_lua_async_fn<F, A, R, T>(&self, shell: &Shell, func: F) -> LuaResult<LuaFunction>
    where
        F: Fn(Self, Shell, Lua, A) -> T + mlua::MaybeSend + 'static + Clone,
        A: FromLuaMulti + Send + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + mlua::MaybeSend + 'static,
    {
        let weak = Arc::downgrade(&self.0);
        let ui = self.borrow().await;
        let shell = Arc::downgrade(&shell.0);
        ui.lua.create_async_function(move |lua, value| {
            let weak = weak.clone();
            let func = func.clone();
            let shell = shell.clone();
            async move {
                let ui = Ui::try_upgrade(&weak)?;
                func(ui, Shell(shell.upgrade().unwrap()), lua, value).await
                    .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))
            }
        })
    }

    async fn init_lua(&self, shell: &Shell) -> Result<()> {
        self.set_lua_async_fn("get_cursor", shell, |ui, _shell, _lua, _val: ()| async move {
            Ok(ui.borrow().await.buffer.get_cursor())
        } ).await?;
        self.set_lua_async_fn("get_buffer", shell, |ui, _shell, lua, _val: ()| async move {
            Ok(lua.create_string(ui.borrow().await.buffer.get_contents())?)
        }).await?;

        self.set_lua_async_fn("set_cursor", shell, |mut ui, _shell, _lua, val: usize| async move {
            ui.borrow_mut().await.buffer.set_cursor(val);
            Ok(())
        }).await?;

        self.set_lua_async_fn("set_buffer", shell, |mut ui, shell, _lua, val: mlua::String| async move {
            let clone = ui.clone();
            let mut ui = ui.borrow_mut().await;
            ui.buffer.set_contents(&val.as_bytes());
            if ui.event_callbacks.has_buffer_change_callbacks() {
                ui.event_callbacks.trigger_buffer_change_callbacks(&clone, &shell, &ui.lua, ());
            }
            Ok(())
        }).await?;

        self.set_lua_async_fn("accept_line", shell, |mut ui, shell, _lua, _val: ()| async move {
            ui.accept_line(&shell).await
        }).await?;

        #[derive(Debug, Default, Deserialize)]
        #[serde(default)]
        struct RedrawOptions {
            prompt: bool,
            buffer: bool,
            messages: bool,
            all: bool,
        }
        self.set_lua_async_fn("redraw", shell, |mut ui, shell, lua, val: Option<LuaValue>| async move {
            if let Some(val) = val {
                let val: RedrawOptions = lua.from_value(val)?;
                let mut ui = ui.borrow_mut().await;
                if val.all { ui.dirty = true; }
                if val.prompt { ui.prompt.dirty = true; }
                if val.buffer { ui.buffer.dirty = true; }
                if val.messages { ui.tui.dirty = true; }
            }

            ui.draw(&shell).await
        }).await?;

        self.set_lua_async_fn("eval", shell, |_ui, shell, lua, (cmd, stderr): (mlua::String, bool)| async move {
            let data = shell.lock().await.eval((*cmd.as_bytes()).into(), stderr).unwrap();
            Ok(lua.create_string(data)?)
        }).await?;

        self.set_lua_async_fn("allocate_height", shell, |mut ui, _shell, _lua, height: u16| async move {
            Ui::allocate_height(&mut ui.borrow_mut().await.stdout, height)
        }).await?;

        keybind::init_lua(self, shell).await?;
        completion::init_lua(self, shell).await?;
        crate::tui::init_lua(self, shell).await?;
        crate::history::init_lua(self, shell).await?;
        crate::events::init_lua(self, shell).await?;
        crate::lua::init_lua(self, shell).await?;
        crate::string::init_lua(self, shell).await?;

        let lua = self.borrow().await.lua.clone();
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

    fn reset(&mut self, shell: &mut ShellInner) {
        self.buffer.reset();
        self.y_offset = 0;
        self.dirty = true;
        shell.set_curhist(i64::MAX);
    }

}

impl Drop for UiInner {
    fn drop(&mut self) {
        if let Err(err) = self.deactivate() {
            log::error!("{:?}", err);
        };
    }
}

fn print_events() -> Result<()> {
    loop {
        // Blocking read
        let event = event::read()?;

        println!("Event: {:?}\r", event);

        if event == event::Event::Key(event::KeyCode::Char('c').into()) {
            println!("Cursor position: {:?}\r", position());
        }

        if let event::Event::Resize(x, y) = event {
            let (original_size, new_size) = flush_resize_events((x, y));
            println!("Resize from: {:?}, to: {:?}\r", original_size, new_size);
        }

        if event == event::Event::Key(event::KeyCode::Esc.into()) {
            break;
        }
    }

    Ok(())
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
