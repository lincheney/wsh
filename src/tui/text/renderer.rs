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
    fn pad_to<W, C>(
        drawer: &mut Drawer<W, C>,
        x: u16,
        cell: &Cell,
    ) -> std::io::Result<()>
    where
        W :Write,
        C: Canvas
    {
        for _ in 0 .. x.saturating_sub(drawer.get_pos().0) {
            drawer.draw_cell(cell, false)?;
        }
        Ok(())
    }

    fn draw_one_line<W, C, F>(
        &mut self,
        drawer: &mut Drawer<W, C>,
        newlines: bool,
        pad_to: (u16, &Cell),
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
        newlines: bool,
        pad_to: (u16, &Cell),
        mut callback: Option<F>,
    ) -> std::io::Result<()>
    where
        W :Write,
        C: Canvas,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize),
    {
        while self.draw_one_line(drawer, newlines, pad_to, &mut callback)? { }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Borders {
    buffer: BufferRef,
    inner: Rect,
    top_y: u16,
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
}

impl Borders {
    fn top(&self) -> &[Cell] {
        let width = self.buffer.inner.area.width;
        &self.buffer.inner.content[(width * self.top_y) as usize .. (width * self.inner.y) as usize]
    }
    fn bottom(&self) -> &[Cell] {
        &self.buffer.inner.content[self.buffer.inner.area.width as usize * (self.inner.y + self.inner.height) as usize ..]
    }
    fn left(&self) -> &[Cell] {
        &self.buffer.inner.content[(self.buffer.inner.area.width * self.inner.y) as usize ..][.. self.inner.x as usize]
    }
    fn right(&self) -> &[Cell] {
        let width = self.buffer.inner.area.width as usize;
        &self.buffer.inner.content[width * self.inner.y as usize ..][(self.inner.x + self.inner.width) as usize .. width]
    }
}

pub struct TextRenderer<'a> {
    area: Rect,
    max_width: usize,
    line_count: usize,
    lines: ScrolledLinesIter<'a>,
    alignment: Alignment,
    need_newline: Option<Option<Cell>>,
    clear_cell: Option<Cell>,
    indent_cell: Cell,
    scrollbar_range: Option<(Range<usize>, Cell)>,
    borders: Option<Borders>,
}


impl<'a> TextRenderer<'a> {

    pub fn new<T, W, C, H>(
        text: &'a Text<T>,
        drawer: &mut Drawer<W, C>,
        block: Option<&Block<'_>>,
        max_width: Option<usize>,
        max_height: Option<(usize, Scroll)>,
        extra_highlights: H,
    ) -> Self
    where
        T: 'a,
        C: Canvas,
        H: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    {

        let full_width = max_width.unwrap_or(usize::MAX).min(drawer.term_width() as _);
        let full_height = drawer.term_height() as usize;
        let mut area = Rect{ x: 0, y: 0, height: full_height as u16, width: full_width as u16 };

        // setup the borders
        let mut borders = if let Some(block) = block {
            // 3 lines in case you have borders
            let mut buffer = BufferRef::new();
            let bufarea = Rect{ height: 3, ..area };
            buffer.inner.resize(bufarea);
            buffer.inner.reset();
            block.render_ref(bufarea, &mut buffer.inner);
            let inner_area = block.inner(bufarea);
            // since the border buffer height is different, the inner height will be wrong
            area = Rect{ height: full_height as u16 - (bufarea.height - inner_area.height), ..inner_area };

            Some(Borders{
                buffer,
                inner: inner_area,
                top_y: 0,
                top: true,
                bottom: true,
                left: true,
                right: true,
            })
        } else {
            None
        };

        let (max_height, scroll) = if let Some((max_height, scroll)) = max_height {
            (max_height, scroll)
        } else {
            (usize::MAX, Scroll{ show_scrollbar: false, position: ScrollPosition::Line(0) })
        };
        let max_height = max_height.min(full_height - drawer.get_pos().1 as usize);

        let clear_cell = text.make_default_style_cell();
        let mut indent_cell = Cell::EMPTY;
        indent_cell.set_style(text.style);

        let initial = drawer.get_pos().0 as usize % full_width;
        let max_lines = max_height.saturating_sub(full_height - area.height as usize);
        let scrolled = crate::tui::scroll::wrap(
            &text.lines,
            text.highlights.iter().chain(extra_highlights),
            Some(text.style),
            area.width as usize,
            max_lines,
            initial,
            scroll.position,
        );

        // check if no space for the border
        if let Some(borders) = &mut borders && borders.bottom {
            borders.inner.height = borders.inner.height.min((max_height - scrolled.range.len()) as u16 - borders.inner.y);
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
            area,
            max_width: full_width,
            line_count: 0,
            lines: scrolled.into_lines(),
            alignment: text.alignment,
            need_newline: None,
            clear_cell,
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
}

impl Renderer for TextRenderer<'_> {
    fn draw_one_line<W, C, F>(
        &mut self,
        drawer: &mut Drawer<W, C>,
        newlines: bool,
        pad_to: (u16, &Cell),
        callback: &mut Option<F>,
    ) -> std::io::Result<bool>
    where
        W :Write,
        C: Canvas,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize),
    {

        // draw top border
        if let Some(borders) = &mut self.borders && borders.top && !borders.top().is_empty() {
            Self::pad_to(drawer, pad_to.0, pad_to.1)?;
            for cell in &borders.top()[..self.max_width] {
                drawer.draw_cell(cell, false)?;
            }
            borders.top_y += 1;
            self.need_newline = Some(None);
            return Ok(true)
        }

        // draw a line
        if let Some(slice) = self.lines.next() {
            Self::pad_to(drawer, pad_to.0, pad_to.1)?;
            let line = self.lines.slice(slice);
            let lineno = self.line_count;
            self.line_count += 1;

            if newlines && let Some(need_newline) = self.need_newline.take() {
                drawer.goto_newline(need_newline.as_ref())?;
            }
            self.need_newline = Some(self.clear_cell.clone());

            // draw left border
            if let Some(borders) = &self.borders && borders.left {
                for cell in borders.left() {
                    drawer.draw_cell(cell, false)?;
                }
            }

            let left_edge = drawer.get_pos().0;
            let line_width = line.iter()
                .map(|token| if let WrapToken::String(str) = &token.inner {
                    str.width()
                } else {
                    0
                }).sum();

            // draw the indent
            for _ in 0 .. self.get_alignment_indent(self.area.width as _, line_width) {
                drawer.draw_cell(&self.indent_cell, false)?;
            }

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
            if let Some((scrollbar_range, cell)) = &self.scrollbar_range && scrollbar_range.contains(&lineno) {
                drawer.clear_to_end_of_line(self.clear_cell.as_ref())?;
                let pos = drawer.get_pos();
                drawer.move_to((left_edge + self.area.width, pos.1));
                drawer.draw_cell(cell, false)?;
            }

            // draw right border
            if let Some(borders) = &self.borders && borders.right {
                let right = borders.right();
                if !right.is_empty() {
                    drawer.clear_to_end_of_line(self.clear_cell.as_ref())?;
                    let pos = drawer.get_pos();
                    drawer.move_to((left_edge + borders.inner.width, pos.1));
                    for cell in right {
                        drawer.draw_cell(cell, false)?;
                    }
                }
            }
            return Ok(true)
        }

        // draw bottom border
        if let Some(borders) = &mut self.borders && borders.bottom && !borders.bottom().is_empty() {
            Self::pad_to(drawer, pad_to.0, pad_to.1)?;
            if newlines && let Some(need_newline) = self.need_newline.take() {
                drawer.goto_newline(need_newline.as_ref())?;
            }
            for cell in &borders.bottom()[..self.max_width] {
                drawer.draw_cell(cell, false)?;
            }
            borders.inner.height += 1;
            return Ok(true)
        }

        drawer.clear_to_end_of_line(self.need_newline.as_ref().unwrap_or(&None).as_ref())?;
        Ok(false)
    }
}
