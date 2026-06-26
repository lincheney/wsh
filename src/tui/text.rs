use std::ops::Range;
use bstr::{BStr, BString, ByteVec};
use unicode_width::UnicodeWidthStr;
use crate::tui::{Style, Cell};
use super::wrap::WrapToken;
mod renderer;
pub use renderer::{Renderer, TextRenderer};

pub(super) const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone)]
pub struct Highlight<T> {
    pub style: Style,
    pub blend: bool,
    pub namespace: T,
    pub virtual_text: Option<BString>,
    pub conceal: Option<bool>,
    pub priority: f64,
}

impl<T> Highlight<T> {
    fn is_empty(&self) -> bool {
        self.style == Style::default()
        && self.virtual_text.as_ref().is_none_or(|s| s.is_empty())
        && !self.conceal.unwrap_or_default()
    }

    pub fn may_cause_resize(&self) -> bool {
        // only conceal and virtual text affect sizing
        self.conceal.unwrap_or_default() || self.has_virtual_text()
    }

    pub fn has_virtual_text(&self) -> bool {
        self.virtual_text.as_ref().is_some_and(|x| !x.is_empty())
    }
}

impl<T: Default> From<Style> for Highlight<T> {
    fn from(style: Style) -> Self {
        Self {
            style,
            blend: true,
            namespace: T::default(),
            virtual_text: None,
            conceal: None,
            priority: 0.,
        }
    }
}

#[derive(Debug, Clone)]
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
    lines: Vec<BString>,
    pub alignment: Alignment,
    pub highlights: Vec<HighlightedRange<T>>,
    pub style: Style,
    pub dirty: bool,
}

fn add_highlight<T>(highlights: &mut Vec<HighlightedRange<T>>, hl: HighlightedRange<T>) {
    // sort in reverse order of priority so higher priority comes first
    let index = match highlights.binary_search_by(|x| hl.lineno.cmp(&x.lineno).then(hl.inner.priority.total_cmp(&x.inner.priority).reverse())) {
        Ok(index) | Err(index) => index,
    };
    highlights.insert(index, hl);
}

impl<T> Text<T> {

    pub fn get(&self) -> &[BString] {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn add_highlight(&mut self, hl: HighlightedRange<T>) {
        add_highlight(&mut self.highlights, hl);
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
            add_highlight(&mut self.highlights, HighlightedRange{
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
            add_highlight(&mut self.highlights, HighlightedRange{
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
            self.add_highlight(HighlightedRange{
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
        if let Some(hl) = hl && !hl.is_empty() {
            add_highlight(&mut self.highlights, HighlightedRange{
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

        let mut pos = (initial_indent, 0);
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

            super::wrap::wrap(line, highlights, None, width, initial_indent, |_, token, _| {
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

            if i + 1 != self.lines.len() {
                pos = (0, pos.1 + 1);
            }
        }
        ::log::debug!("DEBUG(behalf)\t{}\t= {:?}", stringify!(123), 123);

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
                add_highlight(&mut self.highlights, HighlightedRange{
                    lineno: old_len + i,
                    start: 0,
                    end: line.len(),
                    inner: hl.clone(),
                });
            }
        }
    }
}
