use bstr::BStr;
use std::default::Default;
use std::rc::Rc;
use crate::tui::{Style, Modifier, Hyperlink};
use crate::tui::border::{Border};
use crossterm::style::Color;
mod ansi;
pub use ansi::parse_ansi_col;
use super::scroll::ScrollPosition;
use super::text::Scroll;

#[derive(Default, Debug, Clone, Copy)]
pub enum UnderlineOption {
    #[default]
    None,
    Set,
    Color(Color),
}

#[derive(Debug, Default, Clone)]
pub struct StyleOptions {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: Option<bool>,
    pub dim: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<UnderlineOption>,
    pub strikethrough: Option<bool>,
    pub reversed: Option<bool>,
    pub blink: Option<bool>,
    pub hyperlink: Option<Option<Rc<Hyperlink>>>,
}

impl StyleOptions {
    pub fn as_style(&self) -> Style {
        let mut modifier = Modifier::empty();
        let mut modifier_mask = Modifier::empty();
        let mut underline_color = None;

        match self.underline {
            None => (),
            Some(UnderlineOption::None) => {
                modifier_mask |= Modifier::UNDERLINED;
            },
            Some(UnderlineOption::Set) => {
                modifier      |= Modifier::UNDERLINED;
                modifier_mask |= Modifier::UNDERLINED;
            },
            Some(UnderlineOption::Color(color)) => {
                underline_color = Some(color);
                modifier      |= Modifier::UNDERLINED;
                modifier_mask |= Modifier::UNDERLINED;
            },
        }

        macro_rules! set_modifier {
            ($field:ident, $flag:ident) => {
                if let Some(v) = self.$field {
                    modifier_mask |= Modifier::$flag;
                    if v { modifier |= Modifier::$flag; }
                }
            }
        }

        set_modifier!(bold,          BOLD);
        set_modifier!(dim,           DIM);
        set_modifier!(italic,        ITALIC);
        set_modifier!(strikethrough, CROSSED_OUT);
        set_modifier!(reversed,      REVERSED);
        set_modifier!(blink,         SLOW_BLINK);

        Style {
            fg: self.fg,
            bg: self.bg,
            underline_color,
            hyperlink: self.hyperlink.clone().flatten(),
            modifier,
            modifier_mask,
        }
    }

    pub fn merge(&self, other: &Self) -> Self {
        Self{
            fg: other.fg.or(self.fg),
            bg: other.bg.or(self.bg),
            bold: other.bold.or(self.bold),
            dim: other.dim.or(self.dim),
            italic: other.italic.or(self.italic),
            underline: other.underline.or(self.underline),
            strikethrough: other.strikethrough.or(self.strikethrough),
            reversed: other.reversed.or(self.reversed),
            blink: other.blink.or(self.blink),
            hyperlink: other.hyperlink.clone().or_else(|| self.hyperlink.clone()),
        }
    }

}

#[derive(Default, Debug, Clone)]
pub struct Widget {
    pub inner: super::text::Text,
    pub style: StyleOptions,
    pub border_show_empty: bool,
    pub border: Border,
    // line_count is used by StatusBar for standalone rendering
    pub(super) line_count: u16,

    pub(super) ansi: ansi::Parser,
    pub scroll: Scroll,
    pub ansi_show_cursor: bool,
    pub cursor_space_hl: Option<super::text::HighlightedRange<()>>,
    pub draw_pos: std::cell::Cell<Option<(u16, u16)>>,
}

impl Widget {

    pub(in crate::tui) fn make_cursor_space_hl(&mut self) {
        if self.ansi_show_cursor {

            if self.ansi.need_newline {
                self.ansi.add_line(&mut self.inner);
            }

            let pos = ansi::Parser::to_byte_pos(&self.inner, self.ansi.cursor_x);
            let (lineno, need_space) = if let Some(line) = self.inner.get().last() {
                (self.inner.len().saturating_sub(1), pos == line.len())
            } else {
                (0, true)
            };

            self.cursor_space_hl = Some(super::text::HighlightedRange{
                lineno,
                start: pos,
                end: pos + 1,
                inner: super::text::Highlight {
                    style: Style::new().add_modifier(Modifier::REVERSED),
                    blend: true,
                    namespace: (),
                    virtual_text: need_space.then(|| b" ".into()),
                    conceal: None,
                    priority: 0.,
                },
            });
        } else {
            self.cursor_space_hl = None;
        }
    }

    pub fn get_height_for_width(
        &self,
        mut max_width: u16,
        show_scrollbar: Option<bool>,
    ) -> u16 {

        // border
        let (top, bottom) = self.border.inner_height();
        let (left, right) = self.border.inner_width(max_width);
        let border_height = top + bottom;
        max_width = max_width.saturating_sub(left + right);

        if show_scrollbar.unwrap_or(self.scroll.show_scrollbar) {
            max_width = max_width.saturating_sub(1);
        }

        let mut height = self.inner.get_size(max_width as _, 0, self.cursor_space_hl.iter()).1 as u16;
        if self.border_show_empty || height > 0 {
            height += border_height;
        }

        height
    }

    pub fn feed_ansi(&mut self, string: &BStr) {
        self.ansi.feed(&mut self.inner, string);
    }

    pub fn clear(&mut self) {
        self.inner.clear();
        self.ansi.clear();
    }

    pub fn scroll(&mut self, value: isize, relative: bool) -> bool {
        let max_line = self.inner.len().saturating_sub(1);
        let value = if relative {
            let current_line = match self.scroll.position {
                ScrollPosition::Line(line) => line,
                ScrollPosition::StickyBottom => max_line,
            };
            current_line as isize + value
        } else {
            value
        };
        let value = value.max(0) as usize;

        let new_scroll = if value >= max_line {
            ScrollPosition::StickyBottom
        } else {
            ScrollPosition::Line(value)
        };

        if self.scroll.position == new_scroll {
            false
        } else {
            self.scroll.position = new_scroll;
            true
        }
    }

}
