#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Escape,
    Function(u8),
    Insert,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Begin,
    Home,
    End,
    Pageup,
    Pagedown,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: super::Modifiers,
}

impl KeyEvent {
    pub const fn try_into_byte(&self) -> Option<u8> {
        Some(
            match (self.key, self.modifiers) {
                (Key::Char(c), super::Modifiers::NONE) if c.is_ascii() => c as u8,
                (Key::Char(c), super::Modifiers::CONTROL) if c.is_ascii() => {
                    match c {
                        '@'..='~' | ' ' => c as u8 & 0x1f,
                        '2'             => 0,
                        '3'..='7'       => c as u8 - b'3' + b'\x1b',
                        '8' | '?'       => b'\x7f',
                        '-' | '/'       => b'\x1f',
                        _                  => return None,
                    }
                }
                (Key::Enter, super::Modifiers::NONE) => b'\r',
                (Key::Backspace, super::Modifiers::NONE) => 0x7f,
                (Key::Escape, super::Modifiers::NONE) => 0x1b,
                _ => return None,
            }
        )
    }
}

impl Key {
    pub fn parse_special_from_label(key: &str) -> Option<Self> {
        Some(match key {
            "bs" | "backspace" => Self::Backspace,
            "cr" | "enter" => Self::Enter,
            "left" => Self::Left,
            "right" => Self::Right,
            "up" => Self::Up,
            "down" => Self::Down,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" => Self::Pageup,
            "pagedown" => Self::Pagedown,
            "tab" => Self::Char('\t'),
            "delete" => Self::Delete,
            "insert" => Self::Insert,
            "esc" | "escape" => Self::Escape,
            "lt" => Self::Char('<'),
            key if key.starts_with('f') => {
                if let Ok(n) = key[1..].parse() {
                    Self::Function(n)
                } else {
                    return None
                }
            }
            _ => return None,
        })
    }

    pub fn parse_normal_from_label(key: &str) -> Option<Self> {
        if key.len() == 1 && &key[0..1] != "<" && key.is_ascii() {
            Some(Self::Char(key.chars().next().unwrap()))
        } else {
            None
        }
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Key::Char('\t') => write!(f, "tab"),
            Key::Char(c) => write!(f, "{c}"),
            Key::Function(n) => write!(f, "f{n}"),
            Key::Enter => write!(f, "enter"),
            Key::Backspace => write!(f, "backspace"),
            Key::Escape => write!(f, "escape"),
            Key::Insert => write!(f, "insert"),
            Key::Delete => write!(f, "delete"),
            Key::Up => write!(f, "up"),
            Key::Down => write!(f, "down"),
            Key::Left => write!(f, "left"),
            Key::Right => write!(f, "right"),
            Key::Begin => write!(f, "begin"),
            Key::Home => write!(f, "home"),
            Key::End => write!(f, "end"),
            Key::Pageup => write!(f, "pageup"),
            Key::Pagedown => write!(f, "pagedown"),
        }
    }
}
