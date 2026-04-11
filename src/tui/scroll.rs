use std::ops::Range;
use super::text::{HighlightedRange};
use ratatui::style::{Style};
use bstr::{BString, BStr};
use super::wrap::WrapToken;

#[derive(Debug, Clone, Copy, Default ,PartialEq)]
pub enum ScrollPosition {
    Line(usize),
    #[default]
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

#[derive(Debug)]
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

    // add a dummy line at the end
    for (i, line) in lines.iter().map(|l| l.as_ref()).chain(std::iter::once(BStr::new(b""))).enumerate() {

        let past_end = i >= lines.len();
        if past_end && !highlights.clone().any(|hl| hl.lineno >= i && hl.inner.virtual_text.is_some()) {
            continue
        }

        super::wrap::wrap(
            line,
            highlights.clone().filter(|hl| hl.lineno == i || (past_end && hl.lineno > i)),
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

    let max_visual = tokens.last().map_or(0, |t| t.visual_lineno) + 1;

    let (start, end) = if !tokens.is_empty() && let Some(max_height) = max_height {

        let start = tokens.partition_point(|t| t.lineno < lineno);
        let end = start + tokens[start..].partition_point(|t| t.lineno <= lineno);
        let start = tokens.get(start).or(tokens.last()).unwrap().visual_lineno;
        let end = tokens.get(end.saturating_sub(1)).or(tokens.last()).unwrap().visual_lineno + 1;
        let current_height = end - start;

        let space = max_height.saturating_sub(current_height);
        if space > 0 {
            // can fit more lines
            // prefer to add space on the bottom first?
            let end = (end + space / 2).min(max_visual);
            let start = end.saturating_sub(max_height);
            let end = start + max_height;
            (start, end)
        } else {
            (start, end)
        }

    } else {
        // full height
        (0, max_visual)
    };

    let partition_start = tokens.partition_point(|t| t.visual_lineno < start);
    drop(tokens.drain(..partition_start));
    let partition_end = tokens.partition_point(|t| {
        t.visual_lineno + 1 < end || (t.visual_lineno < end && !matches!(&t.inner, WrapToken::LineBreak))
    });
    drop(tokens.drain(partition_end..));

    Scrolled {
        total_line_count,
        range: start .. end,
        in_view: tokens,
    }
}

