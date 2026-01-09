use super::text::{HighlightedRange, Highlight};
use bstr::BString;
use std::ops::{Deref, DerefMut};
use std::io::{Write};
use crate::tui::{Drawer, Canvas};
use crate::buffer::Buffer;
use crate::shell::ShellClient;
use ratatui::layout::Rect;
mod prompt;

// for internal use
const PREDISPLAY_NS: usize = usize::MAX;
const POSTDISPLAY_NS: usize = PREDISPLAY_NS - 1;

#[derive(Default)]
pub struct CommandLineState {
    pub cursor_coord: (u16, u16),
    pub draw_end_pos: (u16, u16),

    predisplay_dirty: bool,
    postdisplay_dirty: bool,
    pub prompt: prompt::Prompt,
}

impl CommandLineState {
    pub fn into_command_line<'a>(
        &'a mut self,
        shell: &'a ShellClient,
        buffer: &'a mut Buffer,
    ) -> CommandLine<'a> {
        CommandLine {
            parent: self,
            shell,
            buffer,
        }
    }

    pub fn y_offset_to_end(&self) -> u16 {
        self.draw_end_pos.1 - self.cursor_coord.1
    }

}

pub struct CommandLine<'a> {
    parent: &'a mut CommandLineState,
    shell: &'a ShellClient,
    buffer: &'a mut Buffer,
}

impl CommandLine<'_> {

    pub fn set_is_dirty(&mut self, value: bool) {
        self.prompt.dirty = value;
        self.buffer.dirty = value;
        self.predisplay_dirty = value;
        self.postdisplay_dirty = value;
    }

    pub fn is_dirty(&self) -> bool {
        self.prompt.dirty || self.buffer.dirty
    }

    pub fn get_height(&self) -> usize {
        self.draw_end_pos.1 as usize
    }

    pub fn reset(&mut self) {
        self.prompt.dirty = true;
        self.buffer.dirty = true;
        self.predisplay_dirty = true;
        self.postdisplay_dirty = true;
        self.cursor_coord = (0, 0);
        self.draw_end_pos = (0, 0);
    }

    pub async fn refresh_display_string(&mut self, name: &str, pos: usize, namespace: usize) -> Option<BString> {
        let text = self.shell.get_var_as_string(name.into(), true).await.unwrap();
        self.buffer.clear_highlights_in_namespace(namespace);
        if let Some(text) = &text && !text.is_empty() {
            self.buffer.add_highlight(HighlightedRange {
                lineno: 0,
                start: pos,
                end: pos,
                inner: Highlight {
                    style: Default::default(),
                    blend: true,
                    namespace: PREDISPLAY_NS,
                    virtual_text: Some(text.clone()),
                },
            });
        }
        text
    }

    pub async fn refresh(&mut self, area: Rect) {
        let old_prompt_size = self.prompt.get_size();
        if self.prompt.dirty {
            self.parent.prompt.refresh(self.shell, area.width).await;
        }
        let new_prompt_size = self.prompt.get_size();

        // if the prompt width has changed, we redraw buffer as it may wrap differently
        if new_prompt_size.0 != old_prompt_size.0 {
            self.buffer.dirty = true;
        }

        if self.predisplay_dirty {
            self.refresh_display_string("PREDISPLAY", PREDISPLAY_NS, 0).await;
        }
        if self.postdisplay_dirty {
            self.refresh_display_string("POSTDISPLAY", POSTDISPLAY_NS, usize::MAX).await;
        }

        // self.predisplay.get_size(area.width, new_prompt_size.0);

        if !self.buffer.dirty && new_prompt_size.1 != old_prompt_size.1 {
            self.cursor_coord.1 = self.cursor_coord.1.saturating_sub(old_prompt_size.1) + new_prompt_size.1;
            self.draw_end_pos.1 = self.draw_end_pos.1.saturating_sub(old_prompt_size.1) + new_prompt_size.1;
        }

        if self.buffer.dirty {
            let (width, height) = self.buffer.get_size(area.width as _, new_prompt_size.0 as _);
            // there is up 1 overlapping line
            self.draw_end_pos = (width as _, height.saturating_sub(1) as u16 + new_prompt_size.1);
        }
    }

    pub fn render<W :Write, C: Canvas>(&mut self, drawer: &mut Drawer<W, C>, dirty: bool) -> std::io::Result<()> {

        let mut prompt_end = self.prompt.get_size();
        if prompt_end.0 >= drawer.term_width() {
            // wrap to next line
        } else {
            prompt_end.1 -= 1;
        }

        // redraw the prompt
        if dirty || self.prompt.dirty {
            drawer.write_raw(self.prompt.as_bytes(), prompt_end)?;
        }

        // redraw the buffer
        if dirty || self.buffer.dirty {
            // draw buffer starting from end of prompt
            drawer.move_to(prompt_end);

            // also record where is the cursor
            let cursor = self.buffer.cursor_byte_pos();
            let mut cursor_coord = drawer.get_pos();
            self.buffer.render(drawer, Some(|drawer: &mut Drawer<W, C>, lineno, _start, end| {
                if end == cursor && lineno == 0 {
                    cursor_coord = drawer.get_pos();
                }
            }))?;
            self.cursor_coord = cursor_coord;
            self.draw_end_pos = drawer.get_pos();
        }

        Ok(())
    }

}

impl Deref for CommandLine<'_> {
    type Target = CommandLineState;
    fn deref(&self) -> &Self::Target {
        self.parent
    }
}

impl DerefMut for CommandLine<'_> {
    fn deref_mut(&mut self) -> &mut CommandLineState {
        self.parent
    }
}
