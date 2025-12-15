use std::ops::Range;
use unicode_width::{UnicodeWidthStr};
use bstr::{BStr, BString, ByteSlice};
use ratatui::{
    style::*,
};

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
}

#[derive(Debug)]
pub struct Parser {
    buffer: BString,
    style: Style,
    state: State,
    pub(super) widget: super::widget::Widget,
    cursor_x: usize,
    need_newline: bool,
    pub ocrnl: bool,
}

fn parse_ansi_col(mut style: Style, string: &BStr) -> Style {
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
                } else {
                    (std::str::from_utf8(part).unwrap().parse::<usize>().unwrap(), colon)
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
                match parts.next() {
                    Some((0, _)) => style.remove_modifier(Modifier::UNDERLINED),
                    Some((1, _)) => style.add_modifier(Modifier::UNDERLINED),
                    Some((2, _)) => style.add_modifier(Modifier::UNDERLINED), // underdouble
                    Some((3, _)) => style.add_modifier(Modifier::UNDERLINED), // undercurl
                    Some((4, _)) => style.add_modifier(Modifier::UNDERLINED), // underdotted
                    Some((5, _)) => style.add_modifier(Modifier::UNDERLINED), // underdashed
                    _ => style.add_modifier(Modifier::UNDERLINED),
                }
            } else {
                style.add_modifier(Modifier::UNDERLINED)
            },
            5 => style.add_modifier(Modifier::SLOW_BLINK),
            7 => style.add_modifier(Modifier::REVERSED),
            8 => style.add_modifier(Modifier::HIDDEN),
            9 => style.add_modifier(Modifier::CROSSED_OUT),
            21 => style.add_modifier(Modifier::UNDERLINED), // underdouble
            22 => style.remove_modifier(Modifier::BOLD).remove_modifier(Modifier::DIM),
            23 => style.remove_modifier(Modifier::ITALIC),
            24 => style.remove_modifier(Modifier::UNDERLINED),
            25 => style.remove_modifier(Modifier::SLOW_BLINK),
            27 => style.remove_modifier(Modifier::REVERSED),
            28 => style.remove_modifier(Modifier::HIDDEN),
            29 => style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style.fg(Color::Indexed(part as u8 - 30)),
            39 => style.fg(Color::Reset),
            40..=47 => style.bg(Color::Indexed(part as u8 - 30)),
            49 => style.bg(Color::Reset),
            59 => style.underline_color(Color::Reset),
            90..=97 => style.fg(Color::Indexed(part as u8 - 90 + 8)),
            100..=107 => style.bg(Color::Indexed(part as u8 - 90 + 8)),
            38 => match parts.next() {
                Some((2, _)) => if let Some(((r, g), b)) = parts.next().zip(parts.next()).zip(parts.next()) {
                    style.fg(Color::Rgb(r.0 as u8, g.0 as u8, b.0 as u8))
                } else {
                    style
                },
                Some((5, _)) => if let Some(part) = parts.next() {
                    style.fg(Color::Indexed(part.0 as u8))
                } else {
                    style
                },
                _ => style,
            },
            48 => match parts.next() {
                Some((2, _)) => if let Some(((r, g), b)) = parts.next().zip(parts.next()).zip(parts.next()) {
                    style.bg(Color::Rgb(r.0 as u8, g.0 as u8, b.0 as u8))
                } else {
                    style
                },
                Some((5, _)) => if let Some(part) = parts.next() {
                    style.bg(Color::Indexed(part.0 as u8))
                } else {
                    style
                },
                _ => style,
            },
            58 => match parts.next() {
                Some((2, _)) => if let Some(((r, g), b)) = parts.next().zip(parts.next()).zip(parts.next()) {
                    style.underline_color(Color::Rgb(r.0 as u8, g.0 as u8, b.0 as u8))
                } else {
                    style
                },
                Some((5, _)) => if let Some(part) = parts.next() {
                    style.underline_color(Color::Indexed(part.0 as u8))
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

    pub fn as_widget(&mut self) -> &mut super::widget::Widget {
        &mut self.widget
    }

    fn add_line(&mut self) {
        self.widget.inner.push_line(b"".into(), None);
        self.cursor_x = 0;
        self.need_newline = false;
    }

    fn add_buffer(&mut self) {
        if !self.buffer.is_empty() {
            self.add_str(format!("{}", self.buffer));
            self.buffer.clear();
        }
    }

    fn cursor_x_to_byte_pos(&self) -> usize {
        let line = self.widget.inner.get().last().unwrap();
        let mut width = 0;
        let mut pos = 0;
        for (s, _, c) in line.grapheme_indices().chain(std::iter::once((line.len(), line.len(), " "))) {
            if width <= self.cursor_x {
                pos = s;
            }
            width += c.width();
        }
        pos
    }

    fn splice(&mut self, range: Option<Range<usize>>, replace_with: Option<String>, style: Option<Style>) {
        let replace_with = replace_with.as_ref().map(|x| x.as_str().into());
        let style = style.map(|s| s.into());

        if let Some(range) = range {
            let lineno = self.widget.inner.len() - 1;
            let len = self.widget.inner.get()[0].len();

            self.widget.inner.delete_str(lineno, range.start, range.end - range.start);
            if let Some(replace_with) = replace_with {
                self.widget.inner.insert_str(replace_with, lineno, len, style);
            }

        } else if let Some(replace_with) = replace_with {
            self.widget.inner.push_str(replace_with, style);
        }
    }

    fn add_str(&mut self, string: String) {
        if string.is_empty() {
            return
        }

        if self.need_newline || self.widget.inner.get().is_empty() {
            self.add_line();
        }

        self.cursor_x += string.width();
        self.splice(None, Some(string), Some(self.style));
    }

    pub fn feed(&mut self, string: &BStr) {
        // we support some csi styling, newlines, tabs and normal text and that's about it

        for c in string.iter() {
            let old_state = self.state;
            self.state = match (old_state, c) {
                (State::None, b'\x1b') => State::Esc,
                (State::Esc, b'[') => State::Csi,

                (State::Csi | State::CsiParams, b'0'..=b'9' | b';' | b':') => {
                    self.buffer.push(*c);
                    State::CsiParams
                },
                (State::Csi | State::CsiParams, b'm') => {
                    self.style = parse_ansi_col(self.style, self.buffer.as_ref());
                    self.buffer.clear();
                    State::None
                },
                (State::Csi | State::CsiParams, b'K') => {
                    let param = self.buffer.split(|c| *c == b';' || *c == b':').next().unwrap_or(b"");
                    let param = if param.is_empty() { b"0" } else { param };
                    let param = std::str::from_utf8(param).unwrap().parse::<usize>().unwrap();

                    if let Some(last_line) = self.widget.inner.get().last() {
                        let cursor_x = self.cursor_x_to_byte_pos();
                        match param {
                            0 => {
                                let range = cursor_x .. last_line.len();
                                self.splice(Some(range), None, Some(Style::new()));
                            },
                            1 => {
                                let range = 0 .. cursor_x;
                                let replace_with = " ".repeat(self.cursor_x);
                                self.splice(Some(range), Some(replace_with), Some(Style::new()));
                            },
                            2 => {
                                let range = 0 .. last_line.len();
                                let replace_with = " ".repeat(self.cursor_x);
                                self.splice(Some(range), Some(replace_with), Some(Style::new()));
                            },
                            _ => (),
                        }
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

                (State::None, b'\n') => {
                    self.add_buffer();
                    self.need_newline = true;
                    State::None
                },
                (State::None, b'\r') => {
                    self.add_buffer();
                    if self.ocrnl {
                        self.need_newline = true;
                    } else {
                        self.cursor_x = 0;
                    }
                    State::None
                },
                (State::None, b'\t') => {
                    self.add_buffer();
                    let len = TAB_SIZE - self.cursor_x % TAB_SIZE;
                    self.add_str(" ".repeat(len));
                    State::None
                },
                (State::None, 0..0x7f) if !(b' '..b'~').contains(c) => {
                    // unprintable ascii
                    State::None
                },
                (State::None, _) => {
                    self.buffer.push(*c);
                    State::None
                },
            };

            if old_state == State::None && self.state != State::None {
                self.add_buffer();
            }
        }

        if self.state == State::None {
            self.add_buffer();
        }
    }

    pub fn clear(&mut self) {
        self.widget.inner.clear();
        self.buffer.clear();
        self.state = State::None;
        self.cursor_x = 0;
        self.need_newline = false;
    }

}

impl std::default::Default for Parser {
    fn default() -> Self {
        Self {
            buffer: Default::default(),
            style: Default::default(),
            state: Default::default(),
            widget: super::widget::Widget::default(),
            cursor_x: 0,
            need_newline: false,
            ocrnl: false,
        }
    }
}

impl From<&[u8]> for Parser {
    fn from(val: &[u8]) -> Self {
        let mut parser = Self::default();
        parser.feed(val.into());
        parser
    }
}
