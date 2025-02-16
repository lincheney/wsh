use std::ffi::{CStr};
use std::cmp::Ordering;
use std::os::raw::*;
use std::ptr::NonNull;
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

#[derive(Debug, Copy, Clone)]
pub struct EntryIter {
    ptr: Option<NonNull<zsh_sys::histent>>,
    up: bool,
}

impl EntryIter {
    pub fn up(&self) -> Self {
        Self{ up: true, ..*self }
    }

    pub fn down(&self) -> Self {
        Self{ up: false, ..*self }
    }

    fn end(&self) -> Option<NonNull<zsh_sys::histent>> {
        let mut iter = self.clone();
        while let Some(ptr) = iter.next_ptr() {
            iter.ptr = Some(ptr);
        }
        iter.ptr
    }

    pub fn top(&self) -> Self {
        Self{ up: false, ptr: self.up().end() }
    }

    pub fn bottom(&self) -> Self {
        Self{ up: true, ptr: self.down().end() }
    }

    fn next_ptr(&self) -> Option<NonNull<zsh_sys::histent>> {
        let hist = unsafe{ self.ptr?.as_ref() };
        let ptr = NonNull::new(if self.up { hist.up } else { hist.down })?;

        match unsafe{ ptr.as_ref() }.histnum.cmp(&hist.histnum) {
            Ordering::Greater if !self.up => Some(ptr),
            Ordering::Less if self.up => Some(ptr),
            _ => None,
        }
    }
}

impl Iterator for EntryIter {
    type Item = Entry;
    fn next(&mut self) -> Option<Self::Item> {
        let entry = Entry::from_histent(unsafe{ self.ptr?.as_ref() });
        self.ptr = self.next_ptr();
        Some(entry)
    }
}

impl std::iter::FusedIterator for EntryIter {}

pub fn get_history() -> EntryIter {
    EntryIter{ ptr: NonNull::new(unsafe{ zsh_sys::hist_ring }), up: true }
}
