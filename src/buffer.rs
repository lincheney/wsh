use std::ops::Range;
use std::io::Write;
use bstr::{BString, ByteSlice};
use unicode_width::UnicodeWidthStr;
use ratatui::style::{Style, Color};
use crate::tui::{Drawer};

#[derive(Debug)]
pub struct Edit {
    before: BString,
    after: BString,
    position: usize,
}

#[derive(Debug)]
pub struct Highlight {
    pub start: usize,
    pub end: usize,
    pub style: Style,
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

impl HighlightStack<'_> {
    fn merge(&self) -> Style {
        let mut style = Style::new();
        for h in &self.0 {
            style = style.patch(h.style);
        }
        style
    }
}

#[derive(Debug, Default)]
pub struct Buffer {
    contents: BString,
    // display: String,
    len: Option<usize>,
    cursor: usize,

    history: Vec<Edit>,
    history_index: usize,

    saved_contents: BString,
    saved_cursor: usize,

    pub dirty: bool,

    pub draw_end_pos: (u16, u16),
    pub cursor_coord: (u16, u16),

    pub highlights: Vec<Highlight>,
    pub highlight_counter: usize,
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
            // all highlights are now invalid!
            self.highlights.clear();
            self.contents.resize(contents.len(), 0);
            self.cursor = 0;
            self.splice_at_cursor(contents, Some(self.contents.len()));
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
        // turn it into an edit
        let start = self.cursor_byte_pos();
        let end = if let Some(replace_len) = replace_len {
            self.byte_pos(self.cursor + replace_len)
        } else {
            self.contents.len()
        };
        let edit = Edit{
            before: self.contents[start .. end].into(),
            after: data.into(),
            position: start,
        };
        if self.history_index < self.history.len() {
            drop(self.history.drain(self.history_index .. ));
        }
        self.history.push(edit);
        self.apply_edit(self.history_index, false);
        self.history_index += 1;
    }

    fn apply_edit(&mut self, index: usize, reverse: bool) {
        let edit = &self.history[index];
        let (old, new) = if reverse {
            (&edit.after, &edit.before)
        } else {
            (&edit.before, &edit.after)
        };

        let start = edit.position;
        let end = start + old.len();
        self.contents.splice(start .. end, new.iter().copied());
        self.len = None;

        self.highlights.retain_mut(|hl| {
            hl.shift(start .. end, start + new.len());
            !hl.is_empty()
        });

        // calculate the new cursor
        let end = start + new.len();
        self.cursor = self.contents.grapheme_indices().take_while(|(s, _, _)| *s < end).count();
        self.fix_cursor();
    }

    pub fn move_in_history(&mut self, forward: bool) -> bool {
        if forward && self.history_index < self.history.len() {
            self.apply_edit(self.history_index, false);
            self.history_index += 1;
            true
        } else if !forward && self.history_index > 0 {
            self.history_index -= 1;
            self.apply_edit(self.history_index, true);
            true
        } else {
            false
        }
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

    pub fn cursor_is_at_end(&self) -> bool {
        self.cursor_byte_pos() >= self.contents.len()
    }

    pub fn reset(&mut self) {
        self.contents.clear();
        self.len = None;
        self.history.clear();
        self.history_index = 0;
        self.cursor = 0;
        self.cursor_coord = (0, 0);
        self.draw_end_pos = (0, 0);
        self.saved_contents.clear();
        self.saved_cursor = 0;
        self.dirty = true;
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

    pub fn render<W :Write>(&mut self, drawer: &mut Drawer<W>) -> std::io::Result<()> {

        let cursor = self.cursor_byte_pos();

        if self.contents.is_empty() {
            drawer.clear_to_end_of_line()?;
            self.cursor_coord = drawer.cur_pos;
            self.draw_end_pos = drawer.cur_pos;
            return Ok(())
        }

        if cursor == 0 {
            self.cursor_coord = drawer.cur_pos;
        }

        let escape_style = Style::default().fg(Color::Gray);
        let mut stack = HighlightStack(vec![]);
        let mut cell = ratatui::buffer::Cell::EMPTY;

        for (i, (start, end, c)) in self.contents.grapheme_indices().enumerate() {
            if self.highlights.iter().any(|h| h.start == i) {
                stack.0.extend(self.highlights.iter().filter(|h| h.start == i));
                cell.set_style(stack.merge());
            }

            if c == "\n" {
                drawer.goto_newline()?;
            } else if c.width() > 0 && (start + 1 != end || c != "\u{FFFD}") {
                let mut cell = cell.clone();
                cell.set_symbol(c);
                drawer.draw_cell(&cell, false)?;
            } else {
                // invalid
                let mut cell = cell.clone();
                cell.set_style(cell.style().patch(escape_style));
                for c in self.contents[start..end].iter() {
                    let mut cursor = std::io::Cursor::new([0; 64]);
                    write!(cursor, "<u{c:04x}>").unwrap();
                    let buf = &cursor.get_ref()[..cursor.position() as usize];
                    cell.set_symbol(std::str::from_utf8(buf).unwrap());
                    drawer.draw_cell(&cell, false)?;
                }
            }

            if end == cursor {
                self.cursor_coord = drawer.cur_pos;
            }

            if !stack.0.iter().all(|h| h.end > i + 1) {
                stack.0.retain(|h| h.end > i + 1 );
                cell.set_style(stack.merge());
            }
        }

        let old_end_pos = self.draw_end_pos;
        self.draw_end_pos = drawer.cur_pos;

        // clear any old lines below
        if self.draw_end_pos.1 < old_end_pos.1 {
            for _ in self.draw_end_pos.1 .. old_end_pos.1 {
                drawer.goto_newline()?;
            }
        }
        // clear the last/current line
        drawer.clear_to_end_of_line()?;

        Ok(())
    }

}
