use std::io::{Write, Cursor};
use std::ops::Range;
use bstr::{BStr, BString, ByteVec, ByteSlice};
use unicode_width::UnicodeWidthStr;
use ratatui::style::{Style, Color};
use ratatui::text::{Line, Span};
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::{Block, WidgetRef};
use ratatui::buffer::{Buffer, Cell};
use crate::tui::{Drawer, Canvas};

pub(super) const TAB_WIDTH: usize = 4;
const ESCAPE_STYLE: Style = Style::new().fg(Color::Gray);
const SCROLLBAR_CHAR: &str = "▕";

#[derive(Debug, Clone)]
pub struct Highlight<T> {
    pub style: Style,
    pub blend: bool,
    pub namespace: T,
}

impl<T: Default> From<Style> for Highlight<T> {
    fn from(style: Style) -> Self {
        Self {
            style,
            blend: true,
            namespace: T::default(),
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
            self.start = self.start + new_end - range.end;
        } else if range.start <= self.start {
            self.start = new_end;
        }

        if range.end < self.end {
            self.end = self.end + new_end - range.end;
        } else if range.start < self.end {
            self.end = new_end;
        }

        self.start = self.start.min(self.end);
    }

    fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn namespace(&self) -> &T {
        &self.inner.namespace
    }
}


fn merge_highlights<'a, T: 'a, I: Iterator<Item=&'a Highlight<T>>>(init: Style, iter: I) -> Style {
    let mut style = init;
    for h in iter {
        if !h.blend {
            // start from scratch
            style = Style::new();
        }
        style = style.patch(h.style);
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

    pub fn get_height_for_width(&self, width: usize, initial_indent: usize) -> usize {
        self.lines.iter().flat_map(|line| super::wrap::wrap(line.as_ref(), width, initial_indent)).count()
    }

    fn make_line_cells<E, F: FnMut(usize, usize, Option<(&str, Style)>) -> Result<(), E>>(
        &self,
        lineno: usize,
        line: &BStr,
        range: (usize, usize),
        mut callback: F,
    ) -> Result<(), E> {

        let mut style = self.style;
        for (start, end, c) in line[range.0 .. range.1].grapheme_indices() {
            let start = start + range.0;
            let end = end + range.0;

            if self.highlights.iter().any(|h| h.lineno == lineno && (h.start == start || h.end == start)) {
                let highlights = self.highlights.iter()
                    .filter(|h| h.lineno == lineno && h.start <= start && start < h.end)
                    .map(|h| &h.inner);
                style = merge_highlights(self.style, highlights);
            }

            if c == "\n" {
                // do nothing
                callback(start, end, None)?;
            } else if c == "\t" {
                for _ in 0 .. TAB_WIDTH {
                    callback(start, end, Some((" ", style)))?;
                }
            } else if c.width() > 0 && c != "\u{FFFD}" {
                callback(start, end, Some((c, style)))?;
            } else {
                // invalid
                let style = style.patch(ESCAPE_STYLE);
                let mut cursor = Cursor::new([0; 64]);
                for c in line[start..end].iter() {
                    cursor.set_position(0);
                    write!(cursor, "<u{c:04x}>").unwrap();
                    let buf = &cursor.get_ref()[..cursor.position() as usize];
                    for c in buf {
                        let c = [*c];
                        let c = std::str::from_utf8(&c).unwrap();
                        callback(start, end, Some((c, style)))?;
                    }
                }
            }
        }
        Ok(())
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

    pub fn render_line<W :Write, C: Canvas>(
        &self,
        lineno: usize,
        line: &BStr,
        range: (usize, usize),
        drawer: &mut Drawer<W, C>,
        marker: Option<(usize, usize)>,
    ) -> std::io::Result<Option<(u16, u16)>> {

        let mut marker_pos = None;
        let mut cell = Cell::EMPTY;
        self.make_line_cells(lineno, line, range, |_start, end, data| {
            let result = if let Some((symbol, style)) = data {
                cell.reset();
                cell.set_symbol(symbol);
                cell.set_style(style);
                drawer.draw_cell(&cell, false)
            } else {
                Ok(())
            };
            if Some((lineno, end)) == marker {
                marker_pos = Some(drawer.get_pos());
            }
            result
        })?;
        Ok(marker_pos)
    }

    pub fn get_alignment_indent(&self, max_width: usize, line_width: usize) -> usize {
        match self.alignment {
            Alignment::Left => 0,
            Alignment::Right => max_width.saturating_sub(line_width),
            Alignment::Center => max_width.saturating_sub(line_width) / 2,
        }
    }

    pub fn render<W :Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
        mut block: Option<(&Block<'_>, &mut Buffer)>,
        marker: Option<(usize, usize)>,
        max_height: Option<(usize, Scroll)>,

    ) -> std::io::Result<(u16, u16)> {

        struct Borders<'a> {
            top: &'a [Cell],
            bottom: &'a [Cell],
            left: &'a [Cell],
            right: &'a [Cell],
        }

        let full_width = drawer.term_width() as usize;
        let full_height = drawer.term_height() as usize;
        let mut area = Rect{ x: 0, y: 0, height: full_height as u16, width: full_width as u16 };

        let borders = if let Some((block, ref mut buffer)) = block {
            // 3 lines in case you have borders
            buffer.resize(Rect{ height: 3, ..area });
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

        let mut marker_pos = drawer.get_pos();
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
        let scrolled = super::scroll::wrap(&self.lines, area.width as usize, max_lines, initial, scroll.position);
        max_height -= scrolled.in_view.len();

        let scrollbar_range = if scroll.show_scrollbar {
            area.width -= 1;

            let mut start = scrolled.in_view.start * scrolled.in_view.len() / scrolled.ranges.len().max(1);
            if scrolled.in_view.start > 0 && start == 0 {
                start = 1;
            }
            let mut end = scrolled.in_view.end * scrolled.in_view.len() / scrolled.ranges.len().max(1);
            if scrolled.in_view.end < scrolled.ranges.len() && end == scrolled.in_view.len() {
                end = end.saturating_sub(1);
            }

            let mut cell = Cell::new(SCROLLBAR_CHAR);
            cell.set_style(self.style);
            Some((start .. end, cell))
        } else {
            None
        };

        for (i, &(lineno, (range, line_width))) in scrolled.ranges[scrolled.in_view].iter().enumerate() {

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

            // draw the indent
            for _ in 0 .. self.get_alignment_indent(area.width as _, line_width) {
                drawer.draw_cell(&indent_cell, false)?;
            }

            // draw the line
            if let Some(pos) = self.render_line(lineno, self.lines[lineno][range.0 .. range.1].as_ref(), range, drawer, marker)? {
                marker_pos = pos;
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

        Ok(marker_pos)
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
        let _: Result<(), ()> = val.make_line_cells(0, line.as_ref(), (0, line.len()), |_, _, cell| {
            if let Some((str, style)) = cell {
                if style != prev_style {
                    if !string.is_empty() {
                        let new_string = std::mem::take(&mut string);
                        spans.push(Span::styled(new_string, prev_style));
                    }
                    prev_style = style;
                }
                string.push_str(str);
            }
            Ok(())
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
