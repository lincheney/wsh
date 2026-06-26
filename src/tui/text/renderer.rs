use std::ops::Range;
use unicode_width::UnicodeWidthStr;
use std::io::Write;
use crate::tui::{Drawer, Canvas, Cell, text::Alignment, border::Border};

use super::{Text, Scroll, HighlightedRange};
use crate::tui::wrap::WrapToken;
use crate::tui::scroll::{ScrollPosition, ScrolledLinesIter};

const SCROLLBAR_CHAR: &str = "▕";

pub trait Renderer {
    fn finished(&mut self) -> bool;

    fn draw_one_line<W, C, F>(
        &mut self,
        drawer: &mut Drawer<W, C>,
        newlines: bool,
        pad: bool,
        callback: &mut Option<F>,
    ) -> std::io::Result<bool>
    where
        W :Write,
        C: Canvas,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize)
        ;

    fn render<W, C, F>(
        &mut self,
        drawer: &mut Drawer<W, C>,
        mut newline: bool,
        pad: bool,
        mut callback: Option<F>,
    ) -> std::io::Result<()>
    where
        W :Write,
        C: Canvas,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize),
    {
        while self.draw_one_line(drawer, newline, pad, &mut callback)? {
            newline = true;
        }
        Ok(())
    }
}

pub struct TextRenderer<'a> {
    content_width: usize,
    width: usize,
    remaining_height: Option<usize>,
    line_count: usize,
    lines: ScrolledLinesIter<'a>,
    alignment: Alignment,
    newline: Option<Cell>,
    clear_cell: Cell,
    initial_indent: Option<usize>,
    indent_cell: Cell,
    scrollbar_range: Option<(Range<usize>, Cell)>,
    border: Option<&'a Border>,
    top_consumed:    bool,
    bottom_consumed: bool,
}


impl<'a> TextRenderer<'a> {

    pub fn new<T, H>(
        text: &'a Text<T>,
        initial_indent: usize,
        border: Option<&'a Border>,
        width: usize,
        height: Option<usize>,
        scroll: Option<Scroll>,
        extra_highlights: H,
    ) -> Self
    where
        T: 'a,
        H: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    {

        // how many rows do top/bottom borders consume
        let (top_rows, bottom_rows) = border.map(|b| b.inner_height()).unwrap_or_default();
        // how many cols do left/right borders consume
        let (left_cols, right_cols) = border.map(|b| b.inner_width(width as u16)).unwrap_or_default();
        let border_h = (top_rows + bottom_rows) as usize;
        let border_w = (left_cols + right_cols) as usize;

        let content_width = width.saturating_sub(border_w);

        let scroll = scroll.unwrap_or(Scroll{ show_scrollbar: false, position: ScrollPosition::Line(0) });

        let mut indent_cell = Cell::EMPTY;
        indent_cell.style = text.style.clone();

        let text_height = height.map(|h| h.saturating_sub(border_h));
        let scrolled = crate::tui::scroll::wrap(
            &text.lines,
            text.highlights.iter().chain(extra_highlights),
            Some(text.style.clone()),
            content_width - if scroll.show_scrollbar { 1 } else { 0 },
            text_height,
            initial_indent,
            scroll.position,
        );

        let scrollbar_range = if scroll.show_scrollbar && !(scrolled.range.start == 0 && scrolled.range.end >= scrolled.total_line_count.max(1)) {
            let num_lines = scrolled.total_line_count.max(1);
            let text_height = text_height.unwrap_or(num_lines);
            let height = (text_height as f64 * scrolled.range.len() as f64 / num_lines as f64).round().max(1.) as usize;
            let start = text_height as f64 * scrolled.range.start as f64 / num_lines as f64;
            let start = (start.round().max(0.) as usize).min(text_height.saturating_sub(height));
            let end = start + height.max(1);

            let mut cell = Cell::new(SCROLLBAR_CHAR);
            cell.style = text.style.clone();
            Some((start .. end, cell))
        } else {
            None
        };

        Self {
            content_width,
            width,
            remaining_height: height,
            line_count: 0,
            lines: scrolled.into_lines(),
            alignment: text.alignment,
            newline: None,
            clear_cell: text.make_default_style_cell().unwrap_or_default(),
            initial_indent: Some(initial_indent),
            indent_cell,
            scrollbar_range,
            border,
            top_consumed: false,
            bottom_consumed: false,
        }
    }

    fn get_alignment_indent(&self, max_width: usize, line_width: usize) -> usize {
        match self.alignment {
            Alignment::Left => 0,
            Alignment::Right => max_width.saturating_sub(line_width),
            Alignment::Center => max_width.saturating_sub(line_width) / 2,
        }
    }

    fn top_pending(&self) -> Option<&Border> {
        self.border.filter(|b| !self.top_consumed && b.has_top())
    }
    fn bottom_pending(&self) -> Option<&Border> {
        self.border.filter(|b| !self.bottom_consumed && b.has_bottom())
    }

    fn left_width(&self)  -> usize {
        self.border.is_some_and(|b| b.has_left()).into()
    }
    fn right_width(&self) -> usize {
        self.border.is_some_and(|b| b.has_right()).into()
    }
}

impl Renderer for TextRenderer<'_> {

    fn finished(&mut self) -> bool {
        self.top_pending().is_none()
            && self.bottom_pending().is_none()
            && !self.lines.has_more()
    }

    fn draw_one_line<W, C, F>(
        &mut self,
        drawer: &mut Drawer<W, C>,
        newline: bool,
        pad: bool,
        callback: &mut Option<F>,
    ) -> std::io::Result<bool>
    where
        W :Write,
        C: Canvas,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize),
    {

        let remaining_height = match &mut self.remaining_height {
            Some(0) => return Ok(false), // no height left, done
            Some(height) => {
                let old = *height;
                *height -= 1;
                old
            },
            None => 0, // unlimited height
        };

        // draw top border row
        if let Some(border) = self.top_pending() {
            if newline {
                drawer.goto_newline(self.newline.as_ref())?;
            }
            border.render_top(drawer, self.width as u16)?;
            self.top_consumed = true;
            return Ok(true)
        }

        let line = if let Some(slice) = self.lines.next() {
            Some(self.lines.slice(slice))
        } else if remaining_height >= 2 || (remaining_height >= 1 && self.bottom_pending().is_none()) {
            Some(&[] as _)
        } else {
            None
        };

        // draw a content line
        if let Some(line) = line {
            let lineno = self.line_count;
            self.line_count += 1;

            if newline {
                drawer.goto_newline(self.newline.as_ref())?;
            }
            let initial_x = drawer.get_pos().0.saturating_sub(self.initial_indent.take().unwrap_or(0) as _);

            // draw left border
            if let Some(border) = self.border {
                border.render_left(drawer)?;
            }

            let line_width = line.iter()
                .map(|token| if let WrapToken::String(str) = &token.inner {
                    str.width()
                } else {
                    0
                }).sum();

            // draw the indent
            let indent = self.get_alignment_indent(self.content_width, line_width);
            drawer.draw_cell_n_times(&self.indent_cell, false, indent as u16)?;

            // draw the line
            let mut cell = Cell::EMPTY;
            for token in line {
                if let WrapToken::String(symbol) = &token.inner {
                    cell.reset();
                    cell.set_text(symbol);
                    if let Some(style) = &token.style {
                        cell.style = style.clone();
                    }
                    drawer.draw_cell(&cell, false)?;
                }
                if let Some(callback) = callback {
                    callback(drawer, token.lineno, token.start, token.end);
                }
            }

            // draw the scrollbar
            if let Some((scrollbar_range, bar)) = &self.scrollbar_range && scrollbar_range.contains(&lineno) {
                // +1 for scrollbar
                let x = (initial_x + self.left_width() as u16 + self.content_width as u16).saturating_sub(drawer.get_pos().0 + 1);
                drawer.draw_cell_n_times(&self.clear_cell, false, x as _)?;
                drawer.draw_cell(bar, false)?;
            }

            // draw right border
            if let Some(border) = self.border && border.has_right() {
                let x = (initial_x + self.width as u16).saturating_sub(self.right_width() as u16 + drawer.get_pos().0);
                drawer.draw_cell_n_times(&self.clear_cell, false, x as _)?;
                border.render_right(drawer)?;
            } else if pad {
                let x = (initial_x + self.width as u16).saturating_sub(drawer.get_pos().0);
                drawer.draw_cell_n_times(&self.clear_cell, false, x as _)?;
            }

            return Ok(true)
        }

        // draw bottom border row
        if let Some(border) = self.bottom_pending() {
            if newline {
                drawer.goto_newline(self.newline.as_ref())?;
            }
            border.render_bottom(drawer, self.width as u16)?;
            self.bottom_consumed = true;
            return Ok(true)
        }

        Ok(false)
    }
}
