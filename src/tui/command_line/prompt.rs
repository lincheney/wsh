use crate::shell::ShellClient;
use std::ffi::{CString, CStr};

const FALLBACK_PROMPT: &CStr = c">>> ";

#[derive(Default)]
pub struct Prompt {
    inner: CString,
    pub(super) height: u16,
    pub(super) width: u16,
    pub dirty: bool,
}

impl Prompt {

    pub fn get_size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    pub async fn refresh(&mut self, shell: &ShellClient, width: u16) {
        let prompt = shell.get_prompt(None, true).await.unwrap_or_else(|| FALLBACK_PROMPT.into());
        self.inner = crate::shell::remove_invisible_chars(&prompt).into();

        let size = shell.get_prompt_size(prompt.clone()).await;
        self.width = size.0 as _;
        self.height = size.1 as _;
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.inner.as_bytes()
    }
}
