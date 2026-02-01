use std::io::Write;
use bstr::{BString, ByteSlice};
use crate::tui::{Drawer, Canvas, text::Text, text::HighlightedRange};

#[derive(Debug)]
pub struct Edit {
    before: BString,
    after: BString,
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
        self.contents.clear_highlights();
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
        self.contents.get_size(width, initial_indent)
    }

    fn get_len(&mut self) -> usize {
        *self.len.get_or_insert_with(|| self.contents.get()[0].graphemes().count())
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
            // all highlights are now invalid!
            let len = self.get_contents().len();
            self.contents.delete_str(0, 0, len);
            self.cursor = 0;
            self.splice_at_cursor(contents, None);
        }
        if let Some(cursor) = cursor {
            self.cursor = cursor;
        }
        self.fix_cursor();
    }

    pub fn insert_or_set(&mut self, contents: Option<&[u8]>, cursor: Option<usize>) {
        if let Some(contents) = contents {
            // see if this can be done as an insert
            let (prefix, suffix) = &self.get_contents().split_at_checked(self.cursor).unwrap_or((self.get_contents().as_ref(), b""));
            if contents.starts_with(prefix) && contents.ends_with(suffix) {
                let contents = &contents[prefix.len() .. contents.len() - suffix.len()];
                self.insert_at_cursor(contents);
                self.set(None, cursor);
                return
            }
        }
        self.set(contents, cursor);
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
            self.get_contents().len()
        };
        let edit = Edit{
            before: self.get_contents()[start .. end].into(),
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

        self.contents.delete_str(0, edit.position, old.len());
        self.contents.insert_str(new.as_ref(), 0, edit.position, None);
        self.len = None;

        // calculate the new cursor
        let end = edit.position + new.len();
        self.cursor = self.get_contents().grapheme_indices().take_while(|(s, _, _)| *s < end).count();
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
        self.saved_contents.resize(self.contents.get()[0].len(), 0);
        self.saved_contents.copy_from_slice(self.contents.get()[0].as_ref());
        self.saved_cursor = self.cursor;
    }

    pub fn restore(&mut self) {
        self.contents.swap_line(&mut self.saved_contents, 0);
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
        self.get_contents()
            .grapheme_indices()
            .nth(pos)
            .map(|(s, _, _)| s)
            .unwrap_or_else(|| self.get_contents().len())
    }

    pub fn cursor_byte_pos(&self) -> usize {
        self.byte_pos(self.cursor)
    }

    pub fn render<W :Write, C: Canvas, F: FnMut(&mut Drawer<W, C>, usize, usize, usize)>(
        &self,
        drawer: &mut Drawer<W, C>,
        callback: Option<F>,
    ) -> std::io::Result<()> {
        self.contents.render_with_callback(drawer, None, None, [].iter(), callback)
    }

}
