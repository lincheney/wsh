use std::ops::ControlFlow;
use std::ops::Range;
use bstr::{BStr, BString, ByteVec};
use crate::tui::{Style, Cell};
mod renderer;
pub use renderer::{Renderer, TextRenderer, NoCallback as NoRendererCallback};
pub use super::scroll::ScrollPosition;
mod highlight;
pub use highlight::{Highlight, HighlightedRange, HighlightedRangeSet};

pub(super) const TAB_WIDTH: usize = 4;

#[derive(Default, Debug, Clone, Copy)]
pub struct Scroll {
    pub show_scrollbar: bool,
    pub position: super::scroll::ScrollPosition,
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
pub struct Text<T=(), S=BString> {
    pub(in crate::tui) paragraphs: Vec<BString>,
    pub alignment: Alignment,
    pub highlights: highlight::HighlightedRangeSet<T, S>,
    pub style: Style,
    pub dirty: bool,
}

impl<T, S> Text<T, S> {

    pub fn get(&self) -> &[BString] {
        &self.paragraphs
    }

    pub fn len(&self) -> usize {
        self.paragraphs.len()
    }

    pub fn add_highlight(&mut self, hl: HighlightedRange<T, S>) {
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

    pub fn retain_highlights<F: Fn(&HighlightedRange<T, S>) -> bool>(&mut self, func: F) {
        self.highlights.retain(func);
    }

    pub fn clear(&mut self) {
        self.paragraphs.clear();
        self.clear_highlights(None);
    }

    pub fn push_line(&mut self, line: BString, hl: Option<Highlight<T, S>>) {
        if let Some(hl) = hl {
            self.highlights.push(HighlightedRange{
                parano: self.paragraphs.len(),
                start: 0,
                end: usize::MAX,
                inner: hl,
            });
        }
        self.paragraphs.push(line);
    }

    pub fn push_str(&mut self, str: &BStr, hl: Option<Highlight<T, S>>) {
        if self.paragraphs.is_empty() {
            self.push_line(b"".into(), None);
        }
        let parano = self.paragraphs.len() - 1;
        let line = self.paragraphs.last_mut().unwrap();
        if let Some(hl) = hl {
            self.highlights.push(HighlightedRange{
                parano,
                start: line.len(),
                end: line.len() + str.len(),
                inner: hl,
            });
        }
        line.push_str(str);
    }

    pub fn insert_line(&mut self, line: BString, parano: usize, hl: Option<Highlight<T, S>>) {
        // shift highlights
        for h in self.highlights.iter_mut() {
            if h.parano >= parano {
                h.parano += 1;
            }
        }
        if let Some(hl) = hl {
            self.add_highlight(HighlightedRange{
                parano,
                start: 0,
                end: usize::MAX,
                inner: hl,
            });
        }
        self.paragraphs.insert(parano, line);
    }

    pub fn delete_lines(&mut self, range: Range<usize>) {
        let start = range.start;
        let end = range.end;
        drop(self.paragraphs.drain(range));
        let range = self.highlights.get_range_for_lines(start .. end);
        for hl in &mut self.highlights[range.end..] {
            hl.parano -= 1;
        }
        self.highlights.drain(range);
    }

    pub fn delete_str(&mut self, parano: usize, offset: usize, length: usize) {
        self.paragraphs[parano].drain(offset .. offset + length);
        let range = self.highlights.get_range_for_lines(parano .. parano + 1);
        for hl in &mut self.highlights[range] {
            hl.shift(offset .. offset + length, offset);
        }
    }

    pub fn reset(&mut self) {
        self.clear();
        self.dirty = true;
    }
}

impl<T, S: AsRef<BStr>> Text<T, S> {

    pub fn swap_line(&mut self, line: &mut BString, parano: usize, hl: Option<Highlight<T, S>>) {
        std::mem::swap(&mut self.paragraphs[parano], line);
        self.clear_highlights(Some(parano .. parano + 1));
        if let Some(hl) = hl && !hl.is_empty() {
            self.highlights.push(HighlightedRange{
                parano,
                start: 0,
                end: usize::MAX,
                inner: hl,
            });
        }
    }

    pub fn get_first_line_width<'a, I>(
        &'a self,
        width: usize,
        initial_indent: usize,
        extra_highlights: I,
    ) -> usize
    where
        T: 'a,
        I: Clone + Iterator<Item=&'a HighlightedRange<T, S>>,
    {

        let empty = self.paragraphs.is_empty();
        let highlights = self.highlights.iter()
            .skip_while(|hl| empty || hl.parano == 0)
            .chain(extra_highlights.clone().filter(|hl| empty || hl.parano == 0))
            .filter(|h| h.inner.may_cause_resize());

        let mut first_line_width = 0;

        // skip if no text, not even virtual text
        if !empty || highlights.clone().any(|hl| hl.inner.has_virtual_text()) {
            let paragraph = self.paragraphs.first().map(|p| p.as_ref()).unwrap_or_default();
            super::wrap::wrap(paragraph, highlights, None, width, initial_indent, Some(|_, token, _, _, _| {
                if matches!(token, super::wrap::WrapToken::LineBreak) {
                    ControlFlow::Break(())
                } else {
                    first_line_width += 1;
                    ControlFlow::Continue(())
                }
            }));
        }

        first_line_width
    }

    pub fn get_size<'a, I>(
        &'a self,
        width: usize,
        mut initial_indent: usize,
        extra_highlights: I,
    ) -> (usize, usize)
    where
        T: 'a,
        I: Clone + Iterator<Item=&'a HighlightedRange<T, S>>,
    {

        let mut last_line_width = 0;
        let mut height = 0;
        let mut start = 0;

        // add a dummy line at the end
        for (i, para) in self.paragraphs.iter().map(|l| l.as_ref()).chain(std::iter::once(BStr::new(b""))).enumerate() {
            let past_end = i >= self.paragraphs.len();

            let line_filter = |h: &HighlightedRange<T, S>| h.parano == i || (past_end && h.parano > i);
            let end = start + self.highlights[start..].partition_point(line_filter);
            let highlights = self.highlights[start..end].iter()
                .chain(extra_highlights.clone().filter(|h| line_filter(h)))
                .filter(|h| h.inner.may_cause_resize());
            start = end;

            // dont draw the dummy line if there is no virtual text
            if past_end && !highlights.clone().any(|hl| hl.inner.has_virtual_text()) {
                continue
            }

            let (x, y, _lineno) = super::wrap::wrap(para, highlights, None, width, initial_indent, super::wrap::NoCallback::None);
            let x = x.saturating_sub(initial_indent);
            last_line_width = x;
            height += y;
            initial_indent = 0;

            if i + 1 != self.paragraphs.len() {
                last_line_width = 0;
                height += 1;
            }
        }

        if height != 0 || last_line_width != 0 {
            height += 1;
        }
        (last_line_width, height)
    }

    pub fn make_default_style_cell(&self) -> Option<Cell> {
        if self.style == Style::default() {
            None
        } else {
            Some(Cell::new_with_style(" ", self.style.clone()))
        }
    }

}

impl<T: Clone, S: Clone> Text<T, S> {
    pub fn push_lines<I: IntoIterator<Item=BString>>(&mut self, lines: I, hl: Option<Highlight<T, S>>) {
        let old_len = self.paragraphs.len();
        self.paragraphs.extend(lines);
        if let Some(hl) = hl {
            for (i, line) in self.paragraphs[old_len..].iter().enumerate() {
                self.highlights.push(HighlightedRange{
                    parano: old_len + i,
                    start: 0,
                    end: line.len(),
                    inner: hl.clone(),
                });
            }
        }
    }

    pub fn insert_lines<I: IntoIterator<Item=BString>>(&mut self, lines: I, parano: usize, hl: Option<Highlight<T, S>>) {
        let old_len = self.paragraphs.len();
        self.paragraphs.splice(parano..parano, lines);
        if let Some(hl) = hl {
            for (i, line) in self.paragraphs[old_len..].iter().enumerate() {
                self.highlights.push(HighlightedRange{
                    parano: old_len + i,
                    start: 0,
                    end: line.len(),
                    inner: hl.clone(),
                });
            }
        }
    }
}

impl<T: Clone, S: Clone + AsRef<BStr>> Text<T, S> {
    pub fn insert_str(&mut self, str: &BStr, parano: usize, offset: usize, retain_highlights: bool, hl: Option<Highlight<T, S>>) {
        // shift highlights
        let range = self.highlights.get_range_for_lines(parano .. parano + 1);
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
                parano,
                start: offset,
                end: offset + str.len(),
                inner: hl,
            });
        }
        self.paragraphs[parano].insert_str(offset, str);
    }

}
