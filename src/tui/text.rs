use std::io::{Write, Cursor};
use std::ops::Range;
use bstr::{BStr, BString, ByteVec, ByteSlice};
use unicode_width::UnicodeWidthStr;
use ratatui::style::{Style, Color};
use ratatui::text::{Line, Span};
use crate::tui::{Drawer};

const ESCAPE_STYLE: Style = Style::new().fg(Color::Gray);

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

struct HighlightStack<'a, T>(Vec<&'a HighlightedRange<T>>);

impl<T> HighlightStack<'_, T> {
    fn merge(&self, mut style: Style) -> Style {
        for h in &self.0 {
            if !h.inner.blend {
                // start from scratch
                style = Style::new();
            }
            style = style.patch(h.inner.style);
        }
        style
    }
}

pub fn wrap(line: &BStr, width: usize, initial_indent: usize) -> impl Iterator<Item=((usize, usize), usize)> {
    const TAB_WIDTH: usize = 4;
    let mut line_range = (0, 0);
    let mut line_width = initial_indent;
    let mut graphemes = line.grapheme_indices().fuse();
    let mut invalid = None;

    let mut try_add_width = move |w, end| {
        let old_width = line_width;
        line_width += w;
        if line_width > width {
            // wrap
            line_range = (line_range.1, end);
            line_width = w;
            Some((line_range, old_width))
        } else {
            None
        }
    };

    std::iter::from_fn(move || {
        loop {
            if let Some((start, end)) = invalid.take() {
                // iter over previous invalid text
                let mut cursor = Cursor::new([0; 64]);
                for (i, c) in line[start .. end].iter().enumerate() {
                    cursor.set_position(0);
                    write!(cursor, "<u{c:04x}>").unwrap();
                    if let Some(result) = try_add_width(cursor.position() as usize, start + i) {
                        invalid = Some((start + i, end));
                        return Some(result)
                    }
                }

            } else if let Some((start, end, c)) = graphemes.next() {
                if c == "\n" {
                    // newline
                    let old_width = line_width;
                    line_width = 0;
                    line_range = (line_range.1, end);
                    return Some((line_range, old_width))
                } else if c == "\t" {
                    let result = try_add_width(TAB_WIDTH, start);
                    if result.is_some() {
                        return result
                    }
                } else if c.width() > 0 && c != "\u{FFFD}" {
                    let result = try_add_width(c.width(), start);
                    if result.is_some() {
                        return result
                    }
                } else {
                    // invalid text
                    invalid = Some((start, end));
                }
            } else if line_range.1 < line.len() {
                // no more text, emit last line
                line_range = (line_range.1, line.len());
                return Some((line_range, line_width))
            } else {
                return None
            }
        }
    })
}

pub struct RenderState<'a, T> {
    highlights: HighlightStack<'a, T>,
    cell: ratatui::buffer::Cell,
}

impl<T> RenderState<'_, T> {
    pub fn clear(&mut self) {
        self.highlights.0.clear();
        self.cell = Default::default();
    }
}

impl<T> Default for RenderState<'_, T> {
    fn default() -> Self {
        Self{
            highlights: HighlightStack(vec![]),
            cell: ratatui::buffer::Cell::default(),
        }
    }
}


#[derive(Debug, Default)]
pub struct Text<T=()> {
    lines: Vec<BString>,
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
        self.lines.iter().flat_map(|line| wrap(line.as_ref(), width, initial_indent)).count()
    }

    fn make_line_cells<'a, E, F: FnMut(usize, usize, Option<(&str, Style)>) -> Result<(), E>>(
        &'a self,
        highlights: &mut HighlightStack<'a, T>,
        lineno: usize,
        line: &BStr,
        range: (usize, usize),
        mut callback: F,
    ) -> Result<(), E> {

        let mut style = self.style;
        for (i, (start, end, c)) in line[range.0 .. range.1].grapheme_indices().enumerate() {
            let start = start + range.0;
            let end = end + range.0;

            if self.highlights.iter().any(|h| h.lineno == lineno && (h.start == i || h.end == i)) {
                highlights.0.splice(.., self.highlights.iter().filter(|h| h.lineno == lineno && h.start <= i && i < h.end));
                style = highlights.merge(self.style);
            }

            if c == "\n" {
                // do nothing
                callback(start, end, None)?;
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

    pub fn render_line<'a, W :Write>(
        &'a self,
        state: &mut RenderState<'a, T>,
        lineno: usize,
        line: &BStr,
        range: (usize, usize),
        drawer: &mut Drawer<W>,
        marker: Option<(usize, usize)>,
    ) -> std::io::Result<Option<(u16, u16)>> {

        let mut marker_pos = None;
        self.make_line_cells(&mut state.highlights, lineno, line, range, |_start, end, data| {
            let result = if let Some((symbol, style)) = data {
                state.cell.reset();
                state.cell.set_symbol(symbol);
                state.cell.set_style(style);
                drawer.draw_cell(&state.cell, false)
            } else {
                Ok(())
            };
            if Some((lineno, end)) == marker {
                marker_pos = Some(drawer.cur_pos);
            }
            result
        })?;
        Ok(marker_pos)
    }

    pub fn render<W :Write>(
        &self,
        drawer: &mut Drawer<W>,
        marker: Option<(usize, usize)>,
    ) -> std::io::Result<(u16, u16)> {

        let width = drawer.term_width() as _;
        let mut state = RenderState::default();
        let mut marker_pos = drawer.cur_pos;
        let mut first_line = true;

        for (lineno, line) in self.lines.iter().enumerate() {
            state.clear();

            for (range, _width) in wrap(line.as_ref(), width, drawer.cur_pos.0 as _) {
                if !first_line {
                    drawer.goto_newline()?;
                }
                first_line = false;

                if let Some(pos) = self.render_line(&mut state, lineno, line.as_ref(), range, drawer, marker)? {
                    marker_pos = pos;
                }
            }
        }
        drawer.clear_to_end_of_line()?;

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
        let Some(line) = val.lines.get(0)
            else { return Default::default() };

        let line = line.lines().next().unwrap();
        let mut spans = vec![];
        let mut state = RenderState::default();
        let mut prev_style = Style::new();
        let mut string = String::new();
        let _: Result<(), ()> = val.make_line_cells(&mut state.highlights, 0, line.as_ref(), (0, line.len()), |_, _, cell| {
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
