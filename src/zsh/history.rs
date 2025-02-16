use std::ffi::{CStr};
use std::os::raw::*;
use bstr::BString;

#[derive(Debug)]
pub struct Entry {
    pub text: BString,
    pub start_time: zsh_sys::time_t,
    pub finish_time: zsh_sys::time_t,
    histnum: c_long,
}

impl Entry {
    fn from_histent(histent: &zsh_sys::histent) -> Self {
        let text_ptr = if !histent.zle_text.is_null() {
            histent.zle_text
        } else {
            histent.node.nam
        };
        let mut text = unsafe{ CStr::from_ptr(text_ptr) }.to_bytes_with_nul().to_owned();
        super::unmetafy_owned(&mut text);

        Self {
            text: text.into(),
            start_time: histent.stim,
            finish_time: histent.ftim,
            histnum: histent.histnum,
        }
    }
}

pub fn get_history() -> impl Iterator<Item=Entry> {
    use std::ptr::null_mut;
    unsafe{
        zsh_sys::readhistfile(null_mut(), 0, zsh_sys::HFILE_USE_OPTIONS as _);
    }

    let mut ptr = unsafe{ zsh_sys::hist_ring };
    if !ptr.is_null() {
        // move to end
        while unsafe{ *ptr }.down.is_null() {
            ptr = unsafe{ *ptr }.down;
        }
    }

    let start = ptr;
    std::iter::from_fn(move || {
        if let Some(hist) = unsafe{ ptr.as_ref() } {
            let value = Entry::from_histent(hist);
            ptr = hist.up;
            if ptr == start {
                ptr = null_mut();
            }
            Some(value)
        } else {
            None
        }
    })
}
