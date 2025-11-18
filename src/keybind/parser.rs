use std::ops::Range;
use bstr::{BString};
use std::collections::VecDeque;

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    BracketedPaste(BString),
    Focus(bool),
    CursorPosition{x: usize, y: usize},
    InvalidUtf8([u8; 4], KeyModifiers),
    Unknown,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: KeyModifiers,
}

bitflags::bitflags! {
    #[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
    pub struct KeyModifiers: u8 {
        const NONE    = 0;
        const SHIFT   = 1;
        const ALT     = 2;
        const CONTROL = 4;
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Button(usize),
}

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
    MouseButton{x: usize, y: usize, button: MouseButton, release: bool},
    MouseMove{x: usize, y: usize, button: MouseButton},
    MouseScroll{x: usize, y: usize, down: bool},
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
            Key::MouseButton{button, release, ..} => match button {
                MouseButton::Left if *release => write!(f, "leftmouse-release"),
                MouseButton::Left => write!(f, "leftmouse"),
                MouseButton::Right if *release => write!(f, "rightmouse-release"),
                MouseButton::Right => write!(f, "rightmouse"),
                MouseButton::Middle if *release => write!(f, "middlemouse-release"),
                MouseButton::Middle => write!(f, "middlemouse"),
                MouseButton::Button(n) if *release => write!(f, "button{n}-release"),
                MouseButton::Button(n) => write!(f, "button{n}"),
            },
            Key::MouseMove{button, ..} => match button {
                MouseButton::Left => write!(f, "leftmouse-move"),
                MouseButton::Right => write!(f, "rightmouse-move"),
                MouseButton::Middle => write!(f, "middlemouse-move"),
                MouseButton::Button(n) => write!(f, "button{n}-move"),
            },
            Key::MouseScroll{down, ..} if *down => write!(f, "scrolldown"),
            Key::MouseScroll{..} => write!(f, "scrollup"),
        }
    }
}

impl From<Key> for Event {
    fn from(key: Key) -> Self {
        Self::Key(KeyEvent{ key, modifiers: KeyModifiers::NONE })
    }
}

pub struct Parser {
    buffer: VecDeque<u8>,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.buffer.extend(bytes.iter());
    }

    // extract (up to) a fixed size array
    fn extract<const N: usize>(&self, start: usize, default: u8) -> ([u8; N], usize) {
        let mut buf = [default; N];
        for (c, b) in self.buffer.range(start..).zip(buf.iter_mut()) {
            *b = *c;
        }
        (buf, self.buffer.len().saturating_sub(start))
    }

    // parse a fixed number of semicolon delimited params
    fn read_params<const N: usize>(&self, range: Range<usize>) -> Option<([Option<usize>; N], usize)> {
        let mut len = 0;
        let mut buf = [None; N];
        let mut range = self.buffer.range(range);
        for (i, b) in buf.iter_mut().enumerate() {
            let mut prev0 = false;
            let x = b.get_or_insert(0);
            loop {
                match range.next() {
                    // no more chars
                    None => return Some((buf, len)),
                    Some(c) => {
                        len = i + 1;
                        if *c == b';' {
                            break
                        }
                        if prev0 {
                            // leading 0s, is that allowed?
                            return None
                        }
                        *x = *x * 10 + (c - b'0') as usize;
                        prev0 = *x == 0;
                    },
                }
            }
        }
        if range.next().is_none() {
            Some((buf, len))
        } else {
            // some trailing chars
            None
        }
    }

    fn find(&self, start: usize, needle: &[u8]) -> Option<usize> {
        let (mut left, mut right) = self.buffer.as_slices();
        right = &right[start.saturating_sub(left.len())..];
        left = &left[start.min(left.len())..];

        left.windows(needle.len()).position(|x| x == needle)
            .or_else(|| {
                // search in the overlap of left and right
                for i in (1..needle.len()).rev() {
                    if left.ends_with(&needle[..i]) && right.starts_with(&needle[i..]) {
                        return Some(left.len() - i)
                    }
                }
                None
            }).or_else(|| {
                right.windows(needle.len()).position(|x| x == needle)
            })
    }

    fn parse_sgr_mouse(&self) -> Option<(Event, usize)> {
        // assuming prefix is \x1b[<

        // find the end of this escape sequence
        let len = 4 + self.buffer.range(3..).position(|c| !matches!(c, b'0'..=b'9' | b';'))?;
        let release = match self.buffer[len-1] {
            b'M' => false,
            b'm' => true,
            _ => return Some((Event::Unknown, len)),
        };

        let Some(([Some(button), Some(x), Some(y), ..], 3)) = self.read_params::<4>(3 .. len-1)
            else {
                return Some((Event::Unknown, len))
            };
        let x = x.saturating_sub(1);
        let y = y.saturating_sub(1);

        let mut modifiers = KeyModifiers::NONE;
        if button & 4 > 0 {
            modifiers.insert(KeyModifiers::SHIFT);
        }
        if button & 8 > 0 {
            modifiers.insert(KeyModifiers::ALT);
        }
        if button & 16 > 0 {
            modifiers.insert(KeyModifiers::CONTROL);
        }
        let button = button & !4 & !8 & !16 & !32;
        let mouse = match button {
            64 | 65 => return Some((Event::Key(KeyEvent{ key: Key::MouseScroll{x, y, down: button == 65}, modifiers }), len)),

            0  => MouseButton::Left,
            1  => MouseButton::Middle,
            2  => MouseButton::Right,
            n  => MouseButton::Button(n & !64 & !128),
        };

        let event = if button & 32 > 0 {
            Key::MouseMove{x, y, button: mouse}
        } else {
            Key::MouseButton{x, y, button: mouse, release}
        };

        Some((Event::Key(KeyEvent{ key: event, modifiers }), len))
    }

    fn parse_csi(&self) -> Option<(Event, usize)> {
        // assuming prefix is \x1b[

        // find the end of this escape sequence
        let mut len = 3 + self.buffer.range(2..).position(|c| !matches!(c, b'0'..=b'9' | b';'))?;

        // this is none if there are MORE than 4 params
        let Some((params, param_len)) = self.read_params::<4>(2 .. len-1)
            else {
                return Some((Event::Unknown, len))
            };

        let params = &params[..param_len];
        let suffix = self.buffer[len - 1];
        let event;
        let event = match (params, suffix) {
            ([], b'I') => Event::Focus(true),
            ([], b'O') => Event::Focus(false),

            ([], b'<') => {
                (event, len) = self.parse_sgr_mouse()?;
                event
            },

            ([Some(200)], b'~') => {
                // bracketed paste
                const PASTE_END: &[u8] = b"\x1b[201~";
                len = 6 + PASTE_END.len() + self.find(6, PASTE_END)?;
                Event::BracketedPaste(self.buffer.range(6 .. len - PASTE_END.len()).copied().collect())
            },

            ([Some(y), Some(x)], b'R') => Event::CursorPosition{x: x.saturating_sub(1), y: y.saturating_sub(1)},

            ([Some(0), m @ (None | Some(1..=8))], b'P'..=b'S') => {
                let modifiers = KeyModifiers::from_bits_truncate(m.unwrap_or(1) as u8 - 1);
                Event::Key(KeyEvent{ key: Key::Function(suffix - b'P' + 1), modifiers })
            },

            (m @ ([] | [Some(1)] | [Some(1), None | Some(1..=8)]), b'A'..=b'H') => {
                let key = match suffix {
                    b'A' => Key::Up,
                    b'B' => Key::Down,
                    b'C' => Key::Right,
                    b'D' => Key::Left,
                    b'E' => Key::Begin,
                    b'F' => Key::End,
                    b'H' => Key::Home,
                    _ => unreachable!(),
                };
                let modifiers = m.get(1).unwrap_or(&None).unwrap_or(1) - 1;
                let modifiers = KeyModifiers::from_bits_truncate(modifiers as _);
                Event::Key(KeyEvent{ key, modifiers })
            },

            ([Some(num), m @ .. ], b'~') if matches!(m, [] | [None | Some(1..=8)]) => {

                let key = match num {
                    2 => Key::Insert,
                    3 => Key::Delete,
                    5 => Key::Pageup,
                    6 => Key::Pagedown,
                    15 => Key::Function(5),
                    17 => Key::Function(6),
                    18 => Key::Function(7),
                    19 => Key::Function(8),
                    20 => Key::Function(9),
                    21 => Key::Function(10),
                    23 => Key::Function(11),
                    24 => Key::Function(12),
                    25 => Key::Function(13),
                    26 => Key::Function(14),
                    28 => Key::Function(15),
                    29 => Key::Function(16),
                    31 => Key::Function(17),
                    32 => Key::Function(18),
                    33 => Key::Function(19),
                    34 => Key::Function(20),
                    42 => Key::Function(21),
                    43 => Key::Function(22),
                    44 => Key::Function(23),
                    45 => Key::Function(24),
                    46 => Key::Function(25),
                    47 => Key::Function(26),
                    48 => Key::Function(27),
                    49 => Key::Function(28),
                    50 => Key::Function(29),
                    51 => Key::Function(30),
                    52 => Key::Function(31),
                    53 => Key::Function(32),
                    54 => Key::Function(33),
                    55 => Key::Function(34),
                    56 => Key::Function(35),
                    _ => return Some((Event::Unknown, len)),
                };

                let modifiers = m.first().unwrap_or(&None).unwrap_or(1) - 1;
                let modifiers = KeyModifiers::from_bits_truncate(modifiers as _);
                Event::Key(KeyEvent{ key, modifiers })
            },

            _ => Event::Unknown,
        };
        Some((event, len))
    }

    fn parse_char(&self, start: usize, modifiers: KeyModifiers) -> Option<(Event, usize)> {
        let Some(c) = self.buffer.get(start)
            else { return Some((Event::Unknown, 0)) }; // incomplete

        let mut len = 1;
        let key = match c {
            b'\x7f'             => Key::Backspace,
            b'\r' | b'\n'       => Key::Enter,
            b'\t' | b' '..=b'~' => Key::Char((*c).into()),
            // utf8
            b'\xc2'..=b'\xf4' => {
                let (array, array_len) = self.extract::<4>(start, 0);
                match std::str::from_utf8(&array[..array_len]) {
                    Ok(s) => {
                        len = s.len();
                        let c = s.chars().next().unwrap();
                        Key::Char(c)
                    },
                    Err(e) => {
                        let Some(len) = e.error_len()
                            else { return Some((Event::Unknown, 0)) }; // incomplete

                        let mut invalid = [0; 4];
                        invalid.copy_from_slice(&array[..len]);
                        return Some((Event::InvalidUtf8(invalid, modifiers), len))
                    },
                }
            },
            _ => return None,
        };
        let event = Event::Key(KeyEvent{ key, modifiers });
        Some((event, len))
    }

    pub fn get_one_event(&mut self) -> Option<(Event, BString)> {
        let mut len = 1;

        let c = self.buffer.front()?;
        let event;
        let event = match self.parse_char(0, KeyModifiers::NONE) {

            Some((_, 0)) => return None,
            Some((e, l)) => { len = l; e },

            None => match c {
                b'\x00'..=b'\x1a'   => Event::Key(KeyEvent{ key: Key::Char((c + 0x60).into()), modifiers: KeyModifiers::CONTROL }),
                b'\x1c'..=b'\x1f'   => Event::Key(KeyEvent{ key: Key::Char((c + b'3' - 0x1b).into()), modifiers: KeyModifiers::CONTROL }),

                b'\x1b' => match self.buffer.get(1) {
                    Some(b'[') => {
                        (event, len) = self.parse_csi()?;
                        event
                    },
                    Some(b'O') => {
                        let (array, array_len) = self.extract::<2>(2, 0);
                        match &array {
                            [c @ b'P'..=b'S', _] => { len = 3; Key::Function(c - b'P' + 1).into() },
                            [b'I', _] => { len = 3; Key::Char('\t').into() },
                            [b' ', _] => { len = 3; Key::Char(' ').into() },
                            [b'j', _] => { len = 3; Key::Char('*').into() },
                            [b'k', _] => { len = 3; Key::Char('+').into() },
                            [b'l', _] => { len = 3; Key::Char(',').into() },
                            [b'm', _] => { len = 3; Key::Char('-').into() },
                            [b'n', _] => { len = 3; Key::Char('.').into() },
                            [b'o', _] => { len = 3; Key::Char('/').into() },
                            [b'X', _] => { len = 3; Key::Char('=').into()},
                            [b'M', _] => { len = 3; Key::Enter.into() },
                            [b'F', _] => { len = 3; Key::End.into() },
                            [b'H', _] => { len = 3; Key::Home.into() },
                            [b'2', b'~'] => { len = 4; Key::Insert.into() },
                            [b'3', b'~'] => { len = 4; Key::Delete.into() },
                            _ if array_len == 0 => return None,
                            _ => { len = 3; Event::Unknown },
                        }
                    },
                    // no more data, probably just a single escape key
                    None => Key::Escape.into(),
                    // check for alt-key otherwise treat as a single escape key
                    _ => match self.parse_char(1, KeyModifiers::ALT) {
                        Some((_, 0)) => return None,
                        Some((event, l)) => {
                            len = l + 1;
                            event
                        },
                        None => Key::Escape.into(),
                    },
                },

                _ => Event::Unknown,
            },
        };

        let buf = self.buffer.drain(..len).collect();
        Some((event, buf))
    }

    pub fn iter(&mut self) -> impl Iterator<Item=(Event, BString)> {
        std::iter::from_fn(|| self.get_one_event())
    }

}
