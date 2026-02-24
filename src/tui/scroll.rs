use std::ops::Range;
use super::text::{HighlightedRange};
use ratatui::style::{Style};
use bstr::{BString};
use super::wrap::WrapToken;

#[derive(Clone, Copy)]
pub enum ScrollPosition {
    Line(usize),
    StickyBottom,
}

#[derive(Debug)]
pub struct ScrollWrapToken<'a> {
    pub lineno: usize,
    visual_lineno: usize,
    pub start: usize,
    pub end: usize,
    pub inner: WrapToken<'a>,
    pub style: Option<Style>,
}

pub struct Scrolled<'a> {
    pub total_line_count: usize,
    pub range: Range<usize>,
    pub in_view: Vec<ScrollWrapToken<'a>>,
}

pub struct ScrolledLinesIter<'a> {
    inner: Vec<ScrollWrapToken<'a>>,
    start: usize,
}

impl ScrolledLinesIter<'_> {
    pub fn slice(&self, range: std::ops::Range<usize>) -> &[ScrollWrapToken<'_>] {
        &self.inner[range]
    }

    pub fn has_more(&self) -> bool {
        self.start < self.inner.len()
    }
}

impl Iterator for ScrolledLinesIter<'_> {
    type Item = std::ops::Range<usize>;
    fn next(&mut self) -> Option<Self::Item> {
        if !self.has_more() {
            return None
        }

        let mut end = self.start + 1;
        let end = loop {
            match self.inner.get(end) {
                Some(x) if matches!(x.inner, WrapToken::LineBreak) => break end,
                Some(_) => { end += 1; },
                None => break self.inner.len(),
            }
        };
        let result = self.start .. end;
        self.start = end;
        Some(result)
    }
}

impl<'a> Scrolled<'a> {
    pub fn into_lines(self) -> ScrolledLinesIter<'a> {
        ScrolledLinesIter{
            inner: self.in_view,
            start: 0,
        }
    }
}

pub fn wrap<'a, T: 'a, I: Clone + Iterator<Item=&'a HighlightedRange<T>> >(
    lines: &'a [BString],
    highlights: I,
    init_style: Option<Style>,
    max_width: usize,
    max_height: Option<usize>,
    mut initial_indent: usize,
    scroll: ScrollPosition,
) -> Scrolled<'a> {

    let lineno = match scroll {
        ScrollPosition::Line(lineno) => lineno.min(lines.len().saturating_sub(1)),
        ScrollPosition::StickyBottom => lines.len().saturating_sub(1),
    };

    let mut total_line_count = 0;
    let mut tokens = vec![];
    let mut start = 0;
    let end;
    for (i, line) in lines.iter().enumerate() {
        if i < lineno {
            start = total_line_count;
        }
        super::wrap::wrap(
            line.as_ref(),
            highlights.clone().filter(|hl| hl.lineno == i),
            init_style,
            max_width,
            initial_indent,
            |start, end, token, style| {
                let is_line_break = matches!(token, WrapToken::LineBreak);
                tokens.push(ScrollWrapToken {
                    lineno: i,
                    visual_lineno: total_line_count,
                    start,
                    end,
                    inner: token,
                    style,
                });
                if is_line_break {
                    total_line_count += 1;
                }
            },
        );
        initial_indent = 0;

        tokens.push(ScrollWrapToken {
            lineno: i,
            visual_lineno: total_line_count,
            start: line.len(),
            end: line.len(),
            inner: WrapToken::LineBreak,
            style: None,
        });
        total_line_count += 1;
    }

    // pop the trailing line break
    tokens.pop();

    if let Some(h) = max_height {
        start = start.saturating_sub(h / 2);
        end = (start + h).min(total_line_count);
        if end - start < h {
            start = end.saturating_sub(h);
        }
    } else {
        // infinite height
        start = 0;
        end = total_line_count;
    }

    tokens.retain(|t| start <= t.visual_lineno && t.visual_lineno < end);
    Scrolled {
        total_line_count,
        range: start .. end,
        in_view: tokens,
    }
}

