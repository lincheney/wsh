use std::cmp::Ordering;
use std::range::Range;
use super::text::{HighlightedRange};
use super::style::Style;
use bstr::{BString, BStr};
use super::wrap::WrapToken;

#[derive(Debug, Clone, Copy, Default ,PartialEq)]
pub enum ScrollPosition {
    Line(usize),
    #[default]
    StickyBottom,
}

impl ScrollPosition {
    pub fn get_approx_line_range(&self, max_height: Option<usize>, len: usize) -> (usize, std::ops::Range<usize>) {
        let lineno = match *self {
            ScrollPosition::Line(lineno) => lineno.min(len.saturating_sub(1)),
            ScrollPosition::StickyBottom => len.saturating_sub(1),
        };

        let min_lineno = max_height.map_or(0, |h| lineno.saturating_sub(h));
        let max_lineno = max_height.map_or(len, |h| lineno + h);
        (lineno, min_lineno .. max_lineno + 1)
    }
}

#[derive(Debug)]
pub struct ScrollWrapToken<'a> {
    pub lineno: usize,
    visual_lineno: usize,
    pub range: Range<usize>,
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

pub fn wrap<'a, T, I, F>(
    lines: &'a [BString],
    init_style: Option<Style>,
    max_width: usize,
    max_height: Option<usize>,
    mut initial_indent: usize,
    scroll: ScrollPosition,
    highlight_getter: F,
) -> Scrolled<'a>
where
    T: 'a,
    I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    F: Fn(usize) -> I,
{

    let (lineno, line_range) = scroll.get_approx_line_range(max_height, lines.len());

    let mut total_line_count = 0;
    let mut tokens = vec![];
    // let mut start = 0;

    // add a dummy line at the end
    for (i, line) in lines.iter().map(|l| l.as_ref()).chain(std::iter::once(BStr::new(b""))).enumerate() {

        let past_end = i >= lines.len();

        let highlights = highlight_getter(i);
        // start = end;

        if past_end && !highlights.clone().any(|hl| hl.inner.has_virtual_text()) {
            continue
        }

        let may_be_in_range = line_range.contains(&i);

        let (_, y) = if may_be_in_range {
            super::wrap::wrap(
                line,
                highlights,
                init_style.clone(),
                max_width,
                initial_indent,
                Some(|range, token, lineno, style| {
                    tokens.push(ScrollWrapToken {
                        lineno: i,
                        visual_lineno: total_line_count + lineno,
                        range,
                        inner: token,
                        style,
                    });
                }),
            )
        } else {
            super::wrap::wrap(
                line,
                highlights.clone().filter(|hl| hl.inner.may_cause_resize()),
                init_style.clone(),
                max_width,
                initial_indent,
                // don't bother with the callback for lines out of range
                super::wrap::NoCallback::None,
            )
        };

        if may_be_in_range {
            tokens.push(ScrollWrapToken {
                lineno: i,
                visual_lineno: tokens.last().map_or(total_line_count, |t| t.visual_lineno),
                range: (line.len() .. line.len()).into(),
                inner: WrapToken::LineBreak,
                style: None,
            });
        }
        total_line_count += y + 1;
        initial_indent = 0;
    }

    // pop the trailing line break
    tokens.pop();

    let max_visual = match tokens.last() {
        // fit one more line if the last one is a linebreak
        Some(ScrollWrapToken{ visual_lineno, inner: WrapToken::LineBreak, .. }) => visual_lineno + 2,
        Some(ScrollWrapToken{ visual_lineno, .. }) => visual_lineno + 1,
        _ => 1,
    };

    let (start, end) = if !tokens.is_empty() && let Some(max_height) = max_height {

        let start = tokens.partition_point(|t| t.lineno < lineno);
        let end = start + tokens[start..].partition_point(|t| t.lineno <= lineno);
        let start = tokens.get(start).or(tokens.last()).unwrap().visual_lineno;
        let end = tokens.get(end.saturating_sub(1)).or(tokens.last()).unwrap().visual_lineno + 1;
        let current_height = end - start;

        match max_height.cmp(&current_height) {
            Ordering::Equal => (start, end),
            Ordering::Less if matches!(scroll, ScrollPosition::StickyBottom) => {
                // truncate the start
                let start = (start + (current_height - max_height)).min(max_visual);
                (start, end)
            },
            Ordering::Less => {
                // truncate the end
                let end = (end - (current_height - max_height)).min(max_visual);
                (start, end)
            },
            Ordering::Greater => {
                // can fit more lines
                // prefer to add space on the bottom first?
                let end = (end + (max_height - current_height) / 2).min(max_visual);
                let start = end.saturating_sub(max_height);
                let end = start + max_height;
                (start, end)
            },
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
        range: (start .. end).into(),
        in_view: tokens,
    }
}

