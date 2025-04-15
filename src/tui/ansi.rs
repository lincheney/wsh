use bstr::{BStr, BString, ByteSlice};
use ratatui::{
    text::*,
    widgets::*,
    style::*,
};

const TAB_SIZE: usize = 8;

#[derive(Debug, Default)]
pub struct Parser {
    text: Text<'static>,
    buffer: BString,
    style: Style,
    pub(super) widget: super::Widget,
    cursor_x: usize,
    need_newline: bool,
}

fn parse_ansi_col(mut style: Style, string: &BStr) -> Style {
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
                (std::str::from_utf8(part).unwrap().parse::<usize>().unwrap(), colon)
            }
        });

    while let Some((part, colon)) = parts.next() {
        style = match part {
            0 =>  Style::default(),
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
        let mut text = self.text.clone();
        if !self.widget.style.is_empty() {
            let style = self.widget.style.as_style();
            for line in text.lines.iter_mut() {
                for span in line.spans.iter_mut() {
                    *span = std::mem::replace(span, Span::default()).patch_style(style);
                }
            }
        }

        if self.widget.inner.is_none() {
            self.widget.inner = Some(Paragraph::new(text));
            self.widget.make_paragraph();
        }
        &mut self.widget
    }

    fn add_line(&mut self) {
        self.text.lines.push(Line::default());
        self.cursor_x = 0;
        self.need_newline = false;
    }

    fn add_str(&mut self, string: String) {
        if string.is_empty() {
            return
        }

        if self.need_newline || self.text.lines.is_empty() {
            self.add_line();
        }
        let line = self.text.lines.last_mut().unwrap();
        let span = Span::styled(string, self.style);
        let width = span.width();
        if self.cursor_x >= line.width() {
            line.spans.push(span);
        } else {
            // need to overwrite things ....
            let mut start = 0;
            line.spans.retain_mut(|sp| {
                let w = sp.width();
                let start_overlap = (self.cursor_x + width).saturating_sub(start);
                let end_overlap = (start + w).saturating_sub(self.cursor_x);
                start += w;

                if start_overlap > 0 && end_overlap > 0 {
                    // span is fully within the overwrite range
                    false
                } else if start_overlap > 0 {
                    // range overlaps start of this span
                    let content = sp.styled_graphemes(Style::default())
                        .skip(start_overlap)
                        .map(|s| s.symbol)
                        .collect::<Vec<_>>()
                        .join("");
                    *sp = std::mem::replace(sp, Span::default()).content(content);
                    true
                } else if end_overlap > 0 {
                    // range overlaps end of this span
                    let content = sp.styled_graphemes(Style::default())
                        .take(w - end_overlap)
                        .map(|s| s.symbol)
                        .collect::<Vec<_>>()
                        .join("");
                    *sp = std::mem::replace(sp, Span::default()).content(content);
                    true
                } else {
                    // no overlap
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
        self.flush();
    }

    pub fn flush(&mut self) {
        self.widget.inner = None;
    }

    pub fn feed(&mut self, mut string: &BStr) {
        while let Some(chunk) = string.split_inclusive(|c| matches!(c,
            b'\n' | b'\r' | b'\t' | b'\x1b',
        )).next() {

            let (last, chunk) = chunk.split_last().unwrap();

            let mut handler = || {
                if !chunk.is_empty() {
                    self.add_str(format!("{}", chunk.as_bstr()));
                }
                string = &string[chunk.len() + 1 .. ];
            };

            match last {
                b'\n' => {
                    handler();
                    self.need_newline = true;
                },
                b'\r' => {
                    handler();
                    self.cursor_x = 0;
                },
                b'\t' => {
                    handler();
                    let len = TAB_SIZE - self.cursor_x % TAB_SIZE;
                    self.add_str(" ".repeat(len));
                },
                b'\x1b' => {
                    handler();

                    if string.starts_with(b"[") {
                        // csi
                        string = &string[1..];

                        let col = match string.iter().position(|c| !matches!(c, b'0'..=b'9' | b':' | b';')) {
                            Some(col) => col,
                            None => {
                                // csi sequence not finished
                                self.buffer.extend_from_slice(string);
                                return
                            },
                        };

                        self.buffer.extend_from_slice(&string[..col]);
                        string = &string[col .. ];
                        if string.starts_with(b"m") {
                            self.style = parse_ansi_col(self.style, self.buffer.as_ref());
                            self.buffer.clear();
                        }
                        string = &string[1..];
                    }
                },
                _ => {
                    self.add_str(format!("{}", string));
                    return
                },
            }
        }

    }

}

