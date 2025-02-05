use std::time::Duration;
use std::future::Future;
use std::cell::{RefCell, Ref, RefMut};
use std::io::{Write};
use std::rc::{Rc, Weak};
use std::ops::DerefMut;
use mlua::{IntoLuaMulti, FromLuaMulti, Lua, Result as LuaResult};
use anyhow::Result;

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::position,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    queue,
};

use crate::keybind;
use crate::zsh;

struct StrCommand<'a>(&'a str);

impl crossterm::Command for StrCommand<'_> {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Debug, Default)]
struct UiDirty {
    buffer: bool,
}

pub struct UiInner {
    shell: crate::shell::Shell,
    pub lua: Lua,
    pub lua_api: mlua::Table,
    lua_cache: mlua::Table,

    dirty: UiDirty,
    pub keybinds: keybind::KeybindMapping,
    pub buffer: crate::buffer::Buffer,

    stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    cursor: (u16, u16),
    size: (u16, u16),
}

pub struct Ui(Rc<RefCell<UiInner>>);

impl Ui {

    pub fn new() -> Result<Self> {
        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;
        let lua_cache = lua.create_table()?;
        lua_api.set("__cache", &lua_cache)?;

        let ui = Self(Rc::new(RefCell::new(UiInner{
            shell: crate::shell::Shell::new()?,
            lua,
            lua_api,
            lua_cache,
            dirty: UiDirty::default(),
            buffer: std::default::Default::default(),
            keybinds: std::default::Default::default(),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            cursor: crossterm::cursor::position()?,
            size: crossterm::terminal::size()?,
        })));

        ui.init_lua()?;

        Ok(ui)
    }

    pub fn borrow(&self) -> Ref<UiInner> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<UiInner> {
        self.0.borrow_mut()
    }

    pub fn activate(&self) -> Result<()> {
        self.borrow_mut().activate()
    }

    pub fn deactivate(&self) -> Result<()> {
        self.borrow_mut().deactivate()
    }

    pub async fn draw(&self) -> Result<()> {
        let mut ui = self.borrow_mut();
        let ui = ui.deref_mut();

        execute!(
            ui.stdout,
            BeginSynchronizedUpdate,
            StrCommand("\r"),
            Clear(ClearType::FromCursorDown),
        )?;

        let prompt = ui.shell.exec("print -v tmpvar -P \"$PROMPT\" 2>/dev/null", None).await.ok()
            .and_then(|_| zsh::Variable::get("tmpvar").map(|mut v| v.to_bytes()));

        let prompt = prompt.as_ref().map(|p| &p[..]).unwrap_or(b">>> ");
        // let prompt = ui.shell.eval(stringify!(printf %s "${PS1@P}"), false).await?;
        ui.stdout.write(prompt)?;
        ui.stdout.write(ui.buffer.get_contents().as_bytes())?;

        let offset = ui.buffer.get_contents().len() - ui.buffer.get_cursor();
        if offset > 0 {
            queue!(ui.stdout, crossterm::cursor::MoveLeft(offset as u16))?;
        }
        queue!(ui.stdout, EndSynchronizedUpdate)?;
        execute!(ui.stdout)?;
        ui.cursor = crossterm::cursor::position()?;
        Ok(())
    }

    pub async fn handle_event(&self, event: Event) -> Result<bool> {
        // println!("Event::{:?}\r", event);

        if let Event::Key(KeyEvent{code, modifiers, ..}) = event {
            let callback = self.borrow().keybinds.get(&(code, modifiers)).cloned();
            if let Some(callback) = callback {
                if let Err(err) = callback.call_async::<mlua::Value>(mlua::Nil).await {
                    eprintln!("DEBUG(loaf)  \t{}\t= {:?}", stringify!(err), err);
                }
                if self.borrow().shell.closed {
                    return Ok(false)
                } else {
                    self.refresh_on_state().await?;
                    return Ok(true)
                }
            }
        }

        match event {

            Event::Key(KeyEvent{
                code: KeyCode::Char(c),
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                let no_redraw = {
                    let mut ui = self.borrow_mut();

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
                    let mut ui = self.borrow_mut();
                    let ui = ui.deref_mut();
                    let contents = ui.buffer.get_contents();
                    execute!(ui.stdout, StrCommand(&contents[contents.len() - 1 ..]))?;
                } else {
                    self.draw().await?;
                }
            },

            Event::Key(KeyEvent{
                code: KeyCode::Enter,
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                return self.accept_line().await;
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

    async fn accept_line(&self) -> Result<bool> {
        {
            let mut ui = self.borrow_mut();
            let ui = ui.deref_mut();
            ui.deactivate()?;
            // new line
            execute!(ui.stdout, StrCommand("\r\n"))?;
            // time to execute
            if let Err(code) = ui.shell.exec(ui.buffer.get_contents(), None).await {
                eprintln!("DEBUG(atlas) \t{}\t= {:?}", stringify!(code), code);
            }
            // if ! ui.fanos.recv().await? {
                // return Ok(false)
            // }
            ui.buffer.reset();
            ui.activate()?;
        }

        self.draw().await?;
        Ok(true)
    }

    fn try_upgrade(ui: &Weak<RefCell<UiInner>>) -> LuaResult<Self> {
        if let Some(ui) = ui.upgrade() {
            Ok(Ui(ui))
        } else {
            Err(mlua::Error::RuntimeError("ui not running".to_string()))
        }
    }

    pub fn set_lua_fn<F, A, R>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(&Self, &Lua, A) -> LuaResult<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {

        let weak = Rc::downgrade(&self.0);
        let ui = self.borrow();
        ui.lua_api.set(name, ui.lua.create_function(move |lua, value| {
            func(&Ui::try_upgrade(&weak)?, lua, value)
        })?)
    }

    pub fn set_lua_async_fn<F, A, R, T>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(Self, Lua, A) -> T + mlua::MaybeSend + 'static + Clone,
        A: FromLuaMulti + 'static,
        R: IntoLuaMulti,
        T: Future<Output=LuaResult<R>> + mlua::MaybeSend + 'static,
    {
        let weak = Rc::downgrade(&self.0);
        let ui = self.borrow();
        ui.lua_api.set(name, ui.lua.create_async_function(move |lua, value| {
            let weak = weak.clone();
            let func = func.clone();
            async move {
                let ui = Ui::try_upgrade(&weak)?;
                func(ui, lua, value).await
            }
        })?)
    }

    pub async fn refresh_on_state(&self) -> Result<()> {
        if self.borrow().dirty.buffer {
            self.draw().await?;
        }

        self.clean();
        Ok(())
    }

    fn init_lua(&self) -> Result<()> {
        self.set_lua_fn("__get_cursor", |ui, _lua, _val: mlua::Value| Ok(ui.borrow().buffer.get_cursor()))?;
        self.set_lua_fn("__get_buffer", |ui, _lua, _val: mlua::Value| Ok(ui.borrow().buffer.get_contents().clone()))?;

        self.set_lua_fn("__set_cursor", |ui, _lua, val: usize| {
            let mut ui = ui.borrow_mut();
            ui.buffer.set_cursor(val);
            ui.dirty.buffer = true;
            Ok(())
        })?;
        self.set_lua_fn("__set_buffer", |ui, _lua, val: String| {
            let mut ui = ui.borrow_mut();
            ui.buffer.set_contents(val);
            ui.dirty.buffer = true;
            Ok(())
        })?;

        self.set_lua_async_fn("accept_line", |ui, _lua, _val: mlua::Value| async move {
            // TODO error handling
            ui.accept_line().await;
            Ok(())
        })?;

        self.set_lua_async_fn("eval", |ui, lua, (cmd, stderr): (String, bool)| async move {
            let data = ui.borrow_mut().shell.eval(&cmd, stderr).await.unwrap();
            lua.create_string(data)
        })?;

        keybind::init_lua(self)?;

        let lua = self.borrow().lua.clone();
        lua.load("package.path = '/home/qianli/Documents/wish/lua/?.lua;' .. package.path").exec()?;
        if let Err(err) = lua.load("require('wish')").exec() {
            eprintln!("DEBUG(sliver)\t{}\t= {:?}", stringify!(err), err);
        }

        Ok(())
    }

    fn clean(&self) {
        self.borrow_mut().dirty = UiDirty::default();
    }

}

impl UiInner {
    pub fn activate(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;

        if self.enhanced_keyboard {
            queue!(
                self.stdout,
                event::PushKeyboardEnhancementFlags(
                    event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | event::KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    | event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                    | event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                )
            )?;
        }

        execute!(
            self.stdout,
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
