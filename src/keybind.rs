pub mod parser;
pub mod mouse;
pub mod event;
pub mod key;

pub use mouse::{MouseEvent, Mouse};
pub use key::{KeyEvent, Key};
pub use event::{Event, EventIndex};

pub const CONTROL_C_BYTE: u8 = KeyEvent{key: Key::Char('c'), modifiers: Modifiers::CONTROL}.try_into_byte().unwrap();

bitflags::bitflags! {
    #[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
    pub struct Modifiers: u8 {
        const NONE    = 0;
        const SHIFT   = 1;
        const ALT     = 2;
        const CONTROL = 4;
    }
}
