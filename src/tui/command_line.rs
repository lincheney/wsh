use std::borrow::Cow;
use super::text::{HighlightedRange, Highlight, TextRenderer, Renderer, NoRendererCallback};
use bstr::{BString, BStr};
use std::io::{Write};
use crate::tui::{Drawer, Canvas};
use crate::ui::buffer::Buffer;
use crate::shell::{Shell, MetaStr};
use crate::meta_str;

const FALLBACK_PROMPT: &MetaStr = crate::meta_str!(c">>> ");
// for internal use
const PREDISPLAY_NS: usize = usize::MAX;
const POSTDISPLAY_NS: usize = PREDISPLAY_NS - 1;
const TRUNCATION_NS: usize = POSTDISPLAY_NS - 1;

pub const MAX_CMDLINE_HEIGHT: usize = 3;
#[derive(Default, Debug)]
pub struct ShellVars {
    predisplay: Option<BString>,
    postdisplay: Option<BString>,
    prompt: Cow<'static, BStr>,
    prompt_size: (usize, usize),
}

#[derive(Debug)]
pub enum PromptMode {
    ShellVars(ShellVars),
    Custom(super::widget::Widget),
}

impl Default for PromptMode {
    fn default() -> Self {
        Self::ShellVars(Default::default())
    }
}

#[derive(Default, Debug)]
pub struct CommandLineState {
    pub cursor_coord: (u16, u16),
    pub draw_end_pos: (u16, u16),

    pub prompt_mode: PromptMode,
    pub predisplay_dirty: bool,
    pub postdisplay_dirty: bool,
    pub prompt_dirty: bool,

    prompt_size: (usize, usize),
}

impl CommandLineState {
    pub fn is_custom(&self) -> bool {
        matches!(self.prompt_mode, PromptMode::Custom(_))
    }

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

    pub fn get_shell_vars(shell: &Shell, width: u32) -> ShellVars {
        shell.start_zle_scope();
        let predisplay = crate::shell::Variable::get(meta_str!(c"PREDISPLAY")).map(|mut v| v.as_bytes());
        let postdisplay = crate::shell::Variable::get(meta_str!(c"POSTDISPLAY")).map(|mut v| v.as_bytes());
        let prompt = shell.get_prompt(None, true).map_or(Cow::Borrowed(FALLBACK_PROMPT), Cow::Owned);
        let prompt_size = shell.get_prompt_size(&prompt, Some(width as _));
        let prompt = match crate::shell::remove_invisible_chars(prompt) {
            Cow::Owned(prompt) => Cow::Owned(prompt.unmetafy()),
            Cow::Borrowed(prompt) => prompt.unmetafy(),
        };
        shell.end_zle_scope();

        ShellVars {
            predisplay,
            postdisplay,
            prompt,
            prompt_size,
        }
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

    pub fn refresh_display_string(&mut self, text: Option<BString>, pos: usize, namespace: usize) {
        self.buffer.clear_highlights_in_namespace(namespace);
        if let Some(text) = text && !text.is_empty() {
            self.buffer.add_highlight(HighlightedRange {
                parano: 0,
                start: pos,
                end: pos,
                inner: Highlight {
                    style: Default::default(),
                    blend: true,
                    namespace,
                    virtual_text: Some(text),
                    conceal: None,
                    priority: 0.,
                },
            });
        }
    }

    pub fn refresh(&mut self, width: usize) {
        if self.predisplay_dirty || self.postdisplay_dirty {
            match &self.prompt_mode {
                PromptMode::ShellVars(vars) => {
                    let predisplay = vars.predisplay.clone();
                    let postdisplay = vars.postdisplay.clone();
                    if self.predisplay_dirty {
                        self.refresh_display_string(predisplay, PREDISPLAY_NS, 0);
                    }
                    if self.postdisplay_dirty {
                        self.refresh_display_string(postdisplay, POSTDISPLAY_NS, usize::MAX);
                    }
                }
                PromptMode::Custom(_) => {
                    if self.predisplay_dirty {
                        self.buffer.clear_highlights_in_namespace(PREDISPLAY_NS);
                    }
                    if self.postdisplay_dirty {
                        self.buffer.clear_highlights_in_namespace(POSTDISPLAY_NS);
                    }
                }
            }
        }

        let prompt_size = match &self.prompt_mode {
            PromptMode::ShellVars(vars) => vars.prompt_size,
            PromptMode::Custom(widget) => widget.inner.get_size(width, 0, widget.cursor_space_hl.iter()),
        };

        if self.buffer.dirty || self.prompt_size != prompt_size {
            self.prompt_size = prompt_size;
            let (width, height) = self.buffer.get_size(width, self.prompt_size.0 as _);
            // there is 1 overlapping line
            let y = (height + self.prompt_size.1).saturating_sub(2).min(MAX_CMDLINE_HEIGHT - 1);

            self.draw_end_pos = (width as _, y as _);
            self.buffer.dirty = true;
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
            match &self.prompt_mode {
                PromptMode::ShellVars(vars) => {
                    drawer.write_raw(&vars.prompt, Some(prompt_end))?;
                }
                PromptMode::Custom(widget) => {
                    TextRenderer::new(
                        &widget.inner,
                        0,
                        None,
                        drawer.term_width() as _,
                        None,
                        None,
                        |parano| widget.inner.highlights.get_for_parano(parano).iter(),
                    ).render(drawer, false, false, NoRendererCallback::None)?;
                }
            }
        }

        // redraw the buffer
        if dirty || self.buffer.dirty {
            // draw buffer starting from end of prompt
            if !drawer.try_move_to(prompt_end) {
                // no space for the buffer
                return Ok(())
            }

            // also record where is the cursor
            self.cursor_coord = self.buffer.render(drawer, prompt_end.0, Some(MAX_CMDLINE_HEIGHT))?;
            self.draw_end_pos = drawer.get_pos();
        }

        Ok(())
    }
}
