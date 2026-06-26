use std::range::Range;
use std::borrow::Cow;
use unicode_width::UnicodeWidthStr;
use bstr::{BStr, ByteSlice};
use super::style::{Style, Color, Modifier};
use super::text::{HighlightedRange, Highlight};

const ESCAPE_STYLE: Style = Style::new().fg(Color::AnsiValue(7));

pub fn merge_highlights<'a, T: 'a, I: Iterator<Item=&'a Highlight<T>>>(init: Style, iter: I) -> Style {
    let mut style = init;
    for h in iter {
        if !h.blend {
            // start from scratch
            style = Style::new();
        }
        let reverse = style.has_modifier(Modifier::REVERSED);
        style = style.patch(h.style.clone());
        if reverse == h.style.has_modifier(Modifier::REVERSED) {
            style = style.remove_modifier(Modifier::REVERSED);
        } else {
            style = style.add_modifier(Modifier::REVERSED);
        }
    }
    style
}

pub fn merge_conceal<'a, T: 'a, I: Iterator<Item=&'a Highlight<T>>>(iter: I) -> bool {
    let mut conceal = false;
    for h in iter {
        if !h.blend {
            conceal = false;
        }
        conceal = h.conceal.unwrap_or(conceal);
    }
    conceal
}

#[derive(Debug)]
pub enum WrapToken<'a> {
    String(Cow<'a, str>),
    LineBreak,
}

pub fn wrap_grapheme<
    'a,
    F: FnMut(Range<usize>, WrapToken<'a>, Option<Style>)
>(
    range: Range<usize>,
    grapheme: &'a str,
    line: &'a BStr,
    style: Option<Style>,
    max_width: usize,
    mut pos: usize,
    mut callback: F,
) -> usize {
    if grapheme == "\n" {
        // newline
        callback(range, WrapToken::LineBreak, None);
        0

    } else if grapheme == "\t" {
        let width = if pos >= max_width {
            pos = 0;
            callback((range.start..range.start).into(), WrapToken::LineBreak, None);
            max_width
        } else {
            max_width - pos
        }.min(super::text::TAB_WIDTH);
        for _ in 0 .. width {
            callback(range, WrapToken::String(Cow::Borrowed(" ")), style.clone());
        }
        pos + width

    } else if grapheme.width() > 0 && (grapheme != "\u{FFFD}" || &line.as_bytes()[range] == grapheme.as_bytes()) {
        if pos + grapheme.width() > max_width {
            pos = 0;
            callback((range.start..range.start).into(), WrapToken::LineBreak, None);
        }
        callback(range, WrapToken::String(Cow::Borrowed(grapheme)), style);
        pos + grapheme.width()

    } else {
        // invalid text
        let width = 2 + 4 + 1;
        let style = style.map(|s| s.patch(ESCAPE_STYLE));
        for (i, c) in line.as_bytes()[range].iter().enumerate() {
            if pos + width > max_width {
                pos = 0;
                callback((range.start..range.start).into(), WrapToken::LineBreak, None);
            }
            let string = format!("<u{c:04x}>");
            debug_assert_eq!(string.width(), width);
            callback((range.start + i .. range.start + i + 1).into(), WrapToken::String(Cow::Owned(string)), style.clone());
            pos += width;
        }
        pos
    }
}

pub fn wrap<
    'a,
    T: 'a,
    I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    F: FnMut(Range<usize>, WrapToken<'a>, Option<Style>)
>(
    line: &'a BStr,
    highlights: I,
    init_style: Option<Style>,
    max_width: usize,
    initial_indent: usize,
    mut callback: F,
) {
    // TODO performance is terrible if too many lines

    let mut pos = initial_indent;
    let mut style = init_style.clone();

    let handle_virtual_text = |hl: &'a HighlightedRange<T>, start, mut pos, callback: &mut F, init_style: &Option<Style>| {
        if let Some(text) = &hl.inner.virtual_text {
            let style = init_style.as_ref().map(|s| merge_highlights(s.clone(), [&hl.inner].into_iter()));
            for (s, e, grapheme) in text.grapheme_indices() {
                pos = wrap_grapheme((s..e).into(), grapheme, text.as_ref(), style.clone(), max_width, pos, |_, token, style| {
                    callback((start..start).into(), token, style);
                });
            }
        }
        pos
    };

    for (start, end, grapheme) in line.grapheme_indices() {
        let mut conceal = false;

        if highlights.clone().any(|h| h.start == start || h.end == start) {

            style = init_style.as_ref().map(|s| {
                let highlights = highlights.clone()
                    .filter(|h| h.start <= start && start < h.end)
                    .map(|hl| &hl.inner);
                merge_highlights(s.clone(), highlights)
            });

            conceal = merge_conceal(
                highlights.clone()
                    .filter(|h| h.start <= start && start < h.end)
                    .map(|hl| &hl.inner)
            );

            // virtual text
            // use the end pos if concealed
            // so that at least buffer will place the cursor on the
            // end of the virt text
            let x = if conceal { end } else { start };
            for hl in highlights.clone() {
                if hl.start == start {
                    pos = handle_virtual_text(hl, x, pos, &mut callback, &init_style);
                }
            }
        }

        if !conceal {
            pos = wrap_grapheme((start..end).into(), grapheme, line, style.clone(), max_width, pos, &mut callback);
        }
    }

    // virtual text
    for hl in highlights {
        if hl.start >= line.len() {
            pos = handle_virtual_text(hl, hl.start, pos, &mut callback, &init_style);
        }
    }

}
