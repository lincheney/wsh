use std::ops::ControlFlow;
use std::cmp::Ordering;
use std::range::Range;
use super::text::{HighlightedRange};
use super::style::Style;
use bstr::{BString, BStr};
use super::wrap::WrapToken;

#[derive(Debug, Clone, Copy, Default ,PartialEq)]
pub enum ScrollPosition {
    Line(usize),
    Paragraph(usize),
    #[default]
    StickyBottom,
}

#[derive(Debug)]
pub struct ScrollPositionRange {
    pub parano: usize,
    pub para_range: std::ops::Range<usize>,
}

impl ScrollPosition {
    pub fn get_approx_range(&self, max_height: Option<usize>, paragraphs: &[BString]) -> ScrollPositionRange {
        let parano = match *self {
            ScrollPosition::Paragraph(parano) if parano < paragraphs.len() => parano,
            ScrollPosition::Line(lineno) if let Some((parano, _)) = paragraphs
                .iter()
                .enumerate()
                .scan(0, |sum, (i, p)| { *sum += p.split(|&c| c == b'\n').count() + 1; Some((i, *sum)) })
                .find(|(_, sum)| *sum >= lineno)
            => parano,
            // sticky bottom
            _ => paragraphs.len().saturating_sub(1),
        };

        let min = max_height.map_or(0, |h| parano.saturating_sub(h));
        let max = max_height.map_or(paragraphs.len(), |h| parano + h);
        ScrollPositionRange {
            parano,
            para_range: min .. max + 1,
        }
    }
}

#[derive(Debug)]
pub struct ScrollWrapToken<'a> {
    pub parano: usize,
    lineno: usize,
    visual_lineno: usize,
    pub range: Range<usize>,
    pub inner: WrapToken<'a>,
    pub style: Option<Style>,
}

#[derive(Debug)]
pub struct Scrolled<'a> {
    pub total_visual_line_count: usize,
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

    pub fn first_lineno(&self) -> Option<usize> {
        self.inner.get(self.start).map(|t| t.lineno)
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

pub fn wrap<'a, T, S, I, F>(
    paragraphs: &'a [BString],
    init_style: Option<&Style>,
    max_width: usize,
    max_height: Option<usize>,
    mut initial_indent: usize,
    scroll: ScrollPosition,
    highlight_getter: F,
) -> Scrolled<'a>
where
    T: 'a,
    S: 'a + AsRef<BStr>,
    I: Clone + Iterator<Item=&'a HighlightedRange<T, S>>,
    F: Fn(usize) -> I,
{

    let approx_range = scroll.get_approx_range(max_height, paragraphs);

    let mut total_line_count = 0;
    let mut total_visual_line_count = 0;
    let mut tokens = vec![];
    // let mut start = 0;

    // add a dummy line at the end
    for (i, paragraph) in paragraphs.iter().map(|l| l.as_ref()).chain(std::iter::once(BStr::new(b""))).enumerate() {

        let past_end = i >= paragraphs.len();

        let highlights = highlight_getter(i);
        // start = end;

        if past_end && !highlights.clone().any(|hl| hl.inner.has_virtual_text()) {
            continue
        }

        let may_be_in_range = approx_range.para_range.contains(&i);

        let (_, y, lineno) = if may_be_in_range {
            super::wrap::wrap(
                paragraph,
                highlights,
                init_style,
                max_width,
                initial_indent,
                Some(|range, token, wrapped_no, lineno, style| {
                    tokens.push(ScrollWrapToken {
                        parano: i,
                        lineno: total_line_count + lineno,
                        visual_lineno: total_visual_line_count + wrapped_no,
                        range,
                        inner: token,
                        style,
                    });
                    ControlFlow::Continue(())
                }),
            )
        } else {
            super::wrap::wrap(
                paragraph,
                highlights.clone().filter(|hl| hl.inner.may_cause_resize()),
                init_style,
                max_width,
                initial_indent,
                // don't bother with the callback for paragraphs out of range
                super::wrap::NoCallback::None,
            )
        };

        if may_be_in_range {
            tokens.push(ScrollWrapToken {
                parano: i,
                lineno: tokens.last().map_or(total_line_count, |t| t.lineno),
                visual_lineno: tokens.last().map_or(total_visual_line_count, |t| t.visual_lineno),
                range: (paragraph.len() .. paragraph.len()).into(),
                inner: WrapToken::LineBreak,
                style: None,
            });
        }
        total_line_count += lineno + 1;
        total_visual_line_count += y + 1;
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

        let (start, end) = if let ScrollPosition::Line(lineno) = scroll {
            let start = tokens.partition_point(|t| t.lineno < lineno);
            let end = start + tokens[start..].partition_point(|t| t.lineno <= lineno);
            (start, end)
        } else {
            let start = tokens.partition_point(|t| t.parano < approx_range.parano);
            let end = start + tokens[start..].partition_point(|t| t.parano <= approx_range.parano);
            (start, end)
        };

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
                // can fit more paragraphs
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
        total_visual_line_count,
        range: (start .. end).into(),
        in_view: tokens,
    }
}

