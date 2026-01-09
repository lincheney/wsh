use std::ops::Range;
use super::text::{HighlightedRange};
use ratatui::style::{Style};
use bstr::{BString};
use super::wrap::WrapToken;

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

impl Scrolled<'_> {
    pub fn lines(&self) -> impl Iterator<Item=&[ScrollWrapToken<'_>]> {
        let mut start = 0;
        self.in_view.iter()
            .enumerate()
            .filter(|(_, x)| matches!(x.inner, WrapToken::LineBreak))
            .map(|(i, _)| i)
            .chain(std::iter::once(self.in_view.len()))
            .map(move |i| {
                let slice = &self.in_view[start .. i];
                start = i + 1;
                slice
            })
    }
}

pub fn wrap<'a, T>(
    lines: &'a [BString],
    highlights: &'a [HighlightedRange<T>],
    init_style: Option<Style>,
    max_width: usize,
    max_height: usize,
    initial_indent: usize,
    scroll: ScrollPosition,
) -> Scrolled<'a> {

    let lineno = match scroll {
        ScrollPosition::Line(lineno) => lineno.min(lines.len().saturating_sub(1)),
        ScrollPosition::StickyBottom => lines.len().saturating_sub(1),
    };

    let mut total_line_count = 0;
    let mut tokens = vec![];
    let mut start = 0;
    // F: FnMut(usize, usize, WrapToken<'a>, Option<Style>) -> ControlFlow<()>
    for (i, line) in lines.iter().enumerate() {
        if i < lineno {
            start = total_line_count;
        }
        super::wrap::wrap(
            line.as_ref(),
            highlights.iter().filter(|hl| hl.lineno == lineno),
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

    start = start.saturating_sub(max_height / 2);
    let end = (start + max_height).min(total_line_count);
    if end - start < max_height {
        start = end.saturating_sub(max_height);
    }

    tokens.retain(|t| start <= t.visual_lineno && t.visual_lineno < end);
    Scrolled {
        total_line_count,
        range: start .. end,
        in_view: tokens,
    }
}

