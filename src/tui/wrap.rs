use unicode_width::UnicodeWidthStr;
use std::io::{Write, Cursor};
use bstr::{BStr, ByteSlice};

pub struct Wrapper<'a> {
    prev_range: (usize, usize),
    width: usize,
    max_width: usize,
    invalid: Option<(usize, usize)>,
    line: &'a BStr,
    graphemes: bstr::GraphemeIndices<'a>,
}

impl Wrapper<'_> {
    fn add_width(&mut self, width: usize, new_end: usize) -> Option<((usize, usize), usize)> {
        let old_width = self.width;
        self.width += width;
        if self.width > self.max_width {
            // wrap
            self.width = width;
            self.prev_range = (self.prev_range.1, new_end);
            Some((self.prev_range, old_width))
        } else {
            None
        }
    }
}

impl Iterator for Wrapper<'_> {
    type Item = ((usize, usize), usize);
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.prev_range.1 >= self.line.len() {
                return None

            } else if let Some((start, end)) = self.invalid.take() {
                // iter over previous invalid text
                let mut cursor = Cursor::new([0; 64]);
                for (i, c) in self.line[start .. end].iter().enumerate() {
                    cursor.set_position(0);
                    write!(cursor, "<u{c:04x}>").unwrap();
                    if let Some(result) = self.add_width(cursor.position() as usize, start + i) {
                        self.invalid = Some((start + i, end));
                        return Some(result)
                    }
                }

            } else if let Some((start, end, c)) = self.graphemes.next() {

                if c == "\n" {
                    // newline
                    let old_width = self.width;
                    self.width = 0;
                    self.prev_range = (self.prev_range.1, end);
                    return Some((self.prev_range, old_width))
                } else if c == "\t" {
                    let result = self.add_width(super::text::TAB_WIDTH, start);
                    if result.is_some() {
                        return result
                    }
                } else if c.width() > 0 && c != "\u{FFFD}" {
                    let result = self.add_width(c.width(), start);
                    if result.is_some() {
                        return result
                    }
                } else {
                    // invalid text
                    self.invalid = Some((start, end));
                }
            } else {
                // no more text, emit last line
                self.prev_range = (self.prev_range.1, self.line.len());
                return Some((self.prev_range, self.width))
            }
        }
    }
}

pub fn wrap(line: &BStr, max_width: usize, initial_indent: usize) -> Wrapper<'_> {
    Wrapper {
        prev_range: (0, 0),
        width: initial_indent,
        max_width,
        invalid: None,
        line,
        graphemes: line.grapheme_indices(),
    }
}
