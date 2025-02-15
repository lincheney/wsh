use std::io::Write;
use std::fmt::{Write as FmtWrite};
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
    len: usize,
    cursor: usize,
    pub dirty: bool,

    pub height: usize,
    pub cursory: usize,
}

struct BufferContents<'a>(&'a BStr);

fn wrap(string: &str, width: usize) -> Vec<std::borrow::Cow<str>> {
    // no word splitting
    let options = textwrap::Options::new(width)
        .word_separator(textwrap::WordSeparator::Custom(|line| {
            Box::new(std::iter::once(textwrap::core::Word::from(line)))
        }));
    textwrap::wrap(string, options)
}

impl Buffer {

    fn refresh_len(&mut self) {
        self.len = self.contents.graphemes().count();
    }

    fn fix_cursor(&mut self) {
        if self.cursor > self.len {
            self.cursor = self.len;
        }
        self.dirty = true;
    }

    pub fn mutate<F: FnOnce(&mut BString, &mut usize, usize)->R, R>(&mut self, func: F) -> R {
        let byte_pos = self.cursor_byte_pos();
        let value = func(&mut self.contents, &mut self.cursor, byte_pos);
        self.refresh_len();
        self.fix_cursor();
        value
    }

    pub fn get_contents(&self) -> &BString {
        &self.contents
    }

    pub fn get_cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_contents(&mut self, contents: BString) {
        self.contents = contents;
        self.refresh_len();
        self.fix_cursor();
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
        self.fix_cursor();
    }

    pub fn reset(&mut self) {
        self.contents.clear();
        self.len = 0;
        self.cursor = 0;
        self.cursory = 0;
        self.dirty = true;
    }

    pub fn cursor_byte_pos(&self) -> usize {
        self.contents.grapheme_indices().skip(self.cursor).next().map(|(s, _, _)| s).unwrap_or(self.len)
    }

    pub fn needs_redraw(&self) -> bool {
        self.dirty
    }

    pub fn draw(
        &mut self,
        stdout: &mut std::io::Stdout,
        (width, height): (u16, u16),
        offset: usize,
    ) -> Result<()> {
        let byte_pos = self.cursor_byte_pos();
        let prefix = format!("{}", BufferContents(self.contents[..byte_pos].into()));
        let suffix = format!("{}", BufferContents(self.contents[byte_pos..].into()));

        queue!(
            stdout,
            crossterm::style::Print(&prefix),
            cursor::SavePosition,
            crossterm::style::Print(&suffix),
            Clear(ClearType::UntilNewLine),
            cursor::RestorePosition,
        )?;

        // the offset represents the prompt width
        let prefix = " ".repeat(offset) + &prefix;

        let prefix = prefix + &suffix[0 .. suffix.len().min(1)];
        self.cursory = wrap(&prefix, width as _).len() - 1;

        let prefix = prefix + &suffix[suffix.len().min(1) ..];
        self.height = wrap(&prefix, width as _).len();

        self.dirty = false;
        Ok(())
    }

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

        for (start, end, c) in self.0.grapheme_indices() {
            if start + 1 == end && c == "\u{FFFD}" {
                // invalid
                start_escape(fmt, &mut escaped)?;
                write!(fmt, "<{:02x}>", self.0[start])?;
            } else if c.width() > 0 {
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
