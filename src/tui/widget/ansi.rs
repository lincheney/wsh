use std::rc::Rc;
use std::ops::Range;
use unicode_width::{UnicodeWidthStr};
use bstr::{BStr, BString, ByteSlice};
use crate::tui::style::{Style, Color, Modifier, Underline};
use crate::tui::text::{Text};

const TAB_SIZE: usize = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum State {
    #[default]
    None,
    Esc,
    Csi,
    CsiParams,
    EscOther,
    CsiOther,
    Apc,
    Osc,
}

#[derive(Debug, Default, Clone)]
pub struct Parser {
    buffer: BString,
    style: Style,
    state: State,
    pub cursor_x: usize,
    pub need_newline: bool,
    pub ocrnl: bool,
    pub captured_sequences: std::cell::RefCell<BString>,
}

pub fn parse_ansi_col(mut style: Style, string: &BStr) -> Style {
    let string = if string.is_empty() {
        b";".as_bstr()
    } else {
        string
    };

    let mut parts = string.split_inclusive(|c| matches!(c, b';' | b':'))
        .map(|part| {
            if part.is_empty() {
                (0, false)
            } else {
                let (part, colon) = if part.ends_with(b":") {
                    (&part[..part.len()-1], true)
                } else if part.ends_with(b";") {
                    (&part[..part.len()-1], false)
                } else {
                    (part, false)
                };
                if part.is_empty() {
                    (0, false)
                } else if let Ok(part) = std::str::from_utf8(part) && let Ok(part) = part.parse() {
                    (part, colon)
                } else {
                    (0, colon)
                }
            }
        });

    while let Some((part, colon)) = parts.next() {
        style = match part {
            0 => Style::default(),
            1 => style.add_modifier(Modifier::BOLD),
            2 => style.add_modifier(Modifier::DIM),
            3 => style.add_modifier(Modifier::ITALIC),
            4 => if colon {
                style.underline = match parts.next() {
                    Some((0, _)) => Some(Underline::None),
                    Some((1, _)) => Some(Underline::Single),
                    Some((2, _)) => Some(Underline::Double),
                    Some((3, _)) => Some(Underline::Curly),
                    Some((4, _)) => Some(Underline::Dashed),
                    Some((5, _)) => Some(Underline::Dotted),
                    _ => Some(Underline::Single),
                };
                style
            } else {
                style.underline = Some(Underline::Single);
                style
            },
            5 => style.add_modifier(Modifier::BLINK),
            7 => style.add_modifier(Modifier::REVERSED),
            8 => style.add_modifier(Modifier::HIDDEN),
            9 => style.add_modifier(Modifier::CROSSED_OUT),
            21 => { style.underline = Some(Underline::Double); style },
            22 => style.remove_modifier(Modifier::BOLD).remove_modifier(Modifier::DIM),
            23 => style.remove_modifier(Modifier::ITALIC),
            24 => { style.underline = Some(Underline::None); style },
            25 => style.remove_modifier(Modifier::BLINK),
            27 => style.remove_modifier(Modifier::REVERSED),
            28 => style.remove_modifier(Modifier::HIDDEN),
            29 => style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style.fg(Color::AnsiValue(part as u8 - 30)),
            39 => style.fg(Color::Reset),
            40..=47 => style.bg(Color::AnsiValue(part as u8 - 30)),
            49 => style.bg(Color::Reset),
            59 => style.underline_color(Color::Reset),
            90..=97 => style.fg(Color::AnsiValue(part as u8 - 90 + 8)),
            100..=107 => style.bg(Color::AnsiValue(part as u8 - 90 + 8)),
            38 => match parts.next() {
                Some((2, _)) => {
                    if let Some(r) = parts.next()
                        && let Ok(r) = r.0.try_into()
                        && let Some(g) = parts.next()
                        && let Ok(g) = g.0.try_into()
                        && let Some(b) = parts.next()
                        && let Ok(b) = b.0.try_into()
                    {
                        style.fg(Color::Rgb { r, g, b })
                    } else {
                        style
                    }
                },
                Some((5, _)) => if let Some(part) = parts.next() && let Ok(part) = part.0.try_into() {
                    style.fg(Color::AnsiValue(part))
                } else {
                    style
                },
                _ => style,
            },
            48 => match parts.next() {
                Some((2, _)) => {
                    if let Some(r) = parts.next()
                        && let Ok(r) = r.0.try_into()
                        && let Some(g) = parts.next()
                        && let Ok(g) = g.0.try_into()
                        && let Some(b) = parts.next()
                        && let Ok(b) = b.0.try_into()
                    {
                        style.bg(Color::Rgb { r, g, b })
                    } else {
                        style
                    }
                },
                Some((5, _)) => if let Some(part) = parts.next() && let Ok(part) = part.0.try_into() {
                    style.bg(Color::AnsiValue(part))
                } else {
                    style
                },
                _ => style,
            },
            58 => match parts.next() {
                Some((2, _)) => {
                    if let Some(r) = parts.next()
                        && let Ok(r) = r.0.try_into()
                        && let Some(g) = parts.next()
                        && let Ok(g) = g.0.try_into()
                        && let Some(b) = parts.next()
                        && let Ok(b) = b.0.try_into()
                    {
                        style.underline_color(Color::Rgb { r, g, b })
                    } else {
                        style
                    }
                },
                Some((5, _)) => if let Some(part) = parts.next() && let Ok(part) = part.0.try_into() {
                    style.underline_color(Color::AnsiValue(part))
                } else {
                    style
                },
                _ => style,
            },
            _ => style,
        }
    }

    style
}

impl Parser {

    pub fn add_line<T>(&mut self, text: &mut Text<T>) {
        text.push_line(b"".into(), None);
        self.cursor_x = 0;
        self.need_newline = false;
    }

    fn add_buffer<T: Default+Clone>(&mut self, text: &mut Text<T>) {
        if !self.buffer.is_empty() {
            self.add_str(text, self.buffer.to_string());
            self.buffer.clear();
        }
    }

    pub fn to_byte_pos<T>(text: &Text<T>, pos: usize) -> usize {
        let Some(line) = text.get().last() else {
            return 0;
        };
        let mut width = 0;
        let mut byte_pos = 0;
        for (s, _, c) in line.grapheme_indices().chain(std::iter::once((line.len(), line.len(), " "))) {
            if width <= pos {
                byte_pos = s;
            }
            width += c.width();
        }
        byte_pos
    }

    fn splice<T: Default+Clone>(&mut self, text: &mut Text<T>, range: Option<Range<usize>>, replace_with: Option<String>, style: Style) {
        let lineno = text.len() - 1;
        let len = text.get()[lineno].len();

        let range = match (range, &replace_with) {
            (Some(range), _) => range,
            (None, Some(replace_with)) => {
                // calculate the range based on the cursor
                Self::to_byte_pos(text, self.cursor_x) .. Self::to_byte_pos(text, self.cursor_x + replace_with.width()).min(len)
            },
            (None, None) => return,
        };

        text.delete_str(lineno, range.start, range.end - range.start);
        if let Some(replace_with) = replace_with {
            text.insert_str(replace_with.as_str().into(), lineno, range.start, false, Some(style.into()));
        }
    }

    fn add_str<T: Default+Clone>(&mut self, text: &mut Text<T>, string: String) {
        if string.is_empty() {
            return
        }

        if self.need_newline || text.get().is_empty() {
            self.add_line(text);
        }

        let width = string.width();
        self.splice(text, None, Some(string), self.style.clone());
        self.cursor_x += width;
    }

    pub fn feed<T: Default+Clone>(&mut self, text: &mut Text<T>, string: &BStr) {
        // we support some csi styling, newlines, tabs and normal text and that's about it

        for c in string.iter() {
            let old_state = self.state;
            self.state = match (old_state, c) {
                (State::None, b'\x1b') => State::Esc,
                (State::Esc, b'[') => State::Csi,
                (State::Esc, b'_') => State::Apc,
                (State::Esc, b']') => State::Osc,

                (State::Apc, b'\x07') => {
                    if self.buffer.starts_with(b"G") {
                        let mut captured = self.captured_sequences.borrow_mut();
                        captured.push(b'\x1b');
                        captured.push(b'_');
                        captured.extend_from_slice(&self.buffer);
                        captured.push(b'\x07');
                    }
                    self.buffer.clear();
                    State::None
                },
                (State::Apc, b'\\') if self.buffer.ends_with(b"\x1b") => {
                    self.buffer.pop(); // remove the ESC
                    if self.buffer.starts_with(b"G") {
                        let mut captured = self.captured_sequences.borrow_mut();
                        captured.push(b'\x1b');
                        captured.push(b'_');
                        captured.extend_from_slice(&self.buffer);
                        captured.push(b'\x1b');
                        captured.push(b'\\');
                    }
                    self.buffer.clear();
                    State::None
                },
                (State::Apc, _) => {
                    self.buffer.push(*c);
                    State::Apc
                },

                (State::Osc, b'\x07') => {
                    self.handle_osc();
                    State::None
                },
                (State::Osc, b'\\') if self.buffer.ends_with(b"\x1b") => {
                    self.buffer.pop(); // remove the ESC
                    self.handle_osc();
                    State::None
                },
                (State::Osc, _) => {
                    self.buffer.push(*c);
                    State::Osc
                },

                (State::Csi | State::CsiParams, b'0'..=b'9' | b';' | b':') => {
                    self.buffer.push(*c);
                    State::CsiParams
                },
                (State::Csi | State::CsiParams, b'm') => {
                    self.style = parse_ansi_col(self.style.clone(), self.buffer.as_ref());
                    self.buffer.clear();
                    State::None
                },
                (State::Csi | State::CsiParams, b'K') => {
                    let param = self.buffer.split(|c| *c == b';').next().unwrap_or(b"");
                    let param = if param.is_empty() { b"0" } else { param };

                    if let Ok(param) = std::str::from_utf8(param)
                        && let Ok(param) = param.parse()
                        && let Some(last_line) = text.get().last()
                    {
                        let cursor_x = Self::to_byte_pos(text, self.cursor_x);
                        match param {
                            0 => {
                                let range = cursor_x .. last_line.len();
                                self.splice(text, Some(range), None, self.style.clone());
                            },
                            1 => {
                                let range = 0 .. cursor_x;
                                let replace_with = " ".repeat(self.cursor_x);
                                self.splice(text, Some(range), Some(replace_with), self.style.clone());
                            },
                            2 => {
                                let range = 0 .. last_line.len();
                                let replace_with = " ".repeat(self.cursor_x);
                                self.splice(text, Some(range), Some(replace_with), self.style.clone());
                            },
                            _ => (),
                        }
                    }
                    self.buffer.clear();
                    State::None
                },
                (State::Csi | State::CsiParams, b'J') => {
                    let param = self.buffer.split(|c| *c == b';' || *c == b':').next().unwrap_or(b"");
                    let param = if param.is_empty() { b"0" } else { param };

                    let param = std::str::from_utf8(param).ok().and_then(|p| p.parse().ok());

                    match param {
                        Some(0) => {
                            // clear from cursor to end of screen
                            if let Some(last_line) = text.get().last() {
                                let cursor_x = Self::to_byte_pos(text, self.cursor_x);
                                let range = cursor_x .. last_line.len();
                                self.splice(text, Some(range), None, self.style.clone());
                            }
                        },
                        Some(1) => {
                            // clear from beginning of screen to cursor
                            for i in 0..text.len() - 1 {
                                text.delete_str(i, 0, text.get()[i].len());
                            }
                            if let Some(_last_line) = text.get().last() {
                                let cursor_x = Self::to_byte_pos(text, self.cursor_x);
                                let range = 0 .. cursor_x;
                                let replace_with = " ".repeat(self.cursor_x);
                                self.splice(text, Some(range), Some(replace_with), self.style.clone());
                            }
                        },
                        Some(2) => {
                            // clear entire screen
                            let lines = text.len();
                            for i in 0..lines {
                                text.delete_str(i, 0, text.get()[i].len());
                            }
                        },
                        _ => (),
                    }
                    self.buffer.clear();
                    State::None
                },
                (State::CsiParams, _) => {
                    self.buffer.clear();
                    State::None
                },

                (State::Esc, b' ' | b'#' | b'%' | b'(' | b')' | b'*' | b'+') => State::EscOther,
                (State::EscOther, _) => State::None,

                (State::Csi, b'?' | b'>' | b'=' | b'!') => State::CsiOther,
                (State::CsiOther, b'0'..=b'9' | b';' | b':') => State::CsiOther,
                (State::CsiOther, _) => State::None,
                (State::Csi, _) => State::None,
                (State::Esc, _) => State::None,

                (_, b'\x08') => {
                    self.add_buffer(text);
                    self.cursor_x = self.cursor_x.saturating_sub(1);
                    State::None
                },
                (State::None, b'\n') => {
                    self.add_buffer(text);
                    self.need_newline = true;
                    State::None
                },
                (State::None, b'\r') => {
                    self.add_buffer(text);
                    if self.ocrnl {
                        self.need_newline = true;
                    } else {
                        self.cursor_x = 0;
                    }
                    State::None
                },
                (State::None, b'\t') => {
                    self.add_buffer(text);
                    let len = TAB_SIZE - self.cursor_x % TAB_SIZE;
                    self.add_str(text, " ".repeat(len));
                    State::None
                },
                (State::None, 0..=0x7f) if !(b' '..=b'~').contains(c) => {
                    // unprintable ascii
                    State::None
                },
                (State::None, _) => {
                    self.buffer.push(*c);
                    State::None
                },
            };

            if old_state == State::None && self.state != State::None {
                self.add_buffer(text);
            }
        }

        if self.state == State::None {
            self.add_buffer(text);
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.state = State::None;
        self.cursor_x = 0;
        self.need_newline = false;
        self.captured_sequences.borrow_mut().clear();
    }

    fn handle_osc(&mut self) {
        if self.buffer.starts_with(b"8;") {
            let mut parts = self.buffer[2..].splitn(2, |c| *c == b';');
            let params = parts.next();
            let url = parts.next();
            if let Some(url) = url {
                if url.is_empty() {
                    self.style.hyperlink = None;
                } else {
                    let mut id = None;
                    if let Some(params) = params {
                        for param in params.split(|c| *c == b':') {
                            if param.starts_with(b"id=") {
                                id = Some(std::str::from_utf8(&param[3..]).unwrap_or("").into());
                                break;
                            }
                        }
                    }
                    self.style.hyperlink = Some(Rc::new(crate::tui::style::Hyperlink {
                        url: std::str::from_utf8(url).unwrap_or("").into(),
                        id,
                    }));
                }
            }
        }
        self.buffer.clear();
    }

    pub fn render<W: std::io::Write, C: crate::tui::Canvas>(&self, drawer: &mut crate::tui::Drawer<W, C>) -> std::io::Result<()> {
        let mut captured = self.captured_sequences.borrow_mut();
        if !captured.is_empty() {
            drawer.write_raw(&captured, None)?;
            captured.clear();
        }
        Ok(())
    }
}
