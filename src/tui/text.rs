use std::io::{Write};
use std::ops::Range;
use bstr::{BStr, BString, ByteVec, ByteSlice};
use unicode_width::UnicodeWidthStr;
use ratatui::style::{Style, Modifier, Stylize};
use ratatui::text::{Line, Span};
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::{Block, WidgetRef};
use ratatui::buffer::{Buffer, Cell};
use crate::tui::{Drawer, Canvas};
use super::wrap::WrapToken;

pub(super) const TAB_WIDTH: usize = 4;
const SCROLLBAR_CHAR: &str = "▕";

#[derive(Debug, Clone)]
pub struct Highlight<T> {
    pub style: Style,
    pub blend: bool,
    pub namespace: T,
    pub virtual_text: Option<BString>,
}

impl<T: Default> From<Style> for Highlight<T> {
    fn from(style: Style) -> Self {
        Self {
            style,
            blend: true,
            namespace: T::default(),
            virtual_text: None,
        }
    }
}

#[derive(Debug)]
pub struct HighlightedRange<T> {
    pub lineno: usize,
    pub start: usize,
    pub end: usize,
    pub inner: Highlight<T>,
}

impl<T> HighlightedRange<T> {
    fn shift(&mut self, range: Range<usize>, new_end: usize) {
        if range.end <= self.start {
            self.start = self.start.saturating_add(new_end) - range.end;
        } else if range.start <= self.start {
            self.start = new_end;
        }

        if range.end < self.end {
            self.end = self.end.saturating_add(new_end) - range.end;
        } else if range.start < self.end {
            self.end = new_end;
        }

        self.start = self.start.min(self.end);
    }

    fn is_empty(&self) -> bool {
        self.start == self.end && self.inner.virtual_text.is_none()
    }

    pub fn namespace(&self) -> &T {
        &self.inner.namespace
    }
}


pub fn merge_highlights<'a, T: 'a, I: Iterator<Item=&'a Highlight<T>>>(init: Style, iter: I) -> Style {
    let mut style = init;
    for h in iter {
        if !h.blend {
            // start from scratch
            style = Style::new();
        }
        let reverse = style.add_modifier.contains(Modifier::REVERSED);
        style = style.patch(h.style);
        if reverse == h.style.add_modifier.contains(Modifier::REVERSED) {
            style = style.not_reversed();
        } else {
            style = style.reversed();
        }
    }
    style
}

pub struct Scroll {
    pub show_scrollbar: bool,
    pub position: super::scroll::ScrollPosition,
}

#[derive(Debug, Default)]
pub struct Text<T=()> {
    lines: Vec<BString>,
    pub alignment: Alignment,
    pub highlights: Vec<HighlightedRange<T>>,
    pub style: Style,
    pub dirty: bool,
}

impl<T> Text<T> {

    pub fn get(&self) -> &[BString] {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn add_highlight(&mut self, hl: HighlightedRange<T>) {
        self.highlights.push(hl);
    }

    pub fn clear_highlights(&mut self) {
        self.highlights.clear();
    }

    pub fn retain_highlights<F: Fn(&HighlightedRange<T>) -> bool>(&mut self, func: F) {
        self.highlights.retain(func);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.clear_highlights();
    }

    pub fn push_line(&mut self, line: BString, hl: Option<Highlight<T>>) {
        if let Some(hl) = hl {
            self.highlights.push(HighlightedRange{
                lineno: self.lines.len(),
                start: 0,
                end: line.len(),
                inner: hl,
            });
        }
        self.lines.push(line);
    }

    pub fn push_str(&mut self, str: &BStr, hl: Option<Highlight<T>>) {
        if self.lines.is_empty() {
            self.push_line(b"".into(), None);
        }
        let lineno = self.lines.len() - 1;
        let line = self.lines.last_mut().unwrap();
        if let Some(hl) = hl {
            self.highlights.push(HighlightedRange{
                lineno,
                start: line.len(),
                end: line.len() + str.len(),
                inner: hl,
            });
        }
        line.push_str(str);
    }

    pub fn insert_line(&mut self, line: BString, lineno: usize, hl: Option<Highlight<T>>) {
        // shift highlights
        for h in &mut self.highlights {
            if h.lineno >= lineno {
                h.lineno += 1;
            }
        }
        if let Some(hl) = hl {
            self.highlights.push(HighlightedRange{
                lineno,
                start: 0,
                end: line.len(),
                inner: hl,
            });
        }
        self.lines.insert(lineno, line);
    }

    pub fn insert_str(&mut self, str: &BStr, lineno: usize, offset: usize, hl: Option<Highlight<T>>) {
        // shift highlights
        for h in &mut self.highlights {
            if h.lineno == lineno {
                h.shift(offset .. offset, str.len());
            }
        }
        if let Some(hl) = hl {
            self.highlights.push(HighlightedRange{
                lineno,
                start: offset,
                end: offset + str.len(),
                inner: hl,
            });
        }
        self.lines[lineno].insert_str(offset, str);
    }

    pub fn swap_line(&mut self, line: &mut BString, lineno: usize) {
        std::mem::swap(&mut self.lines[lineno], line);
        self.highlights.retain_mut(|h| h.lineno != lineno);
    }

    pub fn delete_line(&mut self, lineno: usize) -> BString {
        let line = self.lines.remove(lineno);
        self.highlights.retain_mut(|hl| {
            if hl.lineno == lineno {
                false
            } else {
                if hl.lineno > lineno {
                    hl.lineno -= 1;
                }
                true
            }
        });
        line
    }

    pub fn delete_str(&mut self, lineno: usize, offset: usize, length: usize) {
        self.lines[lineno].drain(offset .. offset + length);
        self.highlights.retain_mut(|h| {
            if h.lineno == lineno {
                h.shift(offset .. offset + length, offset);
            }
            !h.is_empty()
        });
    }

    pub fn reset(&mut self) {
        self.clear();
        self.dirty = true;
    }

    pub fn get_size(&self, width: usize, mut initial_indent: usize) -> (usize, usize) {
        let mut pos = (initial_indent, 0);

        for (lineno, line) in self.lines.iter().enumerate() {
            let highlights = self.highlights.iter().filter(|h| h.lineno == lineno);
            super::wrap::wrap(line.as_ref(), highlights, None, width, initial_indent, |_, _, token, _| {
                match token {
                    WrapToken::LineBreak => {
                        pos = (0, pos.1 + 1);
                    },
                    WrapToken::String(s) => {
                        pos.0 += s.width();
                    },
                }
            });
            initial_indent = 0;

            if lineno != self.lines.len() - 1 {
                pos = (0, pos.1 + 1);
            }
        }

        if pos != (0, 0) {
            pos.1 += 1;
        }
        pos
    }

    pub fn make_default_style_cell(&self) -> Option<Cell> {
        if self.style == Style::default() {
            None
        } else {
            let mut cell = Cell::new("");
            cell.set_style(self.style);
            Some(cell)
        }
    }

    pub fn get_alignment_indent(&self, max_width: usize, line_width: usize) -> usize {
        match self.alignment {
            Alignment::Left => 0,
            Alignment::Right => max_width.saturating_sub(line_width),
            Alignment::Center => max_width.saturating_sub(line_width) / 2,
        }
    }

    pub fn render<'a, W, C, I>(
        &'a self,
        drawer: &mut Drawer<W, C>,
        block: Option<(&Block<'_>, &mut Buffer)>,
        max_height: Option<(usize, Scroll)>,
        extra_highlights: I,
    ) -> std::io::Result<()>
    where
        T: 'a,
        W :Write,
        C: Canvas,
        I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    {
        self.render_with_callback::<W, C, I, fn(&mut Drawer<W, C>, usize, usize, usize)>(drawer, block, max_height, extra_highlights, None)
    }

    pub fn render_with_callback<'a, W, C, I, F>(
        &'a self,
        drawer: &mut Drawer<W, C>,
        mut block: Option<(&Block<'_>, &mut Buffer)>,
        max_height: Option<(usize, Scroll)>,
        extra_highlights: I,
        mut callback: Option<F>,
    ) -> std::io::Result<()>
    where
        T: 'a,
        W :Write,
        C: Canvas,
        I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize),
    {

        struct Borders<'a> {
            top: &'a [Cell],
            bottom: &'a [Cell],
            left: &'a [Cell],
            right: &'a [Cell],
        }

        let full_width = drawer.term_width() as usize;
        let full_height = drawer.term_height() as usize;
        let mut area = Rect{ x: 0, y: 0, height: full_height as u16, width: full_width as u16 };

        // setup the borders
        let borders = if let Some((block, ref mut buffer)) = block {
            // 3 lines in case you have borders
            buffer.resize(Rect{ height: 3, ..area });
            buffer.reset();
            block.render_ref(buffer.area, buffer);
            let inner_area = block.inner(buffer.area);
            // since the border buffer height is different, the inner height will be wrong
            area = Rect{ height: full_height as u16 - (buffer.area.height - inner_area.height), ..inner_area };

            let cells = &buffer.content;
            Some(Borders{
                top: &cells[.. full_width * inner_area.y as usize],
                bottom: &cells[full_width * (inner_area.y + inner_area.height) as usize ..],
                left: &cells[full_width * inner_area.y as usize ..][.. inner_area.x as usize],
                right: &cells[full_width * inner_area.y as usize ..][(inner_area.x + inner_area.width) as usize .. full_width],
            })
        } else {
            None
        };

        let (max_height, scroll) = if let Some((max_height, scroll)) = max_height {
            (max_height, scroll)
        } else {
            (usize::MAX, Scroll{ show_scrollbar: false, position: super::scroll::ScrollPosition::Line(0) })
        };
        let mut max_height = max_height.min(full_height - drawer.get_pos().1 as usize);
        let border_bottom_height = full_height - (area.y + area.height) as usize;

        let clear_cell = self.make_default_style_cell();
        let mut indent_cell = Cell::EMPTY;
        indent_cell.set_style(self.style);

        let mut need_newline = None;

        // draw top border
        if let Some(borders) = &borders {
            let height = max_height.min(borders.top.len() / full_width);
            if height > 0 {
                drawer.draw_lines(borders.top.chunks(full_width as _).take(height))?;
                need_newline = Some(None);
                max_height -= height;
            }
        }

        let initial = drawer.get_pos().0 as usize % full_width;
        let max_lines = max_height.saturating_sub(border_bottom_height);
        let scrolled = super::scroll::wrap(
            &self.lines,
            self.highlights.iter().chain(extra_highlights),
            Some(self.style),
            area.width as usize,
            max_lines,
            initial,
            scroll.position,
        );
        max_height -= scrolled.range.len();

        let scrollbar_range = if scroll.show_scrollbar {
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
            cell.set_style(self.style);
            Some((start .. end, cell))
        } else {
            None
        };

        for (i, line) in scrolled.lines().enumerate() {

            if let Some(need_newline) = need_newline.take() {
                drawer.goto_newline(need_newline)?;
            }
            need_newline = Some(clear_cell.as_ref());

            // draw left border
            if let Some(borders) = &borders {
                for cell in borders.left {
                    drawer.draw_cell(cell, false)?;
                }
            }

            let line_width = line.iter()
                .map(|token| if let WrapToken::String(str) = &token.inner {
                    str.width()
                } else {
                    0
                }).sum();

            // draw the indent
            for _ in 0 .. self.get_alignment_indent(area.width as _, line_width) {
                drawer.draw_cell(&indent_cell, false)?;
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
                if let Some(callback) = &mut callback {
                    callback(drawer, token.lineno, token.start, token.end);
                }
            }

            // draw the scrollbar
            if let Some((scrollbar_range, cell)) = &scrollbar_range && scrollbar_range.contains(&i) {
                drawer.clear_to_end_of_line(clear_cell.as_ref())?;
                let pos = drawer.get_pos();
                drawer.move_to((area.x + area.width, pos.1));
                drawer.draw_cell(cell, false)?;
            }

            // draw right border
            if let Some(borders) = &borders && !borders.right.is_empty()  {
                drawer.clear_to_end_of_line(clear_cell.as_ref())?;
                let pos = drawer.get_pos();
                drawer.move_to(((full_width - borders.right.len()) as _, pos.1));
                for cell in borders.right {
                    drawer.draw_cell(cell, false)?;
                }
            }
        }

        // draw bottom border
        if let Some(borders) = &borders {
            let height = max_height.min(borders.bottom.len() / full_width);
            if height > 0 {
                if let Some(need_newline) = need_newline.take() {
                    drawer.goto_newline(need_newline)?;
                }
                drawer.draw_lines(borders.bottom.chunks(full_width).take(height))?;
                // max_height -= border_height;
            }
        }

        drawer.clear_to_end_of_line(need_newline.unwrap_or(None))?;

        Ok(())
    }

}

impl<T: Clone> Text<T> {
    pub fn push_lines<I: IntoIterator<Item=BString>>(&mut self, lines: I, hl: Option<Highlight<T>>) {
        let old_len = self.lines.len();
        self.lines.extend(lines);
        if let Some(hl) = hl {
            for (i, line) in self.lines[old_len..].iter().enumerate() {
                self.highlights.push(HighlightedRange{
                    lineno: old_len + i,
                    start: 0,
                    end: line.len(),
                    inner: hl.clone(),
                });
            }
        }
    }
}

impl<T> From<&Text<T>> for Line<'_> {
    fn from(val: &Text<T>) -> Self {
        // only gets the first line
        let Some(line) = val.lines.first()
            else { return Line::default() };

        let line = line.lines().next().unwrap();
        let mut spans = vec![];
        let mut prev_style = Style::new();
        let mut string = String::new();
        let highlights = val.highlights.iter().filter(|hl| hl.lineno == 0);
        super::wrap::wrap(line.into(), highlights, Some(val.style), usize::MAX, 0, |_, _, token, style| {
            let WrapToken::String(str) = token
                else { return };
            let style = style.unwrap();
            if style != prev_style {
                if !string.is_empty() {
                    let new_string = std::mem::take(&mut string);
                    spans.push(Span::styled(new_string, prev_style));
                }
                prev_style = style;
            }
            string.push_str(&str);
        });
        if !string.is_empty() {
            spans.push(Span::styled(string, prev_style));
        }

        Line {
            style: Style::new(),
            alignment: None,
            spans,
        }
    }
}
