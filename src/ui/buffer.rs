use byteyarn::ByteYarn;
use std::io::Write;
use bstr::{BStr, BString, ByteSlice};
use crate::tui::{Drawer, Canvas};
use crate::tui::text::{Text, HighlightedRange};
pub mod suffix;

#[derive(Debug)]
pub struct Edit {
    before: ByteYarn,
    after: ByteYarn,
    position: usize,
}

#[derive(Debug, Default)]
pub struct Buffer {
    contents: Text<usize>,
    // display: String,
    len: Option<usize>,
    cursor: usize,

    history: Vec<Edit>,
    history_index: usize,

    saved_contents: BString,
    saved_cursor: usize,

    completion_suffix: Option<(usize, suffix::Suffix)>,

    pub dirty: bool,
    pub highlight_counter: usize,
    pub height: usize,
}

impl Buffer {

    pub fn new() -> Self {
        let mut new = Self::default();
        new.contents.push_line(b"".into(), None);
        new
    }

    pub fn add_highlight(&mut self, hl: HighlightedRange<usize>) {
        self.contents.add_highlight(hl);
        self.dirty = true;
    }

    pub fn clear_highlights(&mut self) {
        self.contents.clear_highlights(None);
        self.dirty = true;
    }

    pub fn clear_highlights_in_namespace(&mut self, namespace: usize) {
        self.retain_highlights(|h| *h.namespace() != namespace);
    }

    pub fn retain_highlights<F: Fn(&HighlightedRange<usize>) -> bool>(&mut self, func: F) {
        self.contents.retain_highlights(func);
        self.dirty = true;
    }

    pub fn get_size(&self, width: usize, initial_indent: usize) -> (usize, usize) {
        self.contents.get_size(width, initial_indent, [].iter())
    }

    pub fn get_len(&mut self) -> usize {
        *self.len.get_or_insert_with(|| {
            let bytes = &self.contents.get()[0];
            if bytes.is_ascii() {
                bytes.len()
            } else {
                bytes.graphemes().count()
            }
        })
    }

    fn fix_cursor(&mut self) {
        if self.cursor > self.get_len() {
            self.cursor = self.get_len();
        }
        self.dirty = true;
    }

    pub fn get_contents(&self) -> &BString {
        &self.contents.get()[0]
    }

    pub fn get_cursor(&self) -> usize {
        self.cursor
    }

    pub fn set(&mut self, contents: Option<&[u8]>, cursor: Option<usize>) {
        if let Some(contents) = contents {
            self.splice_at(0, contents, self.get_contents().len(), true);
        }
        if let Some(cursor) = cursor {
            self.cursor = cursor;
        }
        self.fix_cursor();
    }

    pub(super) fn convert_to_insert<'a>(&self, contents: &'a [u8]) -> Option<&'a [u8]> {
        // see if this can be done as an insert
        let (prefix, suffix) = self.get_contents().split_at_checked(self.cursor).unwrap_or((self.get_contents().as_ref(), b""));
        if contents.starts_with(prefix) && contents[prefix.len()..].ends_with(suffix) {
            Some(&contents[prefix.len() .. contents.len() - suffix.len()])
        } else {
            None
        }
    }

    pub fn insert_or_set(&mut self, contents: Option<&[u8]>, cursor: Option<usize>) {
        if let Some(contents) = contents && let Some(insert) = self.convert_to_insert(contents) {
            self.insert_at_cursor(insert);
            self.set(None, cursor);
        } else {
            self.set(contents, cursor);
        }
    }

    pub fn set_contents(&mut self, contents: &[u8]) {
        self.set(Some(contents), None);
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.set(None, Some(cursor));
    }

    pub fn splice_at(&mut self, start: usize, data: &[u8], replace_len: usize, minimise: bool) {
        // turn it into an edit
        let end = self.byte_pos(start + replace_len);
        let mut start = self.byte_pos(start);
        let mut old = &self.get_contents()[start .. end];
        let mut new: &BStr = data.into();

        if minimise {
            if old == new {
                return
            }
            let prefix_len = new.iter().zip(old).take_while(|(x, y)| x == y).count();
            old = &old[prefix_len .. ];
            new = &new[prefix_len .. ];
            start += prefix_len;
            if !old.is_empty() && !new.is_empty() {
                let suffix_len = new.iter().rev().zip(old.iter().rev()).take_while(|(x, y)| x == y).count();
                old = &old[ .. old.len() - suffix_len];
                new = &new[ .. new.len() - suffix_len];
            }
        }

        let edit = Edit {
            before: ByteYarn::copy(old),
            after: ByteYarn::copy(new),
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

        self.contents.delete_str(0, edit.position, old.len());
        self.contents.insert_str(new.as_bytes().into(), 0, edit.position, true, None);
        self.len = None;

        // calculate the new cursor
        let end = edit.position + new.len();
        let bytes = self.get_contents();
        self.cursor = if bytes.is_ascii() {
            end.min(bytes.len())
        } else {
            bytes.grapheme_indices().take_while(|(s, _, _)| *s < end).count()
        };
        self.fix_cursor();
    }

    pub fn replace_completion_suffix(&mut self, suffix: Option<suffix::Suffix>) -> Option<(usize, suffix::Suffix)> {
        let old = self.completion_suffix.take();
        self.completion_suffix = suffix.map(|s| (self.get_cursor(), s));
        old
    }

    pub fn move_in_history(&mut self, forward: bool) -> bool {
        self.completion_suffix.take();
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
        self.splice_at(self.get_cursor(), data, 0, false);
    }

    pub fn delete_at_cursor(&mut self, count: usize, forwards: bool) {
        if forwards {
            self.splice_at(self.cursor, b"", count, false);
        } else {
            self.splice_at(self.cursor.saturating_sub(count), b"", count, false);
        }
    }

    pub fn save(&mut self) {
        self.saved_contents.resize(self.contents.get()[0].len(), 0);
        self.saved_contents.copy_from_slice(self.contents.get()[0].as_ref());
        self.saved_cursor = self.cursor;
    }

    pub fn restore(&mut self) {
        self.contents.swap_line(&mut self.saved_contents, 0, None);
        std::mem::swap(&mut self.cursor, &mut self.saved_cursor);
        self.len = None;
        self.fix_cursor();
    }

    pub fn reset(&mut self) {
        self.contents.reset();
        self.contents.push_line(b"".into(), None);
        self.len = None;
        self.history.clear();
        self.history_index = 0;
        self.height = 0;
        self.cursor = 0;
        self.saved_contents.clear();
        self.saved_cursor = 0;
        self.dirty = true;
    }

    fn byte_pos(&self, pos: usize) -> usize {
        let bytes = self.get_contents();

        if bytes.is_ascii() {
            return pos.min(bytes.len());
        }

        bytes
            .grapheme_indices()
            .nth(pos)
            .map(|(s, _, _)| s)
            .unwrap_or_else(|| bytes.len())
    }

    pub fn cursor_byte_pos(&self) -> usize {
        self.byte_pos(self.cursor)
    }

    fn get_cursor_lineno(&self) -> usize {
        self.contents.get()[0][..self.cursor_byte_pos()].split(|&c| c == b'\n').count().saturating_sub(1)
    }

    pub fn render<W :Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
        initial_indent: u16,
        max_height: Option<usize>,
    ) -> std::io::Result<(u16, u16)> {

        let cursor = self.cursor_byte_pos();
        let mut cursor_coord = drawer.get_pos();
        let width = drawer.term_width() as usize;
        let scroll = crate::tui::text::ScrollPosition::Line(self.get_cursor_lineno());

        let scrolled = crate::tui::scroll::wrap(
            &self.contents.get(),
            Some(self.contents.style.clone()),
            width,
            max_height,
            initial_indent as _,
            scroll,
            |parano| self.contents.highlights.get_for_parano(parano).iter(),
        );
        let mut lines = scrolled.into_lines();

        let mut first = true;
        let first_lineno = lines.first_lineno().unwrap_or(0);
        while let Some(slice) = lines.next() {
            let mut line = lines.slice(slice);

            if first {
                first = false;
                if first_lineno > 0 && initial_indent as usize + line.iter().map(|t| t.inner.width()).sum::<usize>() >= width {
                    // truncate first line if cursor is on line >= 2
                    // we don't truncate line 0
                    // which will be shown so long as cursor is on line <= 1
                    line = &line[line.len().min(initial_indent as usize) ..];
                }
            } else {
                // do not draw newline before first line
                drawer.goto_newline(None)?;
            }

            // draw the line
            let mut cell = crate::tui::Cell::EMPTY;
            for token in line {
                if let Some(symbol) = token.inner.as_str() {
                    cell.reset();
                    cell.set_text(symbol);
                    if let Some(style) = &token.style {
                        cell.style = style.clone();
                    }
                    drawer.draw_cell(&cell, false)?;
                }

                if token.range.end == cursor && token.parano == 0 {
                    cursor_coord = drawer.get_pos();
                }
            }
        }

        drawer.clear_to_end_of_line(None, crate::shell::is_interrupted())?;
        Ok(cursor_coord)
    }

}
