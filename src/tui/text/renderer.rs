use std::sync::{Mutex};
use std::ops::Range;
use unicode_width::UnicodeWidthStr;
use std::io::Write;
use crate::tui::{Drawer, Canvas};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Rect, Alignment};
use ratatui::widgets::{Block, WidgetRef};

use super::{Text, Scroll, HighlightedRange};
use crate::tui::wrap::WrapToken;
use crate::tui::scroll::{ScrollPosition, ScrolledLinesIter};

const SCROLLBAR_CHAR: &str = "▕";

static BUFFERS: Mutex<Vec<Buffer>> = Mutex::new(vec![]);

#[derive(Debug)]
struct BufferRef {
    inner: Buffer,
}

impl BufferRef {
    fn new() -> Self {
        Self{ inner: BUFFERS.lock().unwrap().pop().unwrap_or_default() }
    }
}

impl Drop for BufferRef {
    fn drop(&mut self) {
        BUFFERS.lock().unwrap().push(std::mem::take(&mut self.inner));
    }
}

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

#[derive(Debug, Default)]
pub struct Borders {
    buffer: Option<BufferRef>,
    inner: Rect,
    top_y: u16,
}

impl Borders {
    fn top(&self) -> Option<&[Cell]> {
        let buffer = self.buffer.as_ref()?;
        let width = buffer.inner.area.width;
        Some(&buffer.inner.content[(width * self.top_y) as usize .. (width * self.inner.y) as usize])
    }
    fn bottom(&self) -> Option<&[Cell]> {
        let buffer = self.buffer.as_ref()?;
        Some(&buffer.inner.content[buffer.inner.area.width as usize * (self.inner.y + self.inner.height) as usize ..])
    }
    fn left(&self) -> Option<&[Cell]> {
        let buffer = self.buffer.as_ref()?;
        Some(&buffer.inner.content[(buffer.inner.area.width * self.inner.y) as usize ..][.. self.inner.x as usize])
    }
    fn right(&self) -> Option<&[Cell]> {
        let buffer = self.buffer.as_ref()?;
        let width = buffer.inner.area.width as usize;
        Some(&buffer.inner.content[width * self.inner.y as usize ..][(self.inner.x + self.inner.width) as usize .. width])
    }
}

pub struct TextRenderer<'a> {
    content_width: usize,
    max_width: usize,
    line_count: usize,
    lines: ScrolledLinesIter<'a>,
    alignment: Alignment,
    newline: Option<Cell>,
    clear_cell: Cell,
    initial_indent: Option<usize>,
    indent_cell: Cell,
    scrollbar_range: Option<(Range<usize>, Cell)>,
    borders: Borders,
}


impl<'a> TextRenderer<'a> {

    pub fn new<T, H>(
        text: &'a Text<T>,
        initial_indent: usize,
        block: Option<&Block<'_>>,
        max_width: usize,
        max_height: Option<(usize, Scroll)>,
        extra_highlights: H,
    ) -> Self
    where
        T: 'a,
        H: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    {

        const SCRATCH_HEIGHT: u16 = 3;
        let mut area = Rect{ x: 0, y: 0, height: SCRATCH_HEIGHT, width: max_width as u16 };

        // setup the borders
        let mut borders = if let Some(block) = block {
            // 3 lines in case you have borders
            let mut buffer = BufferRef::new();
            buffer.inner.resize(area);
            buffer.inner.reset();
            block.render_ref(area, &mut buffer.inner);
            area = block.inner(area);

            Borders{
                buffer: Some(buffer),
                inner: area,
                top_y: 0,
            }
        } else {
            Borders::default()
        };

        let (max_height, scroll) = max_height.unzip();
        let scroll = scroll.unwrap_or(Scroll{ show_scrollbar: false, position: ScrollPosition::Line(0) });
        // let max_height = max_height.min(full_height - drawer.get_pos().1 as usize);

        let mut indent_cell = Cell::EMPTY;
        indent_cell.set_style(text.style);

        let scrolled = crate::tui::scroll::wrap(
            &text.lines,
            text.highlights.iter().chain(extra_highlights),
            Some(text.style),
            area.width as usize,
            max_height.map(|h| h - (SCRATCH_HEIGHT - area.height) as usize),
            initial_indent,
            scroll.position,
        );

        // check if no space for the border
        if borders.bottom().is_some() && let Some(h) = max_height {
            borders.inner.height = borders.inner.height.min((h - scrolled.range.len()) as u16 - borders.inner.y);
        }

        let scrollbar_range = if scroll.show_scrollbar && !(scrolled.range == (0 .. scrolled.range.len()))  {
            area.width -= 1;

            let mut start = scrolled.range.start * scrolled.range.len() / scrolled.total_line_count.max(1);
            if scrolled.range.start > 0 && start == 0 {
                start = 1;
            }
            let mut end = scrolled.range.end * scrolled.range.len() / scrolled.total_line_count.max(1);
            if scrolled.range.end < scrolled.total_line_count && end == scrolled.range.len() {
                end = end.saturating_sub(1);
            }

            let mut cell = Cell::new(SCROLLBAR_CHAR);
            cell.set_style(text.style);
            Some((start .. end, cell))
        } else {
            None
        };

        Self {
            content_width: area.width as _,
            max_width,
            line_count: 0,
            lines: scrolled.into_lines(),
            alignment: text.alignment,
            newline: None,
            clear_cell: text.make_default_style_cell().unwrap_or_default(),
            initial_indent: Some(initial_indent),
            indent_cell,
            scrollbar_range,
            borders,
        }
    }

    fn get_alignment_indent(&self, max_width: usize, line_width: usize) -> usize {
        match self.alignment {
            Alignment::Left => 0,
            Alignment::Right => max_width.saturating_sub(line_width),
            Alignment::Center => max_width.saturating_sub(line_width) / 2,
        }
    }

    fn left_border_width(&self) -> usize {
        self.borders.left().map_or(0, |b| b.len())
    }

    fn right_border_width(&self) -> usize {
        self.borders.right().map_or(0, |b| b.len())
    }

    fn get_left_border(&self) -> Option<&[Cell]> {
        self.borders.left().filter(|b| !b.is_empty())
    }

    fn get_right_border(&self) -> Option<&[Cell]> {
        self.borders.right().filter(|b| !b.is_empty())
    }

    fn get_top_border(&self) -> Option<&[Cell]> {
        self.borders.top().filter(|b| !b.is_empty())
    }

    fn get_bottom_border(&self) -> Option<&[Cell]> {
        self.borders.bottom().filter(|b| !b.is_empty())
    }
}

impl Renderer for TextRenderer<'_> {

    fn finished(&mut self) -> bool {
        self.get_top_border().is_none() && self.get_bottom_border().is_none() && !self.lines.has_more()
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

        // draw top border
        if let Some(top) = self.get_top_border() {
            if newline {
                drawer.goto_newline(self.newline.as_ref())?;
            }
            drawer.draw_cells(&top[..self.max_width], false)?;
            self.borders.top_y += 1;
            // no need for padding, border should span the whole width
            return Ok(true)
        }

        // draw a line
        if let Some(slice) = self.lines.next() {
            let line = self.lines.slice(slice);
            let lineno = self.line_count;
            self.line_count += 1;

            if newline {
                drawer.goto_newline(self.newline.as_ref())?;
            }
            let initial_x = drawer.get_pos().0.saturating_sub(self.initial_indent.take().unwrap_or(0) as _);

            // draw left border
            if let Some(left) = self.get_left_border() {
                drawer.draw_cells(left, false)?;
            }

            let line_width = line.iter()
                .map(|token| if let WrapToken::String(str) = &token.inner {
                    str.width()
                } else {
                    0
                }).sum();

            // draw the indent
            let indent = self.get_alignment_indent(self.content_width as _, line_width);
            drawer.draw_cell_n_times(&self.indent_cell, false, indent as _)?;

            // draw the line
            let mut cell = Cell::EMPTY;
            for token in line {
                if let WrapToken::String(symbol) = &token.inner {
                    cell.reset();
                    cell.set_symbol(symbol);
                    cell.set_style(token.style.unwrap());
                    drawer.draw_cell(&cell, false)?;
                }
                if let Some(callback) = callback {
                    callback(drawer, token.lineno, token.start, token.end);
                }
            }

            // draw the scrollbar
            if let Some((scrollbar_range, bar)) = &self.scrollbar_range && scrollbar_range.contains(&lineno) {
                let x = (initial_x + self.left_border_width() as u16 + self.content_width as u16).saturating_sub(drawer.get_pos().0);
                drawer.draw_cell_n_times(&self.clear_cell, false, x)?;
                drawer.draw_cell(bar, false)?;
            }

            // draw right border
            if let Some(right) = self.get_right_border() {
                let x = (initial_x + self.max_width as u16).saturating_sub(self.right_border_width() as u16 + drawer.get_pos().0);
                drawer.draw_cell_n_times(&self.clear_cell, false, x)?;
                drawer.draw_cells(right, false)?;
                // no more padding, border should span the whole width

            } else if pad {
                let x = (initial_x + self.max_width as u16).saturating_sub(drawer.get_pos().0);
                drawer.draw_cell_n_times(&self.clear_cell, false, x)?;
            }

            return Ok(true)
        }

        // draw bottom border
        if let Some(bottom) = self.get_bottom_border() {
            if newline {
                drawer.goto_newline(self.newline.as_ref())?;
            }
            drawer.draw_cells(&bottom[..self.max_width], false)?;
            self.borders.inner.height += 1;
            // no need for padding, border should span the whole width
            return Ok(true)
        }

        Ok(false)
    }
}
