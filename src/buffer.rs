use std::ops::Range;
use bstr::{BString, ByteSlice, BStr};
use anyhow::Result;
use unicode_width::UnicodeWidthStr;
use crossterm::{
    queue,
    terminal::{Clear, ClearType},
    cursor,
    style::{ContentStyle, Attributes, Stylize},
};

#[derive(Debug)]
pub struct Highlight {
    pub start: usize,
    pub end: usize,
    pub style: ContentStyle,
    pub attribute_mask: Attributes,
    pub namespace: usize,
}

impl Highlight {
    fn shift(&mut self, range: Range<usize>, new_end: usize) {
        if range.end <= self.start {
            self.start = self.start + new_end - range.end;
        } else if range.start <= self.start {
            self.start = new_end;
        }

        if range.end <= self.end {
            self.end = self.end + new_end - range.end;
        } else if range.start <= self.end {
            self.end = new_end;
        }

        self.start = self.start.min(self.end);
    }

    fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

struct HighlightStack<'a>(Vec<&'a Highlight>);

impl<'a> HighlightStack<'a> {
    fn merge(&self) -> String {
        let mut style = ContentStyle::new();
        for h in self.0.iter() {
            if let Some(fg) = h.style.foreground_color {
                style = style.with(fg);
            }
            if let Some(bg) = h.style.background_color {
                style = style.with(bg);
            }
            if let Some(ul) = h.style.underline_color {
                style = style.underline(ul);
            }
            style.attributes = (style.attributes & (style.attributes ^ h.attribute_mask)) | (h.style.attributes & h.attribute_mask);
        }
        format!("{}", style.apply(" "))
    }
}

#[derive(Debug, Default)]
pub struct Buffer {
    contents: BString,
    // display: String,
    len: Option<usize>,
    cursor: usize,

    saved_contents: BString,
    saved_cursor: usize,

    pub dirty: bool,

    pub height: usize,
    pub y_offset: usize,

    pub highlights: Vec<Highlight>,
    pub highlight_counter: usize,
}

struct BufferContents<'a> {
    inner: &'a BStr,
    highlights: &'a Vec<Highlight>,
    offset: usize,
}

impl std::fmt::Display for BufferContents<'_> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {

        let mut escaped = false;

        let start_escape = |fmt: &mut std::fmt::Formatter, escaped: &mut bool| {
            if !*escaped {
                write!(fmt, "\x1b[31m")?;
                *escaped = true;
            }
            Ok(())
        };

        let end_escape = |fmt: &mut std::fmt::Formatter, escaped: &mut bool| {
            if *escaped {
                write!(fmt, "\x1b[0m")?;
                *escaped = false;
            }
            Ok(())
        };

        let mut stack = HighlightStack(vec![]);

        for (i, (start, end, c)) in self.inner.grapheme_indices().enumerate() {
            let mut changed = false;
            for h in self.highlights.iter() {
                if h.start == i + self.offset {
                    changed = true;
                    stack.0.push(h);
                }
            }
            if changed {
                fmt.write_str("\x1b[0m")?;
                fmt.write_str(stack.merge().split_once(' ').unwrap().0)?;
            }

            if start + 1 == end && c == "\u{FFFD}" {
                // invalid
                start_escape(fmt, &mut escaped)?;
                write!(fmt, "<{:02x}>", self.inner[start])?;
            } else if c.width() > 0 || c == "\n" {
                end_escape(fmt, &mut escaped)?;
                fmt.write_str(c)?;
            } else {
                // invalid
                start_escape(fmt, &mut escaped)?;
                for c in self.inner[start..end].iter() {
                    write!(fmt, "<u{:04x}>", c)?;
                }
            }

            let mut changed = false;
            stack.0.retain(|h| {
                if h.end <= i + self.offset + 1 {
                    changed = true;
                    false
                } else {
                    true
                }
            });
            if changed {
                fmt.write_str("\x1b[0m")?;
                fmt.write_str(stack.merge().split_once(' ').unwrap().0)?;
            }
        }
        end_escape(fmt, &mut escaped)?;

        if !stack.0.is_empty() {
            fmt.write_str("\x1b[0m")?;
        }

        Ok(())
    }
}

fn strip_colours(string: &mut String) {
    if string.contains("\x1b") {
        let mut in_esc = false;
        string.retain(|c| {
            if in_esc && c == 'm' {
                in_esc = false;
            } else if c == '\x1b' {
                in_esc = true;
            }

            !in_esc
        });
    }
}

fn wrap(string: &str, width: usize) -> Vec<std::borrow::Cow<str>> {
    // no word splitting
    let options = textwrap::Options::new(width)
        .word_separator(textwrap::WordSeparator::Custom(|line| {
            Box::new(std::iter::once(textwrap::core::Word::from(line)))
        }));
    textwrap::wrap(string, options)
}

impl Buffer {

    fn get_len(&mut self) -> usize {
        *self.len.get_or_insert_with(|| self.contents.graphemes().count())
    }

    fn fix_cursor(&mut self) {
        if self.cursor > self.get_len() {
            self.cursor = self.get_len();
        }
        self.dirty = true;
    }

    pub fn get_contents(&self) -> &BString {
        &self.contents
    }

    pub fn get_cursor(&self) -> usize {
        self.cursor
    }

    pub fn set(&mut self, contents: Option<&[u8]>, cursor: Option<usize>) {
        if let Some(contents) = contents {
            self.contents.resize(contents.len(), 0);
            self.contents.copy_from_slice(contents);
            // all highlights are now invalid!
            self.highlights.clear();
            self.len = None;
        }
        if let Some(cursor) = cursor {
            self.cursor = cursor;
        }
        self.fix_cursor();
    }

    pub fn set_contents(&mut self, contents: &[u8]) {
        self.set(Some(contents), None);
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.set(None, Some(cursor));
    }

    pub fn splice_at_cursor(&mut self, data: &[u8], replace_len: Option<usize>) {
        let start = self.cursor_byte_pos();
        let end = if let Some(replace_len) = replace_len {
            self.byte_pos(self.cursor + replace_len)
        } else {
            self.contents.len()
        };
        self.contents.splice(start .. end, data.iter().copied());
        self.len = None;

        self.highlights.retain_mut(|hl| {
            hl.shift(start .. end, start + data.len());
            !hl.is_empty()
        });

        // calculate the new cursor
        let end = start + data.len();
        self.cursor = self.contents.grapheme_indices().take_while(|(s, _, _)| *s < end).count();
        self.fix_cursor();
    }

    pub fn insert_at_cursor(&mut self, data: &[u8]) {
        self.splice_at_cursor(data, Some(0));
    }

    pub fn save(&mut self) {
        self.saved_contents.resize(self.contents.len(), 0);
        self.saved_contents.copy_from_slice(&self.contents);
        self.saved_cursor = self.cursor;
    }

    pub fn restore(&mut self) {
        std::mem::swap(&mut self.contents, &mut self.saved_contents);
        std::mem::swap(&mut self.cursor, &mut self.saved_cursor);
        self.len = None;
        self.fix_cursor();
    }

    pub fn reset(&mut self) {
        self.contents.clear();
        self.len = None;
        self.cursor = 0;
        self.y_offset = 0;
        self.saved_contents.clear();
        self.saved_cursor = 0;
        self.dirty = true;
        self.height = 0;
    }

    fn byte_pos(&self, pos: usize) -> usize {
        self.contents
            .grapheme_indices()
            .nth(pos)
            .map(|(s, _, _)| s)
            .unwrap_or_else(|| self.contents.len())
    }

    pub fn cursor_byte_pos(&self) -> usize {
        self.byte_pos(self.cursor)
    }

    pub fn draw(
        &mut self,
        stdout: &mut std::io::Stdout,
        (width, _height): (u16, u16),
        prompt_width: usize,
    ) -> Result<bool> {

        let old = self.height;

        let byte_pos = self.cursor_byte_pos();
        let mut prefix = format!("{}", BufferContents{
            inner: self.contents[..byte_pos].into(),
            highlights: &mut self.highlights,
            offset: 0,
        });
        let suffix = format!("{}", BufferContents{
            inner: self.contents[byte_pos..].into(),
            highlights: &mut self.highlights,
            offset: self.cursor,
        });
        // add an extra space for the cursor
        if !suffix.is_empty() {
            prefix += " ";
        }

        // calculate heights
        let mut text = " ".repeat(prompt_width) + &prefix;
        strip_colours(&mut text);
        self.y_offset = wrap(&text, width as _).len() - 1;

        let height = if suffix.is_empty() {
            self.y_offset + 1
        } else {
            // pop the space from the end
            text.pop();
            text += &suffix;
            strip_colours(&mut text);
            wrap(&text, width as _).len()
        };

        self.height = self.height.max(height);
        let changed = old != self.height;

        queue!(stdout, cursor::MoveToColumn(prompt_width as _))?;
        if changed {
            queue!(stdout, Clear(ClearType::FromCursorDown))?;
        }
        queue!(stdout, crossterm::style::Print(&prefix))?;
        if !suffix.is_empty() {
            // move back over the space
            queue!(stdout, cursor::MoveLeft(1))?;
        }
        queue!(
            stdout,
            cursor::SavePosition,
            crossterm::style::Print(&suffix),
            Clear(ClearType::UntilNewLine),
        )?;
        if self.height > height {
            for _ in height .. self.height {
                queue!(
                    stdout,
                    cursor::MoveDown(1),
                    cursor::MoveToColumn(0),
                    Clear(ClearType::UntilNewLine),
                )?;
            }
        }
        queue!(stdout, cursor::RestorePosition)?;


        self.dirty = false;
        Ok(changed)
    }

}
