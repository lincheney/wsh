use std::ops::ControlFlow;
use std::range::Range;
use std::borrow::Cow;
use unicode_width::UnicodeWidthStr;
use bstr::{BStr, ByteSlice};
use super::style::{Style, Color, Modifier};
use super::text::{HighlightedRange, Highlight};

const ESCAPE_STYLE: Style = Style::new().fg(Color::AnsiValue(7));

pub type NoCallback<'a> = Option<fn(Range<usize>, WrapToken<'a>, usize, usize, Option<Style>) -> ControlFlow<()>>;

pub fn merge_highlights<'a, T: 'a, S: 'a, I: Iterator<Item=&'a Highlight<T, S>>>(init: Style, iter: I) -> Style {
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

pub fn merge_conceal<'a, T: 'a, S: 'a, I: Iterator<Item=&'a Highlight<T, S>>>(iter: I) -> bool {
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
            Self::String(s) => Some(s),
            Self::AsciiChar(c) => Some(std::str::from_utf8(c).unwrap()),
            Self::LineBreak => None,
        }
    }

    pub fn width(&self) -> usize {
        match self {
            Self::String(s) => s.width(),
            Self::AsciiChar(_) => 1,
            Self::LineBreak => 0,
        }
    }
}

macro_rules! define_wrap_ascii {
    ($name:ident $(, $callback:ident)?) => (

        fn $name $(<
            'a,
            #[allow(non_camel_case_types)]
            $callback: FnMut(Range<usize>, WrapToken<'a>, usize, usize, Option<Style>) -> ControlFlow<()>
        >)? (
            grapheme: u8,
            max_width: usize,
            (mut x, mut y, lineno): (usize, usize, usize),
            $(
            range: Range<usize>,
            style: Option<Style>,
            mut $callback: $callback,
            )?
        ) -> Result<(usize, usize, usize), ()> {

            if grapheme == b'\n' {
                // newline
                $($callback(range, WrapToken::LineBreak, y, lineno, None).continue_ok()?;)?
                Ok((0, y + 1, lineno + 1))

            } else if grapheme == b'\t' {
                let width = if x >= max_width {
                    $($callback((range.start..range.start).into(), WrapToken::LineBreak, y, lineno, None).continue_ok()?;)?
                    x = 0;
                    y += 1;
                    max_width
                } else {
                    max_width - x
                }.min(super::text::TAB_WIDTH);

                $(
                    for _ in 0 .. width {
                        $callback(range, WrapToken::AsciiChar([b' ']), y, lineno, style.clone()).continue_ok()?;
                    }
                )?
                Ok((x + width, y, lineno))

            } else {
                if x + 1 > max_width {
                    $($callback((range.start..range.start).into(), WrapToken::LineBreak, y, lineno, None).continue_ok()?;)?
                    x = 0;
                    y += 1;
                }
                $($callback(range, WrapToken::AsciiChar([grapheme]), y, lineno, style).continue_ok()?;)?
                Ok((x + 1, y, lineno))
            }
        }

    )
}

define_wrap_ascii!(wrap_ascii);
define_wrap_ascii!(wrap_ascii_with_callback, callback);

macro_rules! define_wrap_grapheme {
    ($name:ident, $wrap_ascii:ident $(, $callback:ident)?) => (

        fn $name <
            'a,
            $(
            #[allow(non_camel_case_types)]
            $callback: FnMut(Range<usize>, WrapToken<'a>, usize, usize, Option<Style>) -> ControlFlow<()>
            )?
        > (
            grapheme: &'a str,
            width: usize,
            max_width: usize,
            (mut x, mut y, lineno): (usize, usize, usize),
            line: &'a BStr,
            range: Range<usize>,
            $(
            style: Option<Style>,
            mut $callback: $callback,
            )?
        ) -> Result<(usize, usize, usize), ()> {

            if grapheme.len() == 1 {
                return $wrap_ascii(grapheme.as_bytes()[0], max_width, (x, y, lineno), $( range, style, $callback )?)
            }

            if width > 0 && (grapheme != "\u{FFFD}" || &line.as_bytes()[range] == grapheme.as_bytes()) {
                if x + grapheme.width() > max_width {
                    x = 0;
                    y += 1;
                    $($callback((range.start..range.start).into(), WrapToken::LineBreak, y, lineno, None).continue_ok()?;)?
                }
                $($callback(range, WrapToken::String(Cow::Borrowed(grapheme)), y, lineno, style).continue_ok()?;)?
                Ok((x + grapheme.width(), y, lineno))

            } else {
                // invalid text
                let width = 2 + 4 + 1;
                $(
                let _ = &$callback;
                let style = style.map(|s| s.patch(ESCAPE_STYLE));
                let mut i = 0;
                )?
                for c in line.as_bytes()[range].iter() {
                    if x + width > max_width {
                        x = 0;
                        y += 1;
                        $($callback((range.start..range.start).into(), WrapToken::LineBreak, y, lineno, None).continue_ok()?;)?
                    }
                    let string = format!("<u{c:04x}>");
                    debug_assert_eq!(string.width(), width);
                    $(
                    $callback((range.start + i .. range.start + i + 1).into(), WrapToken::String(Cow::Owned(string)), y, lineno, style.clone()).continue_ok()?;
                    i += 1;
                    )?
                    x += width;
                }
                Ok((x, y, lineno))
            }
        }
    )
}

define_wrap_grapheme!(wrap_grapheme, wrap_ascii);
define_wrap_grapheme!(wrap_grapheme_with_callback, wrap_ascii_with_callback, callback);

pub fn wrap<
    'a,
    T: 'a,
    S: 'a + AsRef<BStr>,
    I: Clone + Iterator<Item=&'a HighlightedRange<T, S>>,
    F: FnMut(Range<usize>, WrapToken<'a>, usize, usize, Option<Style>) -> ControlFlow<()>
>(
    paragraph: &'a BStr,
    highlights: I,
    init_style: Option<&Style>,
    max_width: usize,
    initial_indent: usize,
    callback: Option<F>,
) -> (usize, usize, usize) {
    let mut pos = (initial_indent, 0, 0);
    let _ = wrap_internal(&mut pos, paragraph, highlights, init_style, max_width, callback);
    pos
}

fn wrap_internal<
    'a,
    T: 'a,
    S: 'a + AsRef<BStr>,
    I: Clone + Iterator<Item=&'a HighlightedRange<T, S>>,
    F: FnMut(Range<usize>, WrapToken<'a>, usize, usize, Option<Style>) -> ControlFlow<()>
>(
    pos: &mut (usize, usize, usize),
    paragraph: &'a BStr,
    highlights: I,
    init_style: Option<&Style>,
    max_width: usize,
    mut callback: Option<F>,
) -> Result<(), ()> {
    // TODO performance is terrible if too many lines

    let all_ascii = paragraph.iter().all(|c| matches!(c, 0x20 ..= 0x7e | b'\n'));

    if all_ascii && callback.is_none() && highlights.clone().next().is_none() {
        // all ascii, no callback, no highlights
        // can't get any easier
        let mut last = paragraph.as_bytes();
        let mut lineno = 0;
        let y: usize = paragraph.split(|&c| c == b'\n').enumerate().map(|(i, l)| {
            last = l;
            let len = l.len() + if i == 0 { pos.0 } else { 0 };
            lineno = i;
            len.saturating_sub(1) / max_width + 1
        }).sum();
        let mut x = last.len() + if last.len() == paragraph.len() { pos.0 } else { 0 };
        if x > 0 {
            x = (x - 1) % max_width + 1;
        }

        *pos = (x, y - 1, lineno);
        return Ok(());
    }

    let mut style = init_style.cloned();

    let handle_virtual_text = |hl: &'a HighlightedRange<T, S>, start, mut pos, callback: &mut Option<&mut F>, init_style: Option<&Style>| {
        if let Some(text) = &hl.inner.virtual_text && !text.as_ref().is_empty() {
            if let Some(callback) = callback {
                let style = init_style.map(|s| merge_highlights(s.clone(), [&hl.inner].into_iter()));
                for (s, e, grapheme) in text.as_ref().grapheme_indices() {
                    pos = wrap_grapheme_with_callback(grapheme, grapheme.width(), max_width, pos, text.as_ref(), (s..e).into(), style.clone(), |_, token, wrapped_no, lineno, style| {
                        callback((start .. start).into(), token, wrapped_no, lineno, style)
                    })?;
                }
            } else {
                for (s, e, grapheme) in text.as_ref().grapheme_indices() {
                    pos = wrap_grapheme(grapheme, grapheme.width(), max_width, pos, text.as_ref(), (s..e).into())?;
                }
            }
        }
        Ok(pos)
    };

    let handle_highlights = |start: usize, end: usize, mut pos, mut style, mut callback: Option<&mut F>| {
        let mut conceal = false;

        if highlights.clone().any(|h| h.start == start || h.end == start) {

            style = init_style.map(|s| {
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
                    pos = handle_virtual_text(hl, x, pos, &mut callback, init_style)?;
                }
            }
        }

        Ok((pos, style, conceal))
    };

    if all_ascii {
        // most of the time it is ascii, to optimise for it

        if let Some(mut callback) = callback.as_mut() {
            for (i, &c) in paragraph.iter().enumerate() {
                let conceal;
                (*pos, style, conceal) = handle_highlights(i, i+1, *pos, style, Some(&mut callback))?;
                if !conceal {
                    *pos = wrap_ascii_with_callback(c, max_width, *pos, (i..i+1).into(), style.clone(), &mut callback)?;
                }
            }
        } else {
            for (i, &c) in paragraph.iter().enumerate() {
                let conceal;
                (*pos, style, conceal) = handle_highlights(i, i+1, *pos, style, None)?;
                if !conceal {
                    *pos = wrap_ascii(c, max_width, *pos)?;
                }
            }
        }

    } else {

        if let Some(mut callback) = callback.as_mut() {
            for (s, e, grapheme) in paragraph.grapheme_indices() {
                let conceal;
                (*pos, style, conceal) = handle_highlights(s, e, *pos, style, Some(&mut callback))?;
                if !conceal {
                    *pos = wrap_grapheme_with_callback(grapheme, grapheme.width(), max_width, *pos, paragraph, (s..e).into(), style.clone(), &mut callback)?;
                }
            }
        } else {
            for (s, e, grapheme) in paragraph.grapheme_indices() {
                let conceal;
                (*pos, style, conceal) = handle_highlights(s, e, *pos, style, callback.as_mut())?;
                if !conceal {
                    *pos = wrap_grapheme(grapheme, grapheme.width(), max_width, *pos, paragraph, (s..e).into())?;
                }
            }
        }

    }

    // virtual text
    for hl in highlights {
        if hl.start >= paragraph.len() {
            *pos = handle_virtual_text(hl, hl.start, *pos, &mut callback.as_mut(), init_style)?;
        }
    }

    Ok(())
}
