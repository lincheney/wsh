use std::ops::Range;
use bstr::{BStr, BString, ByteVec};
use crate::tui::{Style, Cell};
mod renderer;
pub use renderer::{Renderer, TextRenderer, NoCallback as NoRendererCallback};
mod highlight;
pub use highlight::{Highlight, HighlightedRange, HighlightedRangeSet};

pub(super) const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy)]
pub struct Scroll {
    pub show_scrollbar: bool,
    pub position: super::scroll::ScrollPosition,
}

impl Default for Scroll {
    fn default() -> Self {
        Self {
            show_scrollbar: true,
            position: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, strum::EnumString, strum::Display)]
#[strum(ascii_case_insensitive)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Default, Clone)]
pub struct Text<T=()> {
    pub(in crate::tui) lines: Vec<BString>,
    pub alignment: Alignment,
    pub highlights: highlight::HighlightedRangeSet<T>,
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

    pub fn clear_highlights(&mut self, range: Option<Range<usize>>) {
        if let Some(range) = range {
            let range = self.highlights.get_range_for_lines(range);
            self.highlights.drain(range);
        } else {
            self.highlights.clear();
        }
    }

    pub fn retain_highlights<F: Fn(&HighlightedRange<T>) -> bool>(&mut self, func: F) {
        self.highlights.retain(func);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.clear_highlights(None);
    }

    pub fn push_line(&mut self, line: BString, hl: Option<Highlight<T>>) {
        if let Some(hl) = hl {
            self.highlights.push(HighlightedRange{
                lineno: self.lines.len(),
                start: 0,
                end: usize::MAX,
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
        for h in self.highlights.iter_mut() {
            if h.lineno >= lineno {
                h.lineno += 1;
            }
        }
        if let Some(hl) = hl {
            self.add_highlight(HighlightedRange{
                lineno,
                start: 0,
                end: usize::MAX,
                inner: hl,
            });
        }
        self.lines.insert(lineno, line);
    }

    pub fn swap_line(&mut self, line: &mut BString, lineno: usize, hl: Option<Highlight<T>>) {
        std::mem::swap(&mut self.lines[lineno], line);
        self.clear_highlights(Some(lineno .. lineno + 1));
        if let Some(hl) = hl && !hl.is_empty() {
            self.highlights.push(HighlightedRange{
                lineno,
                start: 0,
                end: usize::MAX,
                inner: hl,
            });
        }
    }

    pub fn delete_lines(&mut self, range: Range<usize>) {
        let start = range.start;
        let end = range.end;
        drop(self.lines.drain(range));
        let range = self.highlights.get_range_for_lines(start .. end);
        for hl in &mut self.highlights[range.end..] {
            hl.lineno -= 1;
        }
        self.highlights.drain(range);
    }

    pub fn delete_str(&mut self, lineno: usize, offset: usize, length: usize) {
        self.lines[lineno].drain(offset .. offset + length);
        let range = self.highlights.get_range_for_lines(lineno .. lineno + 1);
        for hl in &mut self.highlights[range] {
            hl.shift(offset .. offset + length, offset);
        }
    }

    pub fn reset(&mut self) {
        self.clear();
        self.dirty = true;
    }

    pub fn get_size<'a, I>(
        &'a self,
        width: usize,
        mut initial_indent: usize,
        extra_highlights: I,
    ) -> (usize, usize)
    where
        T: 'a,
        I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    {

        let mut pos = (0, 0);
        let mut start = 0;

        // add a dummy line at the end
        for (i, line) in self.lines.iter().map(|l| l.as_ref()).chain(std::iter::once(BStr::new(b""))).enumerate() {
            let past_end = i >= self.lines.len();

            let line_filter = |h: &HighlightedRange<T>| h.lineno == i || (past_end && h.lineno > i);
            let end = start + self.highlights[start..].partition_point(line_filter);
            let highlights = self.highlights[start..end].iter()
                .chain(extra_highlights.clone().filter(|h| line_filter(h)))
                .filter(|h| h.inner.may_cause_resize());
            start = end;

            // dont draw the dummy line if there is no virtual text
            if past_end && !highlights.clone().any(|hl| hl.inner.has_virtual_text()) {
                continue
            }

            let (x, y) = super::wrap::wrap(line, highlights, None, width, initial_indent, super::wrap::NoCallback::None);
            pos.0 = x;
            pos.1 += y;
            initial_indent = 0;

            if i + 1 != self.lines.len() {
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
            Some(Cell::new_with_style(" ", self.style.clone()))
        }
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

    pub fn insert_lines<I: IntoIterator<Item=BString>>(&mut self, lines: I, lineno: usize, hl: Option<Highlight<T>>) {
        let old_len = self.lines.len();
        self.lines.splice(lineno..lineno, lines);
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

    pub fn insert_str(&mut self, str: &BStr, lineno: usize, offset: usize, retain_highlights: bool, hl: Option<Highlight<T>>) {
        // shift highlights
        let range = self.highlights.get_range_for_lines(lineno .. lineno + 1);
        if retain_highlights {
            for h in &mut self.highlights[range] {
                h.shift(offset .. offset, str.len());
            }
        } else {
            let split: Vec<_> = self.highlights[range].iter_mut()
                .filter_map(|hl| hl.split(offset))
                .filter(|hl| !hl.is_empty())
                .collect();
            for mut hl in split {
                hl.start += str.len();
                self.add_highlight(hl);
            }
        }

        if let Some(hl) = hl && !hl.is_empty() {
            self.highlights.push(HighlightedRange{
                lineno,
                start: offset,
                end: offset + str.len(),
                inner: hl,
            });
        }
        self.lines[lineno].insert_str(offset, str);
    }

}
