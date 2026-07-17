use bstr::BStr;
use std::default::Default;
use crate::tui::{Style, Modifier};
use crate::tui::border::{Border};
mod ansi;
pub use ansi::parse_ansi_col;
use super::scroll::ScrollPosition;
use super::text::Scroll;

#[derive(Default, Debug, Clone)]
pub struct Widget {
    pub inner: super::text::Text,
    pub style: Style,
    pub ephemeral: super::text::HighlightedRangeSet<()>,
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
            let (parano, need_space) = if let Some(para) = self.inner.get().last() {
                (self.inner.len().saturating_sub(1), pos == para.len())
            } else {
                (0, true)
            };

            self.cursor_space_hl = Some(super::text::HighlightedRange{
                parano,
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
        let border_height = self.border.inner_height();
        let border_width = self.border.inner_width(max_width);
        max_width = max_width.saturating_sub(border_width);

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
            let current = self.scroll.position.get_approx_range(None, self.inner.get()).parano;
            current as isize + value
        } else {
            value
        };
        let value = value.max(0) as usize;

        let new_scroll = if value >= max_line {
            ScrollPosition::StickyBottom
        } else {
            ScrollPosition::Paragraph(value)
        };

        if self.scroll.position == new_scroll {
            false
        } else {
            self.scroll.position = new_scroll;
            true
        }
    }

}
