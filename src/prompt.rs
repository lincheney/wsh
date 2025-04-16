use std::ffi::CString;
use bstr::BStr;
use crate::shell::ShellInner;

#[derive(Default)]
pub struct Prompt {
    prompt: CString,
    default_prompt: CString,
    pub width: u16,
    pub height: u16,

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

    pub fn refresh_prompt(&mut self, shell: &mut ShellInner, width: u16,) {
        let prompt = shell.get_prompt(None, true).unwrap_or_else(|| self.default_prompt.clone());
        let size = shell.get_prompt_size(&prompt);
        self.prompt = ShellInner::remove_invisible_chars(&prompt).into();
        self.width = size.0 as _;
        self.height = size.1 as _;

        // actually takes up whole line
        if self.width >= width as _ {
            self.height += 1;
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.prompt.as_bytes()
    }

}
