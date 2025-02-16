use std::ffi::{CStr};
use std::cmp::Ordering;
use std::os::raw::*;
use std::ptr::null_mut;
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

pub struct EntryIter {
    ptr: *const zsh_sys::histent,
    up: bool,
}

impl EntryIter {
    pub fn rev(&self) -> Self {
        Self{ ptr: self.ptr, up: !self.up }
    }
}

impl Iterator for EntryIter {
    type Item = Entry;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(hist) = unsafe{ self.ptr.as_ref() } {

            self.ptr = if self.up { hist.up } else { hist.down };

            match unsafe{ self.ptr.as_ref() }.map(|h| h.histnum.cmp(&hist.histnum)) {
                Some(Ordering::Greater) if !self.up => {},
                Some(Ordering::Less) if self.up => {},
                _ => { self.ptr = null_mut(); },
            }

            Some(Entry::from_histent(hist))
        } else {
            None
        }
    }
}

impl std::iter::FusedIterator for EntryIter {}

pub fn get_history() -> EntryIter {
    EntryIter{ ptr: unsafe{ zsh_sys::hist_ring }, up: true }
}
