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

    pub dirty: bool,

    pub height: usize,
    pub cursory: usize,
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

    pub fn mutate<F: FnOnce(&mut BString, &mut usize, usize)->R, R>(&mut self, func: F) -> R {
        let byte_pos = self.cursor_byte_pos();
        let value = func(&mut self.contents, &mut self.cursor, byte_pos);
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

    pub fn set_contents(&mut self, contents: BString) {
        self.contents = contents;
        self.len = None;
        self.fix_cursor();
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
        self.fix_cursor();
    }

    pub fn reset(&mut self) {
        self.contents.clear();
        self.len = None;
        self.cursor = 0;
        self.cursory = 0;
        self.dirty = true;
    }

    pub fn cursor_byte_pos(&mut self) -> usize {
        self.contents
            .grapheme_indices()
            .nth(self.cursor)
            .map(|(s, _, _)| s)
            .unwrap_or_else(|| self.get_len())
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

        queue!(
            stdout,
            cursor::MoveToColumn(prompt_width as _),
            crossterm::style::Print(&prefix),
        )?;
        if !suffix.is_empty() {
            // then move back over it
            queue!(stdout, cursor::MoveLeft(1))?;
        }
        queue!(
            stdout,
            cursor::SavePosition,
            crossterm::style::Print(&suffix),
            Clear(ClearType::UntilNewLine),
            cursor::RestorePosition,
        )?;

        let mut prefix = " ".repeat(prompt_width) + &prefix;
        strip_colours(&mut prefix);
        self.cursory = wrap(&prefix, width as _).len() - 1;

        if suffix.is_empty() {
            self.height = self.cursory + 1;
        } else {
            // pop the space from the end
            prefix.pop();
            prefix += &suffix;
            strip_colours(&mut prefix);
            self.height = wrap(&prefix, width as _).len();
        }

        self.dirty = false;
        Ok(old != self.height)
    }

}
