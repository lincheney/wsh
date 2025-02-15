use std::time::Duration;
use std::future::Future;
use std::sync::{Arc, Weak};
use std::ops::DerefMut;
use std::collections::HashSet;
use std::default::Default;
use mlua::{IntoLuaMulti, FromLuaMulti, Lua, Result as LuaResult, Value as LuaValue};
use async_std::sync::RwLock;
use anyhow::Result;

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::{self, position, SavePosition, RestorePosition},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style,
    execute,
    queue,
};

use crate::keybind;
use crate::completion;
use crate::shell::Shell;

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
    lua_cache: mlua::Table,

    pub tui: crate::tui::Tui,

    events: crate::event_stream::EventLocker,
    is_running_process: bool,
    dirty: bool,
    cursory: u16,
    pub keybinds: keybind::KeybindMapping,
    pub buffer: crate::buffer::Buffer,
    pub prompt: crate::prompt::Prompt,

    pub threads: HashSet<nix::unistd::Pid>,
    stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    cursor: (u16, u16),
    size: (u16, u16),
}

#[derive(Clone)]
pub struct Ui(Arc<RwLock<UiInner>>);

impl Ui {

    pub async fn new(shell: &Shell, mut events: crate::event_stream::EventLocker) -> Result<Self> {
        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;
        let lua_cache = lua.create_table()?;
        lua_api.set("__cache", &lua_cache)?;

        let cursor = events.get_cursor_position().await?;

        let ui = Self(Arc::new(RwLock::new(UiInner{
            lua,
            lua_api,
            lua_cache,
            events,
            is_running_process: false,
            dirty: true,
            cursory: 0,
            tui: Default::default(),
            threads: HashSet::new(),
            buffer: Default::default(),
            prompt: crate::prompt::Prompt::new(None),
            keybinds: Default::default(),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            cursor,
            size: crossterm::terminal::size()?,
        })));

        ui.init_lua(shell).await?;

        Ok(ui)
    }

    pub fn borrow(&self) -> async_lock::futures::Read<UiInner> {
        self.0.read()
    }

    pub fn borrow_mut(&self) -> async_lock::futures::Write<UiInner> {
        self.0.write()
    }

    pub async fn activate(&self) -> Result<()> {
        self.borrow().await.activate()
    }

    pub async fn deactivate(&self) -> Result<()> {
        self.borrow_mut().await.deactivate()
    }

    pub async fn draw(&self, shell: &Shell) -> Result<()> {
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
        let size = crossterm::terminal::size()?;

        if ui.dirty || ui.prompt.dirty {
            // move back to top of drawing area
            queue!(ui.stdout, MoveUp(ui.cursory))?;
            ui.dirty = ui.prompt.draw(&mut ui.stdout, &mut *shell.lock().await, size)? || ui.dirty;
        } else {
            // move back to prompt line
            queue!(ui.stdout, MoveUp(ui.buffer.cursory as _))?;
        }

        if ui.dirty || ui.buffer.dirty {
            // MUST start on same line as prompt
            ui.dirty = ui.buffer.draw(&mut ui.stdout, size, ui.prompt.width)? || ui.dirty;
        } else {
            // move to cursor
            queue!(ui.stdout, MoveDown(ui.buffer.cursory as _))?;
        }

        let events = ui.events.lock().await;

        if ui.dirty || ui.tui.dirty {
            // move to last line of buffer
            let yoffset = (ui.buffer.height - ui.buffer.cursory - 1) as u16;
            queue!(ui.stdout, MoveDown(yoffset))?;
            // tui needs to know exactly where it is
            ui.cursor = events.get_cursor_position()?;
            ui.tui.draw(&mut ui.stdout, size, ui.cursor.1)?;
            // then move back
            queue!(ui.stdout, MoveUp(yoffset))?;
        }

        execute!(ui.stdout, EndSynchronizedUpdate)?;
        ui.cursory = (ui.prompt.height + ui.buffer.height) as u16;
        ui.cursor = events.get_cursor_position()?;
        crossterm::terminal::enable_raw_mode()?;

        ui.cursory = 0;
        ui.dirty = false;
        Ok(())
    }

    pub async fn handle_event(&self, event: Event, shell: &Shell) -> Result<bool> {
        // eprintln!("DEBUG(grieve)\t{}\t= {:?}\r", stringify!(event), event);

        if let Event::Key(KeyEvent{code, modifiers, ..}) = event {
            let callback = self.borrow().await.keybinds.get(&(code, modifiers)).cloned();
            if let Some(callback) = callback {
                let ui = self.clone();
                let shell = shell.clone();

                if let Err(err) = callback.call_async::<LuaValue>(mlua::Nil).await {
                    let mut ui = ui.borrow_mut().await;
                    ui.tui.add_error_message(format!("ERROR: {}", err), None);
                }

                if let Err(err) = ui.draw(&shell).await {
                    eprintln!("DEBUG(armada)\t{}\t= {:?}", stringify!(err), err);
                }

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
                // eprintln!("DEBUG(leaps) \t{}\t= {:?}", stringify!("kill"), "kill");
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
                    let mut ui = self.borrow_mut().await;
                    // flush cache
                    ui.lua_cache.set("buffer", mlua::Nil)?;
                    ui.lua_cache.set("cursor", mlua::Nil)?;

                    ui.buffer.mutate(|contents, cursor, byte_pos| {
                        let mut buf = [0; 4];
                        contents.splice(byte_pos .. byte_pos, c.encode_utf8(&mut buf).as_bytes().iter().copied());
                        *cursor += 1;
                    });
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
                // let (complete, tokens) = shell.lock().await.parse("echo $(");
            },

            _ => {},
        }

        // if event == crossterm::event::Event::Key(crossterm::event::KeyCode::Char('c').into()) {
            // println!("Cursor position: {:?}\r", crossterm::cursor::position());
        // }

        Ok(true)
    }

    async fn accept_line(&self, shell: &Shell) -> Result<bool> {
        self.borrow_mut().await.is_running_process = true;
        self.draw(shell).await?;

        {
            let mut ui = self.borrow_mut().await;
            let ui = ui.deref_mut();
            ui.tui.clear_non_persistent();

            {
                // time to execute
                let mut shell = shell.lock().await;
                let (complete, _tokens) = shell.parse(ui.buffer.get_contents().as_ref());
                if complete {
                    shell.clear_completion_cache();

                    ui.deactivate()?;
                    // new line
                    execute!(ui.stdout, style::Print("\r\n"))?;

                    if let Err(code) = shell.exec(ui.buffer.get_contents().as_ref(), None) {
                        eprintln!("DEBUG(atlas) \t{}\t= {:?}", stringify!(code), code);
                    }
                    ui.buffer.reset();
                    ui.prompt.dirty = true;
                } else {
                    eprintln!("DEBUG(lunch) \t{}\t= {:?}", stringify!("invalid command"), "invalid command");
                }
                ui.is_running_process = false;
            }

            ui.dirty = true;
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

    pub async fn set_lua_fn<F, A, R>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(&Self, &Lua, A) -> LuaResult<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {

        let weak = Arc::downgrade(&self.0);
        let ui = self.borrow().await;
        ui.lua_api.set(name, ui.lua.create_function(move |lua, value| {
            func(&Ui::try_upgrade(&weak)?, lua, value)
        })?)
    }

    pub async fn set_lua_async_fn<F, A, R, T>(&self, name: &str, shell: &Shell, func: F) -> LuaResult<()>
    where
        F: Fn(Self, Shell, Lua, A) -> T + mlua::MaybeSend + 'static + Clone,
        A: FromLuaMulti + Send + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + mlua::MaybeSend + 'static,
    {
        let weak = Arc::downgrade(&self.0);
        let ui = self.borrow().await;
        let shell = Arc::downgrade(&shell.0);
        ui.lua_api.set(name, ui.lua.create_async_function(move |lua, value| {
            let weak = weak.clone();
            let func = func.clone();
            let shell = shell.clone();
            async move {
                let ui = Ui::try_upgrade(&weak)?;
                func(ui, Shell(shell.upgrade().unwrap()), lua, value).await
                    .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))
            }
        })?)
    }

    async fn init_lua(&self, shell: &Shell) -> Result<()> {
        self.set_lua_async_fn("__get_cursor", shell, |ui, _shell, _lua, _val: ()| async move {
            Ok(ui.borrow().await.buffer.get_cursor())
        } ).await?;
        self.set_lua_async_fn("__get_buffer", shell, |ui, _shell, lua, _val: ()| async move {
            Ok(lua.create_string(ui.borrow().await.buffer.get_contents())?)
        }).await?;

        self.set_lua_async_fn("__set_cursor", shell, |ui, _shell, _lua, val: usize| async move {
            ui.borrow_mut().await.buffer.set_cursor(val);
            Ok(())
        }).await?;

        self.set_lua_async_fn("__set_buffer", shell, |ui, _shell, _lua, val: mlua::String| async move {
            ui.borrow_mut().await.buffer.set_contents((*val.as_bytes()).into());
            Ok(())
        }).await?;

        self.set_lua_async_fn("accept_line", shell, |ui, shell, _lua, _val: ()| async move {
            ui.accept_line(&shell).await
        }).await?;

        self.set_lua_async_fn("redraw", shell, |ui, shell, _lua, _val: ()| async move {
            ui.draw(&shell).await
        }).await?;

        self.set_lua_async_fn("eval", shell, |_ui, shell, lua, (cmd, stderr): (mlua::String, bool)| async move {
            let data = shell.lock().await.eval((*cmd.as_bytes()).into(), stderr).unwrap();
            Ok(lua.create_string(data)?)
        }).await?;

        keybind::init_lua(self, shell).await?;
        completion::init_lua(self, shell).await?;
        crate::tui::init_lua(self, shell).await?;

        let lua = self.borrow().await.lua.clone();
        lua.load("package.path = '/home/qianli/Documents/wish/lua/?.lua;' .. package.path").exec()?;
        if let Err(err) = lua.load("require('wish')").exec() {
            eprintln!("DEBUG(sliver)\t{}\t= {:?}", stringify!(err), err);
        }

        Ok(())
    }

    pub fn allocate_height(stdout: &mut std::io::Stdout, height: u16) -> Result<()> {
        // the y will be wrong but at least the x will be right
        queue!(stdout, SavePosition)?;
        for _ in 0 .. height {
            queue!(stdout, style::Print("\n"))?;
        }
        queue!(
            stdout,
            RestorePosition,
            MoveDown(height),
            MoveUp(height),
        )?;
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

}

impl Drop for UiInner {
    fn drop(&mut self) {
        if let Err(err) = self.deactivate() {
            eprintln!("ERROR: {}", err);
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
