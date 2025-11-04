use std::ffi::CString;
use bstr::BStr;
use crate::shell::ShellInner;

#[derive(Default)]
pub struct Prompt {
    inner: CString,
    default: CString,
    pub width: u16,
    pub height: u16,

    pub dirty: bool,
}

impl Prompt {
    const DEFAULT: &str = ">>> ";

    pub fn new(default: Option<&BStr>) -> Self {
        let default = default
            .map(|s| CString::new(s.to_vec()))
            .unwrap_or_else(|| CString::new(Prompt::DEFAULT))
            .unwrap();
        Self{ default, ..Self::default() }
    }

    pub fn refresh_prompt(&mut self, shell: &mut ShellInner, width: u16,) {
        let prompt = shell.get_prompt(None, true).unwrap_or_else(|| self.default.clone());
        let size = shell.get_prompt_size(&prompt);
        self.inner = ShellInner::remove_invisible_chars(&prompt).into();
        self.width = size.0 as _;
        self.height = size.1 as _;

        // actually takes up whole line
        if self.width >= width as _ {
            self.height += 1;
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.inner.as_bytes()
    }

}
