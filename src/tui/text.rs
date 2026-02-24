use std::io::{Write};
use std::ops::Range;
use bstr::{BStr, BString, ByteVec, ByteSlice};
use unicode_width::UnicodeWidthStr;
use ratatui::style::{Style};
use ratatui::text::{Line, Span};
use ratatui::layout::{Alignment};
use ratatui::widgets::{Block};
use ratatui::buffer::{Cell};
use crate::tui::{Drawer, Canvas};
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
}

impl<T: Default> From<Style> for Highlight<T> {
    fn from(style: Style) -> Self {
        Self {
            style,
            blend: true,
            namespace: T::default(),
            virtual_text: None,
            conceal: None,
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

        for (lineno, line) in self.lines.iter().enumerate() {
            let highlights = self.highlights.iter().chain(extra_highlights.clone()).filter(|h| h.lineno == lineno);
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

    pub fn make_renderer<'a, W, C, I>(
        &'a self,
        drawer: &mut Drawer<W, C>,
        block: Option<&'a Block<'a>>,
        max_width: Option<usize>,
        max_height: Option<(usize, Scroll)>,
        extra_highlights: I,
    ) -> renderer::TextRenderer<'a>
    where
        T: 'a,
        W :Write,
        C: Canvas,
        I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    {
        renderer::TextRenderer::new(self, drawer, block, max_width, max_height, extra_highlights)
    }

    pub fn render<'a, W, C, I>(
        &'a self,
        drawer: &mut Drawer<W, C>,
        newlines: bool,
        block: Option<&'a Block<'a>>,
        max_width: Option<usize>,
        max_height: Option<(usize, Scroll)>,
        extra_highlights: I,
    ) -> std::io::Result<()>
    where
        T: 'a,
        W :Write,
        C: Canvas,
        I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    {
        self.render_with_callback::<W, C, I, fn(&mut Drawer<W, C>, usize, usize, usize)>(
            drawer,
            newlines,
            block,
            max_width,
            max_height,
            extra_highlights,
            None,
        )
    }

    pub fn render_with_callback<'a, W, C, I, F>(
        &'a self,
        drawer: &mut Drawer<W, C>,
        newlines: bool,
        block: Option<&'a Block<'a>>,
        max_width: Option<usize>,
        max_height: Option<(usize, Scroll)>,
        extra_highlights: I,
        callback: Option<F>,
    ) -> std::io::Result<()>
    where
        T: 'a,
        W :Write,
        C: Canvas,
        I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize),
    {
        self.make_renderer(drawer, block, max_width, max_height, extra_highlights)
            .render(drawer, newlines, (0, &Cell::EMPTY), callback)
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
