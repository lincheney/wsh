use std::time::Duration;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use mlua::{IntoLuaMulti, FromLuaMulti, Lua, Result as LuaResult};
use anyhow::Result;
use paste::paste;

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::position,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    queue,
};

use crate::fanos;
use crate::keybind;

struct StrCommand<'a>(&'a str);

impl crossterm::Command for StrCommand<'_> {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

pub struct Ui {
    fanos: fanos::FanosClient,
    pub lua: Lua,
    pub lua_api: mlua::Table,
    lua_cache: mlua::Table,
    state: Rc<RefCell<UiStateInner>>,

    stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    cursor: (u16, u16),
    size: (u16, u16),
}

pub trait LuaFn<A: FromLuaMulti, R: IntoLuaMulti>: Fn(&UiState, &Lua, A) -> LuaResult<R> + mlua::MaybeSend + 'static {}
impl<A: FromLuaMulti, R: IntoLuaMulti, F: Fn(&UiState, &Lua, A) -> LuaResult<R> + mlua::MaybeSend + 'static> LuaFn<A, R> for F {}

pub fn set_lua_fn<F: LuaFn<A, R>, A: FromLuaMulti, R: IntoLuaMulti>(
    state: &UiState,
    lua: &Lua,
    table: &mlua::Table,
    name: &str,
    func: F,
) -> LuaResult<()> {
    let state = Rc::downgrade(&state);
    table.set(name, lua.create_function(move |lua, value| {
        if let Some(state) = state.upgrade() {
            func(&state, lua, value)
        } else {
            Err(mlua::Error::RuntimeError("ui not running".to_string()))
        }
    })?)
}

#[derive(Debug, Default)]
struct UiDirtyState {
    buffer: bool,
}

#[derive(Debug, Default)]
pub struct UiStateInner {
    pub keybinds: keybind::KeybindMapping,
    dirty: UiDirtyState,

    pub buffer: crate::buffer::Buffer,
}

impl UiStateInner {

    fn init_lua(state: &UiState, lua: &Lua, lua_api: &mlua::Table) -> LuaResult<()> {
        set_lua_fn(state, lua, lua_api, "__get_cursor", |state, _lua, _val: mlua::Value| Ok(state.borrow().buffer.get_cursor()))?;
        set_lua_fn(state, lua, lua_api, "__get_buffer", |state, _lua, _val: mlua::Value| Ok(state.borrow().buffer.get_contents().clone()))?;

        set_lua_fn(state, lua, lua_api, "__set_cursor", |state, _lua, val: usize| {
            let mut state = state.borrow_mut();
            state.buffer.set_cursor(val);
            state.dirty.buffer = true;
            Ok(())
        })?;
        set_lua_fn(state, lua, lua_api, "__set_buffer", |state, _lua, val: String| {
            let mut state = state.borrow_mut();
            state.buffer.set_contents(val);
            state.dirty.buffer = true;
            Ok(())
        })?;

        Ok(())
    }

    fn clean(&mut self) {
        self.dirty = UiDirtyState::default();
    }

}

pub type UiState = Rc<RefCell<UiStateInner>>;

impl Ui {
    pub fn new() -> Result<Self> {

        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;
        let lua_cache = lua.create_table()?;
        lua_api.set("__cache", &lua_cache)?;

        let ui = Self{
            fanos: fanos::FanosClient::new()?,
            lua,
            lua_api,
            lua_cache,
            state: Rc::new(RefCell::new(UiStateInner::default())),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            cursor: crossterm::cursor::position()?,
            size: crossterm::terminal::size()?,
        };

        init_lua(&ui)?;

        Ok(ui)
    }

    pub fn draw_prompt(&mut self) -> Result<()> {
        let state = self.state.borrow();
        queue!(
            self.stdout,
            BeginSynchronizedUpdate,
            StrCommand("\r"),
            Clear(ClearType::FromCursorDown),
            StrCommand(">>> "),
            StrCommand(&state.buffer.get_contents()),
        )?;
        let offset = state.buffer.get_contents().len() - state.buffer.get_cursor();
        if offset > 0 {
            queue!(self.stdout, crossterm::cursor::MoveLeft(offset as u16))?;
        }
        queue!(self.stdout, EndSynchronizedUpdate)?;
        execute!(self.stdout)?;
        self.cursor = crossterm::cursor::position()?;
        Ok(())
    }

    pub async fn handle_event(&mut self, event: Event) -> Result<bool> {
        // println!("Event::{:?}\r", event);

        if let Event::Key(KeyEvent{code, modifiers, ..}) = event {
            let callback = self.state.borrow().keybinds.get(&(code, modifiers)).cloned();
            if let Some(callback) = callback {
                if let Err(err) = callback.call::<mlua::Value>(mlua::Nil) {
                    eprintln!("DEBUG(loaf)  \t{}\t= {:?}", stringify!(err), err);
                }
                self.refresh_on_state()?;
                return Ok(true)
            }
        }

        match event {

            Event::Key(KeyEvent{
                code: KeyCode::Char(c),
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                let need_redraw = {
                    let mut state = self.state.borrow_mut();
                    state.buffer.mutate(|contents, cursor| -> Result<bool> {
                        contents.insert(*cursor, c);
                        *cursor += 1;
                        if *cursor == contents.len() {
                            execute!(self.stdout, StrCommand(&contents[contents.len() - 1..]))?;
                            Ok(false)
                        } else {
                            Ok(true)
                        }
                    })?
                };

                if need_redraw {
                    self.draw_prompt()?;
                }
            },

            Event::Key(KeyEvent{
                code: KeyCode::Enter,
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                self.deactivate()?;
                // new line
                execute!(self.stdout, StrCommand("\r\n"))?;
                // time to execute
                {
                    let state = self.state.borrow();
                    self.fanos.eval(state.buffer.get_contents(), None).await?;
                    if ! self.fanos.recv().await? {
                        return Ok(false)
                    }
                }
                self.state.borrow_mut().buffer.reset();
                self.activate()?;
                self.draw_prompt()?;
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

    pub fn set_lua_fn<F: LuaFn<A, R>, A: FromLuaMulti, R: IntoLuaMulti>(&self, name: &str, func: F) -> LuaResult<()> {
        set_lua_fn(&self.state, &self.lua, &self.lua_api, name, func)
    }

    pub fn refresh_on_state(&mut self) -> Result<()> {
        if self.state.borrow().dirty.buffer {
            self.draw_prompt()?;
        }

        self.state.borrow_mut().clean();
        self.lua_cache.clear()?;
        Ok(())
    }

}

fn init_lua(ui: &Ui) -> Result<()> {

    UiStateInner::init_lua(&ui.state, &ui.lua, &ui.lua_api)?;

    keybind::init_lua(&ui)?;
    ui.lua.load("package.path = '/home/qianli/Documents/wish/lua/?.lua;' .. package.path").exec()?;
    if let Err(err) = ui.lua.load("require('wish')").exec() {
        eprintln!("DEBUG(sliver)\t{}\t= {:?}", stringify!(err), err);
    }
    Ok(())
}

impl Drop for Ui {
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
