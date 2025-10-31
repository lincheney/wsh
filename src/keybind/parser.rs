use std::ops::Range;
use std::io::{Write};
use bstr::{BString};
use std::collections::VecDeque;

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    BracketedPaste(BString),
    Focus(bool),
    CursorPosition{x: usize, y: usize},
    InvalidUtf8([u8; 4]),
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

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
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

    pub fn get_one_event<W: Write>(&mut self, dst: &mut W) -> Option<Event> {
        let c = self.buffer.front()?;

        let mut len = 1;
        let event = match c {
            b'\r' | b'\n'       => Key::Enter.into(),
            b'\x7f'             => Key::Backspace.into(),
            b'\t' | b' '..=b'~' => Key::Char((*c).into()).into(),
            b'\x00'..=b'\x1a'   => Event::Key(KeyEvent{ key: Key::Char((c + 0x60).into()), modifiers: KeyModifiers::CONTROL }),
            b'\x1c'..=b'\x1f'   => Event::Key(KeyEvent{ key: Key::Char((c + b'3' - 0x1b).into()), modifiers: KeyModifiers::CONTROL }),

            // utf8
            b'\xc2'..=b'\xf4' => {
                let (array, array_len) = self.extract::<4>(0, 0);
                match std::str::from_utf8(&array[..array_len]) {
                    Ok(s) => Key::Char(s.chars().next().unwrap()).into(),
                    Err(e) => {
                        len = e.error_len()?; // if None, it means its incomplete
                        let mut invalid = [0; 4];
                        invalid.copy_from_slice(&array[..len]);
                        Event::InvalidUtf8(invalid)
                    },
                }
            },

            b'\x1b' => match self.buffer.get(1) {
                Some(b'[') => {
                    // find the end of this escape sequence
                    len = 3 + self.buffer.range(2..).position(|c| !matches!(c, b'0'..=b'9' | b';'))?;
                    log::debug!("DEBUG(fast)  \t{}\t= {:?}", stringify!(self.buffer), self.buffer);

                    // this is none if there are MORE than 4 params
                    if let Some((params, param_len)) = self.read_params::<4>(2 .. len-1) {
                        let params = &params[..param_len];
                        let suffix = self.buffer[len - 1];
                        match (params, suffix) {
                            ([], b'I') => Some(Event::Focus(true)),
                            ([], b'O') => Some(Event::Focus(false)),

                            ([Some(200)], b'~') => {
                                // bracketed paste
                                const PASTE_END: &[u8] = b"\x1b[201~";
                                len = 6 + PASTE_END.len() + self.find(6, PASTE_END)?;
                                Some(Event::BracketedPaste(self.buffer.range(6 .. len - PASTE_END.len()).copied().collect()))
                            },

                            ([Some(y), Some(x)], b'R') => Some(Event::CursorPosition{x: *x, y: *y}),

                            ([Some(0), m @ (None | Some(0..=7))], b'P'..=b'S') => {
                                let modifiers = KeyModifiers::from_bits_truncate(m.map_or(0, |m| m as u8 - b'0'));
                                Some(Event::Key(KeyEvent{ key: Key::Function(suffix - b'P' + 1), modifiers }))
                            },

                            ([Some(1), m @ ..], b'A'..=b'H') if matches!(m, [] | [None | Some(0..=7)]) => {
                                let key = match suffix {
                                    b'A' => Key::Up,
                                    b'B' => Key::Down,
                                    b'C' => Key::Right,
                                    b'D' => Key::Left,
                                    b'E' => Key::Begin,
                                    b'F' => Key::Home,
                                    b'H' => Key::End,
                                    _ => unreachable!(),
                                };
                                let modifiers = m.get(0).unwrap_or(&None).map_or(0, |m| m as u8 - b'0');
                                let modifiers = KeyModifiers::from_bits_truncate(modifiers);
                                Some(Event::Key(KeyEvent{ key, modifiers }))
                            },

                            ([Some(num), m @ .. ], b'~') if matches!(m, [] | [None | Some(0..=7)]) => {

                                let key = match num {
                                    2 => Some(Key::Insert),
                                    3 => Some(Key::Delete),
                                    5 => Some(Key::Pageup),
                                    6 => Some(Key::Pagedown),
                                    15 => Some(Key::Function(5)),
                                    17 => Some(Key::Function(6)),
                                    18 => Some(Key::Function(7)),
                                    19 => Some(Key::Function(8)),
                                    20 => Some(Key::Function(9)),
                                    21 => Some(Key::Function(10)),
                                    23 => Some(Key::Function(11)),
                                    24 => Some(Key::Function(12)),
                                    25 => Some(Key::Function(13)),
                                    26 => Some(Key::Function(14)),
                                    28 => Some(Key::Function(15)),
                                    29 => Some(Key::Function(16)),
                                    31 => Some(Key::Function(17)),
                                    32 => Some(Key::Function(18)),
                                    33 => Some(Key::Function(19)),
                                    34 => Some(Key::Function(20)),
                                    42 => Some(Key::Function(21)),
                                    43 => Some(Key::Function(22)),
                                    44 => Some(Key::Function(23)),
                                    45 => Some(Key::Function(24)),
                                    46 => Some(Key::Function(25)),
                                    47 => Some(Key::Function(26)),
                                    48 => Some(Key::Function(27)),
                                    49 => Some(Key::Function(28)),
                                    50 => Some(Key::Function(29)),
                                    51 => Some(Key::Function(30)),
                                    52 => Some(Key::Function(31)),
                                    53 => Some(Key::Function(32)),
                                    54 => Some(Key::Function(33)),
                                    55 => Some(Key::Function(34)),
                                    56 => Some(Key::Function(35)),
                                    _ => None,
                                };

                                key.map(|key| {
                                    let modifiers = m.get(0).unwrap_or(&None).map_or(0, |m| m as u8 - b'0');
                                    let modifiers = KeyModifiers::from_bits_truncate(modifiers);
                                    Event::Key(KeyEvent{ key, modifiers })
                                })
                            },

                            _ => None,
                        }
                    } else {
                        None
                    }
                },
                Some(b'O') => {
                    let (array, array_len) = self.extract::<2>(0, 0);
                    Some(match &array {
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
                    })
                },
                // otherwise treat as a single escape key
                _ => Some(Key::Escape.into()),
            }.unwrap_or(Event::Unknown),

            _ => Event::Unknown,
        };

        for c in self.buffer.drain(..len) {
            dst.write_all(&[c]).unwrap();
        }
        Some(event)
    }
}
