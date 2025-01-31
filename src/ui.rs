use std::time::Duration;
use anyhow::Result;

use crossterm::{
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
    pub lua: mlua::Lua,
    pub lua_api: mlua::Table,

    pub keybinds: keybind::KeybindMapping,
    stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    command: String,
    cursor: (u16, u16),
    size: (u16, u16),
}

impl Ui {
    pub fn new() -> Result<Self> {

        let lua = mlua::Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;

        let ui = Self{
            fanos: fanos::FanosClient::new()?,
            lua,
            lua_api,
            keybinds: keybind::KeybindMapping::default(),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            command: String::new(),
            cursor: crossterm::cursor::position()?,
            size: crossterm::terminal::size()?,
        };

        keybind::init_lua(&ui)?;
        ui.lua.load("package.path = '/home/qianli/Documents/wish/lua/?.lua;' .. package.path").exec()?;
        ui.lua.load("require('wish')").exec()?;

        Ok(ui)
    }

    pub fn draw_prompt(&mut self) -> Result<()> {
        execute!(self.stdout, StrCommand(">>> "), StrCommand(&self.command))?;
        self.cursor = crossterm::cursor::position()?;
        Ok(())
    }

    pub async fn handle_event(&mut self, event: Event) -> Result<bool> {
        // println!("Event::{:?}\r", event);

        match event {

            Event::Key(KeyEvent{
                code: KeyCode::Char(c),
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                self.command.push(c);
                execute!(self.stdout, StrCommand(&self.command[self.command.len() - 1..]))?;
            },

            Event::Key(KeyEvent{
                code: KeyCode::Enter,
                modifiers,
                kind: event::KeyEventKind::Press,
                state: _,
            }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                // time to execute
                self.command.insert_str(0, "EVAL ");
                self.deactivate()?;
                execute!(self.stdout, StrCommand("\r\n"))?;
                self.fanos.send(self.command.as_bytes(), None).await?;
                if ! self.fanos.recv().await? {
                    return Ok(false)
                }
                self.activate()?;
                self.command.clear();
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
