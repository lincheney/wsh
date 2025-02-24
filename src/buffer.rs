use bstr::{BString, ByteSlice, BStr};
use anyhow::Result;
use unicode_width::UnicodeWidthStr;
use crossterm::{
    queue,
    terminal::{Clear, ClearType},
    cursor,
};

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
}

struct BufferContents<'a>(&'a BStr);

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

        for (start, end, c) in self.0.grapheme_indices() {
            if start + 1 == end && c == "\u{FFFD}" {
                // invalid
                start_escape(fmt, &mut escaped)?;
                write!(fmt, "<{:02x}>", self.0[start])?;
            } else if c.width() > 0 || c == "\n" {
                end_escape(fmt, &mut escaped)?;
                fmt.write_str(c)?;
            } else {
                // invalid
                start_escape(fmt, &mut escaped)?;
                for c in self.0[start..end].iter() {
                    write!(fmt, "<u{:04x}>", c)?;
                }
            }
        }
        end_escape(fmt, &mut escaped)?;

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

            in_esc
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

    pub fn mutate<F: FnOnce(&mut BString, &mut usize)->R, R>(&mut self, func: F) -> R {
        let value = func(&mut self.contents, &mut self.cursor);
        self.len = None;
        self.fix_cursor();
        value
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

    pub fn insert(&mut self, data: &[u8]) {
        let byte_pos = self.cursor_byte_pos();
        self.contents.splice(byte_pos .. byte_pos, data.iter().copied());
        self.len = None;

        // calculate the new cursor
        let byte_pos = byte_pos + data.len();
        self.cursor = self.contents.grapheme_indices().take_while(|(s, _, _)| *s < byte_pos).count();
        self.fix_cursor();
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
    }

    pub fn cursor_byte_pos(&mut self) -> usize {
        self.contents
            .grapheme_indices()
            .nth(self.cursor)
            .map(|(s, _, _)| s)
            .unwrap_or_else(|| self.contents.len())
    }

    pub fn draw(
        &mut self,
        stdout: &mut std::io::Stdout,
        (width, _height): (u16, u16),
        prompt_width: usize,
    ) -> Result<bool> {

        let old = self.height;

        let byte_pos = self.cursor_byte_pos();
        let mut prefix = format!("{}", BufferContents(self.contents[..byte_pos].into()));
        let suffix = format!("{}", BufferContents(self.contents[byte_pos..].into()));
        // add an extra space for the cursor
        if !suffix.is_empty() {
            prefix += " ";
        }

        // calculate heights
        let mut text = " ".repeat(prompt_width) + &prefix;
        strip_colours(&mut text);
        self.y_offset = wrap(&text, width as _).len() - 1;

        if suffix.is_empty() {
            self.height = self.y_offset + 1;
        } else {
            // pop the space from the end
            text.pop();
            text += &suffix;
            strip_colours(&mut text);
            self.height = wrap(&text, width as _).len();
        }
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
            cursor::RestorePosition,
        )?;

        self.dirty = false;
        Ok(changed)
    }

}
