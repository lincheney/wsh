use bstr::{BString};

#[derive(Debug)]
pub enum Event {
    Key(super::KeyEvent),
    Mouse(super::MouseEvent),
    BracketedPaste(BString),
    Focus(bool),
    CursorPosition{x: usize, y: usize},
    InvalidUtf8([u8; 4], super::Modifiers),
    Unknown,
}

impl From<super::Key> for Event {
    fn from(key: super::Key) -> Self {
        Self::Key(super::KeyEvent{ key, modifiers: super::Modifiers::NONE })
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum EventIndex {
    Key(super::KeyEvent),
    Mouse{mouse: super::Mouse, modifiers: super::Modifiers},
    Focus(bool),
}

impl TryFrom<&Event> for EventIndex {
    type Error = ();
    fn try_from(value: &Event) -> Result<Self, Self::Error> {
        match value {
            Event::Key(ev) => Ok(Self::Key(*ev)),
            Event::Mouse(ev) => Ok(Self::Mouse{mouse: ev.mouse, modifiers: ev.modifiers}),
            Event::Focus(ev) => Ok(Self::Focus(*ev)),
            _ => Err(()),
        }
    }
}

impl EventIndex {

    pub fn parse_from_label(key: &str) -> anyhow::Result<Self> {
        let mut modifiers = super::Modifiers::empty();

        let original = key;
        let mut key = key;
        let special = key.starts_with('<') && key.ends_with('>');

        if special {
            key = &key[1..key.len() - 1];

            if key.contains('-') {
                // this has modifiers
                for modifier in key.rsplit('-').skip(1) {
                    match modifier {
                        "c" => modifiers |= super::Modifiers::CONTROL,
                        "s" => modifiers |= super::Modifiers::SHIFT,
                        "a" => modifiers |= super::Modifiers::ALT,
                        _ => anyhow::bail!("invalid keybind: {:?}", original),
                    }
                }
                key = key.rsplit('-').next().unwrap();
            }
        }

        if let Some(key) = super::Key::parse_normal_from_label(key) {
            return Ok(Self::Key(super::KeyEvent{key, modifiers}))
        }

        if special {
            if let Some(mouse) = super::Mouse::parse_from_label(key) {
                return Ok(Self::Mouse{mouse, modifiers})
            } else if let Some(key) = super::Key::parse_special_from_label(key) {
                return Ok(Self::Key(super::KeyEvent{key, modifiers}))
            } else {
                match key {
                    "focusin" => return Ok(Self::Focus(true)),
                    "focusout" => return Ok(Self::Focus(false)),
                    _ => (),
                }
            }
        }

        anyhow::bail!("invalid keybind: {:?}", original)
    }
}
