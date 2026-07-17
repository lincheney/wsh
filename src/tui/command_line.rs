use std::borrow::Cow;
use super::text::{Highlight, TextRenderer, Renderer, NoRendererCallback};
use bstr::{BString, BStr};
use std::io::{Write};
use crate::tui::{Drawer, Canvas};
use crate::ui::buffer::Buffer;
use crate::shell::{Shell, MetaStr};
use crate::meta_str;

const EMPTY_PROMPT: &MetaStr = crate::meta_str!(c"");
const FALLBACK_PROMPT: &MetaStr = crate::meta_str!(c">>> ");

#[derive(Default, Debug)]
pub struct ShellVarPrompt {
    pub inner: Cow<'static, BStr>,
    size: (usize, usize),
}

#[derive(Default, Debug)]
pub struct ShellVars {
    predisplay: Option<BString>,
    postdisplay: Option<BString>,
    prompt: ShellVarPrompt,
}

#[derive(Debug)]
pub enum PromptMode {
    ShellVars(ShellVars),
    Custom{widget: super::widget::Widget},
}

impl Default for PromptMode {
    fn default() -> Self {
        Self::ShellVars(Default::default())
    }
}

#[derive(Debug)]
pub enum RightPromptMode {
    ShellVars(ShellVarPrompt),
    Custom{widget: super::widget::Widget, auto_disappear: bool},
}

impl Default for RightPromptMode {
    fn default() -> Self {
        Self::ShellVars(Default::default())
    }
}

#[derive(Default, Debug)]
pub struct CommandLineState {
    pub cursor_coord: (u16, u16),
    pub draw_end_pos: (u16, u16),
    max_buffer_height_metric: super::sizing::Metric,
    max_buffer_height_value: u16,

    pub prompt_mode: PromptMode,
    pub rprompt_mode: RightPromptMode,

    pub predisplay_dirty: bool,
    pub postdisplay_dirty: bool,
    pub prompt_dirty: bool,
    pub rprompt_dirty: bool,

    prompt_size: (usize, usize),
    rprompt_size: (usize, usize),
}

impl CommandLineState {
    pub fn uses_shell_vars(&self) -> bool {
        matches!(self.prompt_mode, PromptMode::ShellVars(_))
        || matches!(self.rprompt_mode, RightPromptMode::ShellVars(_))
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
        self.prompt_dirty || self.rprompt_dirty || self.predisplay_dirty || self.postdisplay_dirty
    }

    pub fn y_offset_to_end(&self) -> u16 {
        self.draw_end_pos.1 - self.cursor_coord.1
    }

    pub fn update_shell_vars(&mut self, shell: &Shell, width: u32) {
        shell.start_zle_scope();

        if let PromptMode::ShellVars(vars) = &mut self.prompt_mode {
            let predisplay = crate::shell::Variable::get(meta_str!(c"PREDISPLAY")).map(|mut v| v.as_bytes());
            let postdisplay = crate::shell::Variable::get(meta_str!(c"POSTDISPLAY")).map(|mut v| v.as_bytes());
            let prompt = shell.get_prompt(None, true).map_or(Cow::Borrowed(FALLBACK_PROMPT), Cow::Owned);
            let prompt_size = shell.get_prompt_size(&prompt, Some(width as _));
            let prompt = match crate::shell::remove_invisible_chars(prompt) {
                Cow::Owned(prompt) => Cow::Owned(prompt.unmetafy()),
                Cow::Borrowed(prompt) => prompt.unmetafy(),
            };
            *vars = ShellVars {
                predisplay,
                postdisplay,
                prompt: ShellVarPrompt {
                    inner: prompt,
                    size: prompt_size,
                },
            };
        }

        if let RightPromptMode::ShellVars(vars) = &mut self.rprompt_mode {
            // TODO what if it is multi line???

            let prompt = shell.get_var_as_string(meta_str!(c"RPROMPT"), false);
            let prompt = prompt.map(crate::shell::MetaString::from);
            let prompt = prompt.and_then(|prompt| shell.get_prompt(Some(prompt.as_ref()), true));
            let prompt = prompt.map_or(Cow::Borrowed(EMPTY_PROMPT), Cow::Owned);

            let prompt_size = shell.get_prompt_size(&prompt, Some(width as _));
            let prompt = match crate::shell::remove_invisible_chars(prompt) {
                Cow::Owned(prompt) => Cow::Owned(prompt.unmetafy()),
                Cow::Borrowed(prompt) => prompt.unmetafy(),
            };
            *vars = ShellVarPrompt {
                inner: prompt,
                size: prompt_size,
            };
        }

        shell.end_zle_scope();

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
        self.rprompt_dirty = value;
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

    pub fn refresh(&mut self, width: usize, height: usize) {
        let prompt_size = match &self.prompt_mode {
            _ if !self.prompt_dirty => self.prompt_size,
            PromptMode::ShellVars(vars) => vars.prompt.size,
            PromptMode::Custom{widget} => widget.inner.get_size(width, 0, widget.cursor_space_hl.iter()),
        };

        let mut rprompt_size = match &self.rprompt_mode {
            _ if !self.rprompt_dirty => self.rprompt_size,
            // add extra space for the cursor
            RightPromptMode::ShellVars(vars) => (vars.size.0 + 1, vars.size.0),
            RightPromptMode::Custom{widget, ..} => {
                let size = widget.inner.get_size(width, 0, widget.cursor_space_hl.iter());
                (size.0 + 1, size.1)
            },
        };

        if (self.buffer.dirty || self.rprompt_size != rprompt_size) && self.rprompt_mode.can_disappear() {
            let first_line_width = self.buffer.get_first_line_width(width, prompt_size.0 + rprompt_size.0);
            if prompt_size.0 + first_line_width + rprompt_size.0 > width {
                rprompt_size = (0, 0);
                self.rprompt_dirty = true;
            }
        }

        if self.rprompt_size != rprompt_size {
            self.rprompt_dirty = true;
        }

        if self.buffer.dirty || self.prompt_size != prompt_size || self.rprompt_size != rprompt_size {
            self.prompt_size = prompt_size;
            self.rprompt_size = rprompt_size;

            let buf_size = self.buffer.get_size(width, prompt_size.0 + rprompt_size.0);

            // there is 1 overlapping line
            self.max_buffer_height_value = self.max_buffer_height_metric.resolve(Some(height as _));
            let y = (buf_size.1 + self.prompt_size.1).saturating_sub(2).min(self.max_buffer_height_value.saturating_sub(1) as _);

            self.draw_end_pos = (buf_size.0 as _, y as _);
            self.buffer.dirty = true;
        }
    }

    pub fn render<W :Write, C: Canvas>(&mut self, drawer: &mut Drawer<W, C>, dirty: bool) -> std::io::Result<()> {

        let mut prompt_end = (self.prompt_size.0 as u16, self.prompt_size.1 as u16);
        if prompt_end.0 >= drawer.term_width() {
            prompt_end.0 = 0;
        } else {
            prompt_end.1 = prompt_end.1.saturating_sub(1);
        }

        // redraw the prompt
        if dirty || self.prompt_dirty {
            let term_width = drawer.term_width() as usize;

            match &self.prompt_mode {
                PromptMode::ShellVars(vars) => {
                    drawer.write_raw(&vars.prompt.inner, Some(prompt_end))?;
                }
                PromptMode::Custom{widget} => {
                    TextRenderer::new(
                        &widget.inner,
                        0,
                        None,
                        term_width,
                        None,
                        None,
                        |parano| widget.inner.highlights.get_for_parano(parano).iter(),
                    ).render(drawer, false, false, NoRendererCallback::None)?;
                }
            }
        }

        // redraw the buffer
        if dirty || self.buffer.dirty || self.predisplay_dirty || self.postdisplay_dirty || self.rprompt_dirty {
            // draw buffer starting from end of prompt
            if !drawer.try_move_to(prompt_end) {
                // no space for the buffer
                return Ok(())
            }

            let mut predisplay = None;
            let mut postdisplay = None;
            if let PromptMode::ShellVars(vars) = &self.prompt_mode {
                if let Some(text) = &vars.predisplay && !text.is_empty() {
                    predisplay = Some(Highlight { virtual_text: Some(Cow::Borrowed(text.as_ref())), ..Default::default() });
                }
                if let Some(text) = &vars.postdisplay && !text.is_empty() {
                    postdisplay = Some(Highlight { virtual_text: Some(Cow::Borrowed(text.as_ref())), ..Default::default() });
                }
            }

            // also record where is the cursor
            self.cursor_coord = self.buffer.render(
                drawer,
                prompt_end.0 + self.rprompt_size.0 as u16,
                Some(self.max_buffer_height_value as _),
                predisplay,
                postdisplay,
                &self.rprompt_mode,
                self.rprompt_size,
                dirty || self.rprompt_dirty,
            )?;
            self.draw_end_pos = drawer.get_pos();
        }

        Ok(())
    }
}

impl RightPromptMode {

    fn can_disappear(&self) -> bool {
        !matches!(self, Self::Custom{auto_disappear: false, ..})
    }

    pub fn render<W :Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
        size: (usize, usize),
        dirty: bool,
    ) -> std::io::Result<()> {

        let pos = drawer.get_pos();
        let width = drawer.term_width() as usize;

        if dirty {
            // clear then draw the rprompt
            drawer.clear_to_end_of_line(None, true)?;
            if size.0 > 0 {
                drawer.move_to((width.saturating_sub(size.0 - 1) as _, pos.1));

                match self {
                    Self::ShellVars(vars) => {
                        drawer.write_raw(&vars.inner, None)?;
                    }
                    Self::Custom{widget, ..} => {
                        TextRenderer::new(
                            &widget.inner,
                            0,
                            None,
                            drawer.get_pos().0 as _,
                            Some(1),
                            // show the last line
                            Some(super::text::Scroll::default()),
                            |parano| widget.inner.highlights.get_for_parano(parano).iter(),
                        ).render(drawer, false, false, NoRendererCallback::None)?;
                    }
                }
            }
        } else {
            // still need to clear these cells up to the rprompt
            drawer.draw_cell_n_times(&Default::default(), false, width.saturating_sub(size.0 + pos.0 as usize) as u16)?;
            drawer.move_to((width as _, pos.1));
        }

        Ok(())
    }
}
