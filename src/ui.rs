use std::time::Duration;
use std::future::Future;
use std::sync::{Arc, Weak};
use std::io::{Write};
use std::ops::DerefMut;
use std::collections::HashSet;
use mlua::{IntoLuaMulti, FromLuaMulti, Lua, Result as LuaResult, Value as LuaValue};
use futures::SinkExt;
use async_std::sync::RwLock;
use anyhow::Result;

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::{position},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style,
    execute,
    queue,
};

use crate::keybind;
use crate::completion;
use crate::zsh;
use crate::shell::Shell;
mod text_popup;
mod views;

fn lua_error<T>(msg: &str) -> Result<T, mlua::Error> {
    Err(mlua::Error::RuntimeError(msg.to_string()))
}

struct SetScrollRegion(u16, u16);
impl crossterm::Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{};{}r", self.0, self.1)
    }
}

#[derive(Debug, Default)]
struct UiDirty {
    buffer: bool,
}

pub type TrampolineFut = std::pin::Pin<Box<dyn Future<Output=Result<()>> + Send>>;

pub struct UiInner {
    pub lua: Lua,
    pub lua_api: mlua::Table,
    lua_cache: mlua::Table,

    pub views: views::Views,

    trampoline: futures::channel::mpsc::UnboundedSender<TrampolineFut>,
    dirty: UiDirty,
    is_running_process: bool,
    pub keybinds: keybind::KeybindMapping,
    pub buffer: crate::buffer::Buffer,

    pub threads: HashSet<nix::unistd::Pid>,
    stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    cursor: (u16, u16),
    size: (u16, u16),
}

#[derive(Clone)]
pub struct Ui(Arc<RwLock<UiInner>>);

impl Ui {

    pub async fn new(shell: &Shell, trampoline: futures::channel::mpsc::UnboundedSender<TrampolineFut>) -> Result<Self> {
        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;
        let lua_cache = lua.create_table()?;
        lua_api.set("__cache", &lua_cache)?;

        let ui = Self(Arc::new(RwLock::new(UiInner{
            lua,
            lua_api,
            lua_cache,
            trampoline,
            is_running_process: false,
            views: std::default::Default::default(),
            threads: HashSet::new(),
            dirty: UiDirty::default(),
            buffer: std::default::Default::default(),
            keybinds: std::default::Default::default(),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            cursor: crossterm::cursor::position()?,
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

    pub async fn draw(&self, shell: &Shell, use_trampoline: bool) -> Result<()> {
        let mut ui = self.borrow_mut().await;
        let ui = ui.deref_mut();

        queue!(
            ui.stdout,
            BeginSynchronizedUpdate,
            style::Print("\r"),
            Clear(ClearType::FromCursorDown),
        )?;

        let prompt = shell.lock().await.exec("print -v tmpvar -P \"$PROMPT\" 2>/dev/null", None).ok()
            .and_then(|_| zsh::Variable::get("tmpvar").map(|mut v| v.to_bytes()));

        let prompt = prompt.as_ref().map(|p| &p[..]).unwrap_or(b">>> ");
        // let prompt = ui.shell.eval(stringify!(printf %s "${PS1@P}"), false).await?;
        ui.stdout.write(prompt)?;
        ui.stdout.write(ui.buffer.get_contents().as_bytes())?;

        let offset = ui.buffer.get_contents().len() - ui.buffer.get_cursor();
        if offset > 0 {
            queue!(ui.stdout, crossterm::cursor::MoveLeft(offset as u16))?;
        }

        // do NOT render ui elements if there is a foreground process
        if !ui.is_running_process {
            let (width, height) = crossterm::terminal::size()?;
            ui.views.draw(&mut ui.stdout, width, height.saturating_sub(5))?;
        }

        queue!(ui.stdout, EndSynchronizedUpdate)?;
        execute!(ui.stdout)?;

        // if use_trampoline {
            // // refresh_cursor_position needs to get trampolined out
            // // because it doesn't work while an event stream is active
            // let clone = self.clone();
            // ui.trampoline.send(
                // Box::pin(async move {
                    // clone.borrow_mut().await.refresh_cursor_position()
                // })
            // ).await?;
        // } else {
            // ui.refresh_cursor_position()?;
        // }

        Ok(())
    }

    pub async fn handle_event(&self, event: Event, shell: &Shell) -> Result<bool> {
        // eprintln!("DEBUG(grieve)\t{}\t= {:?}\r", stringify!(event), event);

        if let Event::Key(KeyEvent{code, modifiers, ..}) = event {
            let callback = self.borrow().await.keybinds.get(&(code, modifiers)).cloned();
            if let Some(callback) = callback {
                let clone = self.clone();
                let shell = shell.clone();
                async_std::task::spawn(async move {
                    if let Err(err) = callback.call_async::<LuaValue>(mlua::Nil).await {
                        eprintln!("DEBUG(loaf)  \t{}\t= {:?}", stringify!(err), err);
                    }
                    if shell.lock().await.closed {
                    } else {
                        clone.refresh_on_state(&shell).await;
                    }
                });
                return Ok(true)
            }
        }

        match event {

            Event::Key(KeyEvent{
                code: KeyCode::Esc,
                modifiers,
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
                let no_redraw = {
                    let mut ui = self.borrow_mut().await;

                    // flush cache
                    ui.lua_cache.set("buffer", mlua::Nil)?;
                    ui.lua_cache.set("cursor", mlua::Nil)?;

                    ui.buffer.mutate(|contents, cursor| -> Result<bool> {
                        contents.insert(*cursor, c);
                        *cursor += 1;
                        Ok(*cursor == contents.len())
                    })?
                };

                if no_redraw {
                    let mut ui = self.borrow_mut().await;
                    let ui = ui.deref_mut();
                    let contents = ui.buffer.get_contents();
                    execute!(ui.stdout, style::Print(&contents[contents.len() - 1 ..]))?;
                } else {
                    self.draw(shell, false).await?;
                }
            },

            Event::Key(KeyEvent{
                code: KeyCode::Enter,
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                return self.accept_line(shell, false).await;
            },

            Event::Key(KeyEvent{
                code: KeyCode::Esc,
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.is_empty() => {
                return Ok(false)
            },

            _ => {},
        }

        // if event == crossterm::event::Event::Key(crossterm::event::KeyCode::Char('c').into()) {
            // println!("Cursor position: {:?}\r", crossterm::cursor::position());
        // }

        Ok(true)
    }

    async fn accept_line(&self, shell: &Shell, use_trampoline: bool) -> Result<bool> {
        self.borrow_mut().await.is_running_process = true;
        self.draw(shell, use_trampoline).await?;

        {
            let mut ui = self.borrow_mut().await;
            let ui = ui.deref_mut();
            ui.views.clear_non_persistent();

            {
                // time to execute
                let mut shell = shell.lock().await;
                shell.clear_completion_cache();

                ui.deactivate()?;
                // new line
                execute!(ui.stdout, style::Print("\r\n"))?;

                if let Err(code) = shell.exec(ui.buffer.get_contents(), None) {
                    eprintln!("DEBUG(atlas) \t{}\t= {:?}", stringify!(code), code);
                }
                ui.is_running_process = false;
            }

            ui.buffer.reset();
            ui.activate()?;
        }

        self.draw(shell, use_trampoline).await?;
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
                    .or_else(|e| Err(mlua::Error::RuntimeError(format!("{}", e))))
            }
        })?)
    }

    pub async fn refresh_on_state(&self, shell: &Shell) -> Result<()> {
        let dirty = self.borrow().await.dirty.buffer;
        if dirty {
            self.draw(shell, true).await?;
        }

        self.clean().await;
        Ok(())
    }

    async fn init_lua(&self, shell: &Shell) -> Result<()> {
        self.set_lua_async_fn("__get_cursor", shell, |ui, _shell, _lua, _val: LuaValue| async move { Ok(ui.borrow().await.buffer.get_cursor())} ).await?;
        self.set_lua_async_fn("__get_buffer", shell, |ui, _shell, _lua, _val: LuaValue| async move { Ok(ui.borrow().await.buffer.get_contents().clone()) }).await?;

        self.set_lua_async_fn("__set_cursor", shell, |ui, _shell, _lua, val: usize| async move {
            let mut ui = ui.borrow_mut().await;
            ui.buffer.set_cursor(val);
            ui.dirty.buffer = true;
            Ok(())
        }).await?;

        self.set_lua_async_fn("__set_buffer", shell, |ui, _shell, _lua, val: String| async move {
            let mut ui = ui.borrow_mut().await;
            ui.buffer.set_contents(val);
            ui.dirty.buffer = true;
            Ok(())
        }).await?;

        self.set_lua_async_fn("accept_line", shell, |ui, shell, _lua, _val: LuaValue| async move {
            // TODO error handling
            ui.accept_line(&shell, true).await
        }).await?;

        self.set_lua_async_fn("redraw", shell, |ui, shell, _lua, _val: LuaValue| async move {
            ui.draw(&shell, true).await
        }).await?;

        self.set_lua_async_fn("eval", shell, |_ui, shell, lua, (cmd, stderr): (String, bool)| async move {
            let data = shell.lock().await.eval(&cmd, stderr).unwrap();
            Ok(lua.create_string(data)?)
        }).await?;

        keybind::init_lua(self, shell).await?;
        completion::init_lua(self, shell).await?;
        views::init_lua(self, shell).await?;

        let lua = self.borrow().await.lua.clone();
        lua.load("package.path = '/home/qianli/Documents/wish/lua/?.lua;' .. package.path").exec()?;
        if let Err(err) = lua.load("require('wish')").exec() {
            eprintln!("DEBUG(sliver)\t{}\t= {:?}", stringify!(err), err);
        }

        Ok(())
    }

    async fn clean(&self) {
        self.borrow_mut().await.dirty = UiDirty::default();
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

    fn refresh_cursor_position(&mut self) -> Result<()> {
        self.cursor = loop {
            let now = std::time::SystemTime::now();
            match crossterm::cursor::position() {
                Ok(pos) => break pos,
                Err(e) if now.elapsed().unwrap().as_millis() < 1500 && format!("{}", e) == "The cursor position could not be read within a normal duration" => {
                    // crossterm times out in 2s
                    // but it also fails on EINTR whereas we would like to retry
                },
                Err(e) => return Err(e)?,
            }
        };
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
