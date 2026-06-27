use std::range::Range;
use std::borrow::Cow;
use unicode_width::UnicodeWidthStr;
use bstr::{BStr, ByteSlice};
use super::style::{Style, Color, Modifier};
use super::text::{HighlightedRange, Highlight};

const ESCAPE_STYLE: Style = Style::new().fg(Color::AnsiValue(7));

pub type NoCallback<'a> = Option<fn(Range<usize>, WrapToken<'a>, Option<Style>)>;

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
    AsciiChar([u8; 1]),
    LineBreak,
}

impl WrapToken<'_> {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(&s),
            Self::AsciiChar(c) => Some(std::str::from_utf8(c).unwrap()),
            _ => None,
        }
    }

    pub fn width(&self) -> usize {
        match self {
            Self::String(s) => s.width(),
            Self::AsciiChar(_) => 1,
            _ => 0,
        }
    }
}

fn wrap_ascii<
    'a,
    F: FnMut(Range<usize>, WrapToken<'a>, Option<Style>)
>(
    range: Range<usize>,
    grapheme: u8,
    style: Option<Style>,
    max_width: usize,
    (mut x, mut y): (usize, usize),
    mut callback: Option<F>,
) -> (usize, usize) {

    if grapheme == b'\n' {
        // newline
        callback.as_mut().map(|c| c(range, WrapToken::LineBreak, None));
        (0, y + 1)

    } else if grapheme == b'\t' {
        let width = if x >= max_width {
            x = 0;
            y += 1;
            callback.as_mut().map(|c| c((range.start..range.start).into(), WrapToken::LineBreak, None));
            max_width
        } else {
            max_width - x
        }.min(super::text::TAB_WIDTH);

        callback.as_mut().map(|c| {
            for _ in 0 .. width {
                c(range, WrapToken::AsciiChar([b' ']), style.clone())
            }
        });
        (x + width, y)

    } else {
        if x + 1 > max_width {
            x = 0;
            y += 1;
            callback.as_mut().map(|c| c((range.start..range.start).into(), WrapToken::LineBreak, None));
        }
        callback.as_mut().map(|c| c(range, WrapToken::AsciiChar([grapheme]), style));
        (x + 1, y)
    }
}


pub fn wrap_grapheme<
    'a,
    F: FnMut(Range<usize>, WrapToken<'a>, Option<Style>)
>(
    range: Range<usize>,
    grapheme: &'a str,
    width: usize,
    line: &'a BStr,
    style: Option<Style>,
    max_width: usize,
    (mut x, mut y): (usize, usize),
    mut callback: Option<F>,
) -> (usize, usize) {

    if grapheme.len() == 1 {
        return wrap_ascii(range, grapheme.as_bytes()[0], style, max_width, (x, y), callback)
    }

    if width > 0 && (grapheme != "\u{FFFD}" || &line.as_bytes()[range] == grapheme.as_bytes()) {
        if x + grapheme.width() > max_width {
            x = 0;
            y += 1;
            callback.as_mut().map(|c| c((range.start..range.start).into(), WrapToken::LineBreak, None));
        }
        callback.as_mut().map(|c| c(range, WrapToken::String(Cow::Borrowed(grapheme)), style));
        (x + grapheme.width(), y)

    } else {
        // invalid text
        let width = 2 + 4 + 1;
        let style = style.map(|s| s.patch(ESCAPE_STYLE));
        for (i, c) in line.as_bytes()[range].iter().enumerate() {
            if x + width > max_width {
                x = 0;
                y += 1;
                callback.as_mut().map(|c| c((range.start..range.start).into(), WrapToken::LineBreak, None));
            }
            let string = format!("<u{c:04x}>");
            debug_assert_eq!(string.width(), width);
            callback.as_mut().map(|c| c((range.start + i .. range.start + i + 1).into(), WrapToken::String(Cow::Owned(string)), style.clone()));
            x += width;
        }
        (x, y)
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
    mut callback: Option<F>,
) -> (usize, usize) {
    // TODO performance is terrible if too many lines

    let mut pos = (initial_indent, 0);
    let mut style = init_style.clone();

    let handle_virtual_text = |hl: &'a HighlightedRange<T>, start, mut pos, callback: &mut Option<&mut F>, init_style: &Option<Style>| {
        if let Some(text) = &hl.inner.virtual_text && !text.is_empty() {
            let style = init_style.as_ref().map(|s| merge_highlights(s.clone(), [&hl.inner].into_iter()));
            for (s, e, grapheme) in text.grapheme_indices() {
                let callback = callback.as_mut().map(|c| {
                    |_, token, style| {
                        c((start..start).into(), token, style);
                    }
                });

                pos = wrap_grapheme((s..e).into(), grapheme, grapheme.width(), text.as_ref(), style.clone(), max_width, pos, callback);
            }
        }
        pos
    };

    let handle_highlights = |start: usize, end: usize, mut pos, mut style, mut callback: Option<&mut F>| {
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

        (pos, style, conceal)
    };

    if line.iter().all(|c| matches!(c, 0x20 ..= 0x7e | b'\n')) {
        // most of the time it is ascii, to optimise for it
        for (i, &c) in line.iter().enumerate() {
            let conceal;
            (pos, style, conceal) = handle_highlights(i, i+1, pos, style, callback.as_mut());
            if !conceal {
                pos = wrap_ascii((i..i+1).into(), c, style.clone(), max_width, pos, callback.as_mut());
            }
        }
    } else {
        for (start, end, grapheme) in line.grapheme_indices() {
            let conceal;
            (pos, style, conceal) = handle_highlights(start, end, pos, style, callback.as_mut());
            if !conceal {
                pos = wrap_grapheme((start..end).into(), grapheme, grapheme.width(), line, style.clone(), max_width, pos, callback.as_mut());
            }
        }
    }

    // virtual text
    for hl in highlights {
        if hl.start >= line.len() {
            pos = handle_virtual_text(hl, hl.start, pos, &mut callback.as_mut(), &init_style);
        }
    }

    pos
}
