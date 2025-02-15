use std::ffi::CString;
use std::io::Write;
use bstr::BStr;
use anyhow::Result;
use crossterm::queue;
use crate::shell::Shell;

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

    pub async fn new(shell: &Shell, default: Option<&BStr>) -> Self {
        let mut prompt = Self::default();
        prompt.default_prompt = default
            .map(|s| CString::new(s.to_vec()))
            .unwrap_or_else(|| CString::new(Prompt::DEFAULT)).unwrap();

        prompt.refresh_prompt(shell).await;
        prompt
    }

    async fn refresh_prompt(&mut self, shell: &Shell) {
        let mut shell = shell.lock().await;
        let size = if let Some(prompt) = shell.get_prompt(None, false) {
            self.prompt = prompt;
            let prompt = shell.get_prompt(None, true).unwrap();
            shell.get_prompt_size(&prompt)
        } else {
            self.prompt = self.default_prompt.clone();
            shell.get_prompt_size(&self.prompt)
        };
        self.width = size.0;
        self.height = size.1;
    }

    pub fn needs_redraw(&self) -> bool {
        self.dirty
    }

    pub async fn draw(
        &mut self,
        stdout: &mut std::io::Stdout,
        shell: &Shell,
        _: (u16, u16),
    ) -> Result<()> {
        queue!(stdout, crossterm::cursor::MoveToColumn(0))?;
        self.refresh_prompt(shell).await;
        stdout.write_all(self.prompt.as_bytes())?;
        self.dirty = false;
        Ok(())
    }

}
