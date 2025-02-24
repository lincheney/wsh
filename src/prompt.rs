use std::ffi::CString;
use std::io::Write;
use bstr::BStr;
use anyhow::Result;
use crossterm::{
    queue,
    terminal::{Clear, ClearType},
};
use crate::shell::ShellInner;

#[derive(Default)]
pub struct Prompt {
    prompt: CString,
    default_prompt: CString,
    pub width: usize,
    pub height: usize,

    pub dirty: bool,
}

impl Prompt {
    const DEFAULT: &str = ">>> ";

    pub fn new(default: Option<&BStr>) -> Self {
        let default_prompt = default
            .map(|s| CString::new(s.to_vec()))
            .unwrap_or_else(|| CString::new(Prompt::DEFAULT))
            .unwrap();
        Self{ default_prompt, ..Self::default() }
    }

    fn refresh_prompt(&mut self, shell: &mut ShellInner) {
        let prompt = shell.get_prompt(None, true).unwrap_or_else(|| self.default_prompt.clone());
        let size = shell.get_prompt_size(&prompt);
        self.prompt = ShellInner::remove_invisible_chars(&prompt).into();
        self.width = size.0;
        self.height = size.1;
    }

    pub fn draw(
        &mut self,
        stdout: &mut std::io::Stdout,
        shell: &mut ShellInner,
        (width, _height): (u16, u16),
    ) -> Result<bool> {

        let old = (self.width, self.height);
        self.refresh_prompt(shell);

        // actually takes up whole line
        if self.width >= width as _ {
            self.height += 1;
        }

        let changed = old != (self.width, self.height);

        if changed {
            queue!(stdout, Clear(ClearType::FromCursorDown))?;
        }
        queue!(stdout, crossterm::cursor::MoveToColumn(0))?;
        stdout.write_all(self.prompt.as_bytes())?;
        self.dirty = false;

        Ok(changed)
    }

}
