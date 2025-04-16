use bstr::{BStr, BString, ByteSlice};
use ratatui::{
    text::*,
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

#[derive(Debug, Default)]
pub struct Parser {
    buffer: BString,
    style: Style,
    state: State,
    pub(super) widget: super::Widget,
    cursor_x: usize,
    need_newline: bool,
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
                    (&part[..part.len()-1], false)
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

    pub fn as_widget(&mut self) -> &mut super::Widget {
        &mut self.widget
    }

    fn add_line(&mut self) {
        self.widget.inner.lines.push(Line::default());
        self.cursor_x = 0;
        self.need_newline = false;
    }

    fn add_buffer(&mut self) {
        if !self.buffer.is_empty() {
            self.add_str(format!("{}", self.buffer));
            self.buffer.clear();
        }
    }

    fn add_str(&mut self, string: String) {
        if string.is_empty() {
            return
        }

        if self.need_newline || self.widget.inner.lines.is_empty() {
            self.add_line();
        }

        let line = self.widget.inner.lines.last_mut().unwrap();
        let span = Span::styled(string, self.style);
        let width = span.width();
        if self.cursor_x >= line.width() {
            line.spans.push(span);
        } else {
            // need to overwrite things ....
            let mut start = 0;
            line.spans.retain_mut(|sp| {
                let w = sp.width();
                let overlap_start = start.max(self.cursor_x);
                let overlap_end = (start + w).min(self.cursor_x + width);
                start += w;

                if overlap_start == start && overlap_end == start + w {
                    // span is fully within the overwrite range
                    false
                } else if overlap_start >= overlap_end {
                    // no overlap
                    true
                } else {
                    let content = sp.styled_graphemes(Style::default())
                        .take(overlap_end - (start - w))
                        .skip(overlap_start.saturating_sub(start - w))
                        .map(|s| s.symbol)
                        .collect::<Vec<_>>()
                        .join("");
                    *sp = std::mem::replace(sp, Span::default()).content(content);
                    true
                }
            });

            if line.spans.is_empty() {
                line.spans.push(span);
            } else {
                let mut start = 0;
                for (i, sp) in line.spans.iter().enumerate() {
                    start += sp.width();
                    if start >= self.cursor_x {
                        line.spans.insert(i, span);
                        break
                    }
                }
            }

        }
        self.cursor_x += width;
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
                    self.cursor_x = 0;
                    State::None
                },
                (State::None, b'\t') => {
                    self.add_buffer();
                    let len = TAB_SIZE - self.cursor_x % TAB_SIZE;
                    self.add_str(" ".repeat(len));
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
        self.widget.inner.lines.clear();
        self.buffer.clear();
        self.state = State::None;
        self.cursor_x = 0;
        self.need_newline = false;
    }

}

