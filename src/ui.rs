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

macro_rules! make_ui_state_struct {
    ($($field:ident: $type:ty),*) => (
        paste! {

            #[derive(Debug, Default)]
            pub struct UiStateInner {
                pub keybinds: keybind::KeybindMapping,
                $(
                    $field: $type,
                    [<dirty_ $field>]: bool,
                )*
            }

            impl UiStateInner {

                fn init_lua(state: &UiState, lua: &Lua, lua_api: &mlua::Table) -> LuaResult<()> {
                    $(
                        set_lua_fn(state, lua, lua_api, concat!("get_", stringify!($field)), move |state, _lua, _value: mlua::Value| {
                            Ok(state.borrow().$field.clone())
                        })?;

                        set_lua_fn(state, lua, lua_api, concat!("set_", stringify!($field)), move |state, _lua, value| {
                            let mut state = state.borrow_mut();
                            state.$field = value;
                            state.[<dirty_ $field>] = true;
                            Ok(())
                        })?;
                    )*
                    Ok(())
                }

                fn clean(&mut self) {
                    $(
                    self.[<dirty_ $field>] = false;
                    )*
                }

            }
        }
    )
}

make_ui_state_struct!(buffer: String, cursor: u16);
pub type UiState = Rc<RefCell<UiStateInner>>;

impl Ui {
    pub fn new() -> Result<Self> {

        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;

        let ui = Self{
            fanos: fanos::FanosClient::new()?,
            lua,
            lua_api,
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
            StrCommand(&state.buffer),
        )?;
        let offset = state.buffer.len() as u16 - state.cursor;
        if offset > 0 {
            queue!(self.stdout, crossterm::cursor::MoveLeft(offset))?;
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
                callback.call(mlua::Nil)?;
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
                    let cursor = state.cursor as usize;
                    state.buffer.insert(cursor, c);
                    state.cursor += 1;
                    if state.cursor as usize == state.buffer.len() {
                        execute!(self.stdout, StrCommand(&state.buffer[state.buffer.len() - 1..]))?;
                        false
                    } else {
                        true
                    }
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
                // time to execute
                {
                    let mut state = self.state.borrow_mut();
                    state.buffer.insert_str(0, "EVAL ");
                    execute!(self.stdout, StrCommand("\r\n"))?;
                    self.fanos.send(state.buffer.as_bytes(), None).await?;
                    if ! self.fanos.recv().await? {
                        return Ok(false)
                    }
                    state.buffer.clear();
                    state.cursor = 0;
                }
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
        {
            // fix the cursor
            let mut state = self.state.borrow_mut();
            if state.cursor > state.buffer.len() as u16 {
                state.cursor = state.buffer.len() as u16;
            }
        }

        if {
            let state = self.state.borrow();
            state.dirty_cursor || state.dirty_buffer
        } {
            self.draw_prompt()?;
        }

        self.state.borrow_mut().clean();
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
