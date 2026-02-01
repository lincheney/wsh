use bstr::BStr;
use std::default::Default;
use std::io::{Write};
use ratatui::{
    layout::*,
    widgets::*,
    style::*,
    buffer::Buffer,
};
mod ansi;

#[derive(Default, Debug, Clone, Copy)]
pub enum UnderlineOption {
    #[default]
    None,
    Set,
    Color(Color),
}

#[derive(Debug, Default)]
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
}

impl StyleOptions {
    pub fn as_style(&self) -> Style {
        let mut add_modifier = Modifier::empty();
        let mut sub_modifier = Modifier::empty();
        let mut underline_color = None;
        match self.underline {
            None => (),
            Some(UnderlineOption::None) => { sub_modifier |= Modifier::UNDERLINED; },
            Some(UnderlineOption::Set) => { add_modifier |= Modifier::UNDERLINED; },
            Some(UnderlineOption::Color(color)) => {
                underline_color = Some(color);
                add_modifier |= Modifier::UNDERLINED;
            },
        }

        let mut style = Style {
            fg: self.fg,
            bg: self.bg,
            underline_color,
            add_modifier,
            sub_modifier,
        };

        macro_rules! set_modifier {
            ($field:ident, $enum:ident) => (
                if let Some($field) = self.$field {
                    let value = Modifier::$enum;
                    if $field {
                        style.add_modifier.insert(value);
                    } else {
                        style.sub_modifier.insert(value);
                    }
                }
            )
        }

        set_modifier!(bold, BOLD);
        set_modifier!(dim, DIM);
        set_modifier!(italic, ITALIC);
        set_modifier!(strikethrough, CROSSED_OUT);
        set_modifier!(reversed, REVERSED);
        set_modifier!(blink, SLOW_BLINK);

        style
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
        }
    }

}

#[derive(Default, Debug)]
pub struct Widget {
    pub(super) id: usize,
    pub constraint: Option<Constraint>,
    pub inner: super::text::Text,
    pub style: StyleOptions,
    pub border_sides: Option<Borders>,
    pub border_style: Style,
    pub border_type: BorderType,
    pub border_show_empty: bool,
    pub border_title: Option<super::text::Text>,
    pub block: Option<Block<'static>>,
    pub persist: bool,
    pub hidden: bool,
    pub(super) line_count: u16,

    pub(super) ansi: ansi::Parser,
    pub ansi_show_cursor: bool,
}

impl Widget {

    fn ensure_cursor_space(&mut self) -> (usize, bool) {
        let pos = ansi::Parser::to_byte_pos(&self.inner, self.ansi.cursor_x);
        let line = self.inner.get().last().unwrap();
        let need_space = pos == line.len();
        if need_space {
            // add an extra space
            self.inner.push_str(b" ".into(), None);
        }
        (pos, need_space)
    }

    fn remove_cursor_space(&mut self) {
        let lineno = self.inner.len() - 1;
        let line = self.inner.get().last().unwrap();
        let offset = line.len() - 1;
        self.inner.delete_str(lineno, offset, 1);
    }

    pub(super) fn render<W: Write, C: super::Canvas>(
        &mut self,
        drawer: &mut super::Drawer<W, C>,
        buffer: &mut Buffer,
        max_height: Option<usize>,
    ) -> std::io::Result<()> {

        // this is such a hack
        let mut need_cursor_space = false;
        let cursor_highlight = if self.ansi_show_cursor {
            let start;
            (start, need_cursor_space) = self.ensure_cursor_space();
            Some(super::text::HighlightedRange{
                lineno: self.inner.len().saturating_sub(1),
                start,
                end: start + 1,
                inner: ratatui::style::Style::from(ratatui::style::Modifier::REVERSED).into(),
            })
        } else {
            None
        };

        let result = self.inner.render(
            drawer,
            self.block.as_ref().map(|block| (block, buffer)),
            max_height.map(|w| {
                (
                    w,
                    super::text::Scroll{
                        show_scrollbar: true,
                        position: super::scroll::ScrollPosition::StickyBottom,
                    },
                )
            }),
            cursor_highlight.iter(),
        );

        if need_cursor_space {
            self.remove_cursor_space();
        }

        result
    }

    pub(super) fn get_height_for_width(&mut self, mut area: Rect) -> u16 {
        let mut height = 0;
        let mut min_height = None;
        let mut max_height = None;
        match self.constraint {
            Some(Constraint::Min(min)) => min_height = Some(min),
            Some(Constraint::Max(max)) => max_height = Some(max),
            _ => (),
        }

        let mut border_height = 0;
        if let Some(ref block) = self.block {
            let inner = block.inner(area);
            border_height = area.height - inner.height;
            area = inner;
        }

        let need_cursor_space = self.ansi_show_cursor && self.ensure_cursor_space().1;

        height = height.max(self.inner.get_size(area.width as _, 0).1 as _);

        if need_cursor_space {
            self.remove_cursor_space();
        }

        if self.border_show_empty || height > 0 {
            height += border_height;
        }
        if let Some(min_height) = min_height {
            height = height.max(min_height);
        }
        if let Some(max_height) = max_height {
            height = height.min(max_height);
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

}

