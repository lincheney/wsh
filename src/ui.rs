use std::time::Duration;
use anyhow::Result;

use crossterm::{
    cursor::position,
    event::{
        poll,
        KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
        read,
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange,
        EnableMouseCapture,
        Event,
        KeyCode,
    },
    execute,
    queue,
};

const HELP: &str = r#"Blocking read()
 - Keyboard, mouse, focus and terminal resize events enabled
 - Hit "c" to print current cursor position
 - Use Esc to quit
"#;

pub struct Ui {
    stdout: std::io::Stdout,
    enhanced_keyboard: bool,
}

impl Ui {
    pub fn new() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;

        let mut stdout = std::io::stdout();

        let enhanced_keyboard = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
        if enhanced_keyboard {
            queue!(
                stdout,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                )
            )?;
        }

        execute!(
            stdout,
            EnableBracketedPaste,
            EnableFocusChange,
            EnableMouseCapture,
        )?;

        Ok(Self{
            stdout,
            enhanced_keyboard,
        })
    }

    fn finish(&mut self) -> Result<()> {
        if self.enhanced_keyboard {
            queue!(self.stdout, PopKeyboardEnhancementFlags)?;
        }

        execute!(
            self.stdout,
            DisableBracketedPaste,
            DisableFocusChange,
            DisableMouseCapture
        )?;

        crossterm::terminal::disable_raw_mode()?;
        Ok(())
    }

}

impl Drop for Ui {
    fn drop(&mut self) {
        self.finish();
    }
}

fn print_events() -> Result<()> {
    loop {
        // Blocking read
        let event = read()?;

        println!("Event: {:?}\r", event);

        if event == Event::Key(KeyCode::Char('c').into()) {
            println!("Cursor position: {:?}\r", position());
        }

        if let Event::Resize(x, y) = event {
            let (original_size, new_size) = flush_resize_events((x, y));
            println!("Resize from: {:?}, to: {:?}\r", original_size, new_size);
        }

        if event == Event::Key(KeyCode::Esc.into()) {
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
    while let Ok(true) = poll(Duration::from_millis(50)) {
        if let Ok(Event::Resize(x, y)) = read() {
            last_resize = (x, y);
        }
    }

    (first_resize, last_resize)
}
