use std::borrow::Cow;
use super::text::{HighlightedRange, Highlight};
use bstr::BString;
use std::io::{Write};
use crate::tui::{Drawer, Canvas};
use crate::buffer::Buffer;
use crate::shell::{ShellClient, MetaStr};
use ratatui::layout::Rect;
use crate::meta_str;

const FALLBACK_PROMPT: &MetaStr = crate::meta_str!(c">>> ");
// for internal use
const PREDISPLAY_NS: usize = usize::MAX;
const POSTDISPLAY_NS: usize = PREDISPLAY_NS - 1;

#[derive(Default, Debug)]
pub struct ShellVars {
    predisplay: Option<BString>,
    postdisplay: Option<BString>,
    prompt: BString,
    prompt_size: (usize, usize),
}

#[derive(Default)]
pub struct CommandLineState {
    pub cursor_coord: (u16, u16),
    pub draw_end_pos: (u16, u16),

    pub shell_vars: ShellVars,
    pub predisplay_dirty: bool,
    pub postdisplay_dirty: bool,
    pub prompt_dirty: bool,

    prompt_size: (usize, usize),
}

impl CommandLineState {
    pub fn make_command_line<'a>(
        &'a mut self,
        buffer: &'a mut Buffer,
    ) -> CommandLine<'a> {
        CommandLine {
            parent: self,
            buffer,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.prompt_dirty || self.predisplay_dirty || self.postdisplay_dirty
    }

    pub fn y_offset_to_end(&self) -> u16 {
        self.draw_end_pos.1 - self.cursor_coord.1
    }

    pub async fn get_shell_vars(shell: &ShellClient, width: u32) -> ShellVars {
        shell.run(move |shell| {

            shell.start_zle_scope();
            let predisplay = crate::shell::get_var(shell, meta_str!(c"PREDISPLAY")).map(|mut v| v.as_bytes());
            let postdisplay = crate::shell::get_var(shell, meta_str!(c"POSTDISPLAY")).map(|mut v| v.as_bytes());
            let prompt = shell.get_prompt(None, true).map_or(Cow::Borrowed(FALLBACK_PROMPT), Cow::Owned);
            let prompt_size = shell.get_prompt_size(prompt.clone(), Some(width as _));
            let prompt = crate::shell::remove_invisible_chars(prompt).into_owned();
            shell.end_zle_scope();

            ShellVars {
                predisplay,
                postdisplay,
                prompt: prompt.as_bytes().into(),
                prompt_size,
            }
        }).await
    }

}

pub struct CommandLine<'a> {
    parent: &'a mut CommandLineState,
    buffer: &'a mut Buffer,
}

crate::impl_deref_helper!(self: CommandLine<'a>, self.parent => CommandLineState);
crate::impl_deref_helper!(mut self: CommandLine<'a>, self.parent => CommandLineState);

impl CommandLine<'_> {

    pub fn set_is_dirty(&mut self, value: bool) {
        self.buffer.dirty = value;
        self.prompt_dirty = value;
        self.predisplay_dirty = value;
        self.postdisplay_dirty = value;
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.dirty || self.parent.is_dirty()
    }

    pub fn get_height(&self) -> usize {
        self.draw_end_pos.1 as usize + 1
    }

    pub fn reset(&mut self) {
        self.set_is_dirty(true);
    }

    pub fn hard_reset(&mut self) {
        self.reset();
        self.cursor_coord = (0, 0);
        self.draw_end_pos = (0, 0);
    }

    pub fn refresh_display_string(&mut self, text: Option<BString>, pos: usize, namespace: usize) -> Option<BString> {
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
                    conceal: None,
                },
            });
        }
        text
    }

    pub fn refresh(&mut self, area: Rect) {
        if self.predisplay_dirty {
            self.refresh_display_string(self.shell_vars.predisplay.clone(), PREDISPLAY_NS, 0);
        }
        if self.postdisplay_dirty {
            self.refresh_display_string(self.shell_vars.postdisplay.clone(), POSTDISPLAY_NS, usize::MAX);
        }

        if self.buffer.dirty || self.prompt_size != self.shell_vars.prompt_size {
            self.prompt_size = self.shell_vars.prompt_size;
            let (width, height) = self.buffer.get_size(area.width as _, self.prompt_size.0 as _);
            // there is 1 overlapping line
            self.draw_end_pos = (width as _, (height + self.prompt_size.1).saturating_sub(2) as _);
        }
    }

    pub fn render<W :Write, C: Canvas>(&mut self, drawer: &mut Drawer<W, C>, dirty: bool) -> std::io::Result<()> {

        let mut prompt_end = (self.prompt_size.0 as u16, self.prompt_size.1 as u16);
        if prompt_end.0 >= drawer.term_width() {
            // wrap to next line
        } else {
            prompt_end.1 = prompt_end.1.saturating_sub(1);
        }

        // redraw the prompt
        if dirty || self.prompt_dirty {
            drawer.write_raw(&self.shell_vars.prompt, prompt_end)?;
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
