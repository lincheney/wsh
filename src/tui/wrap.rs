use std::borrow::Cow;
use unicode_width::UnicodeWidthStr;
use bstr::{BStr, ByteSlice};
use ratatui::style::{Style, Color};
use super::text::{HighlightedRange, merge_highlights};

const ESCAPE_STYLE: Style = Style::new().fg(Color::Gray);

#[derive(Debug)]
pub enum WrapToken<'a> {
    String(Cow<'a, str>),
    LineBreak,
}

pub fn wrap_grapheme<
    'a,
    F: FnMut(usize, usize, WrapToken<'a>, Option<Style>)
>(
    start: usize,
    end: usize,
    grapheme: &'a str,
    line: &'a BStr,
    style: Option<Style>,
    max_width: usize,
    mut pos: usize,
    mut callback: F,
) -> usize {
    if grapheme == "\n" {
        // newline
        callback(start, end, WrapToken::LineBreak, None);
        0

    } else if grapheme == "\t" {
        let width = if pos >= max_width {
            pos = 0;
            callback(start, start, WrapToken::LineBreak, None);
            max_width
        } else {
            max_width - pos
        }.min(super::text::TAB_WIDTH);
        for _ in 0 .. width {
            callback(start, end, WrapToken::String(Cow::Borrowed(" ")), style);
        }
        pos + width

    } else if grapheme.width() > 0 && grapheme != "\u{FFFD}" {
        if pos + grapheme.width() > max_width {
            pos = 0;
            callback(start, start, WrapToken::LineBreak, None);
        }
        callback(start, end, WrapToken::String(Cow::Borrowed(grapheme)), style);
        pos + grapheme.width()

    } else {
        // invalid text
        let width = 2 + 4 + 1;
        let style = style.map(|s| s.patch(ESCAPE_STYLE));
        for (i, c) in line[start .. end].iter().enumerate() {
            if pos + width > max_width {
                pos = 0;
                callback(start, start, WrapToken::LineBreak, None);
            }
            let string = format!("<u{c:04x}>");
            debug_assert_eq!(string.width(), width);
            callback(start + i, start + i + 1, WrapToken::String(Cow::Owned(string)), style);
            pos += width;
        }
        pos
    }
}

pub fn wrap<
    'a,
    T: 'a,
    I: Clone + Iterator<Item=&'a HighlightedRange<T>>,
    F: FnMut(usize, usize, WrapToken<'a>, Option<Style>)
>(
    line: &'a BStr,
    highlights: I,
    init_style: Option<Style>,
    max_width: usize,
    initial_indent: usize,
    mut callback: F,
) {

    let mut pos = initial_indent;
    let mut style = init_style;

    let handle_virtual_text = |hl: &'a HighlightedRange<T>, start, mut pos, callback: &mut F| {
        if let Some(text) = &hl.inner.virtual_text {
            let style = init_style.map(|s| merge_highlights(s, [&hl.inner].into_iter()));
            let text = BStr::new(text.as_str());
            for (s, e, grapheme) in text.grapheme_indices() {
                pos = wrap_grapheme(s, e, grapheme, text, style, max_width, pos, |_, _, token, style| {
                    callback(start, start, token, style);
                });
            }
        }
        pos
    };

    for (start, end, grapheme) in line.grapheme_indices() {

        if highlights.clone().any(|h| h.start == start || h.end == start) {

            style = init_style.map(|s| {
                let highlights = highlights.clone()
                    .filter(|h| h.start <= start && start < h.end)
                    .map(|hl| &hl.inner);
                merge_highlights(s, highlights)
            });

            // virtual text
            for hl in highlights.clone() {
                if hl.start == start {
                    pos = handle_virtual_text(hl, start, pos, &mut callback);
                }
            }
        }

        pos = wrap_grapheme(start, end, grapheme, line, style, max_width, pos, &mut callback);
    }

    // virtual text
    for hl in highlights.clone() {
        if hl.start >= line.len() {
            pos = handle_virtual_text(hl, line.len(), pos, &mut callback);
        }
    }

}
