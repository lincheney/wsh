use std::ffi::{CStr};
use std::cmp::Ordering;
use std::os::raw::*;
use std::ptr::NonNull;
use bstr::*;

#[derive(Debug)]
pub struct Entry {
    pub text: BString,
    pub start_time: zsh_sys::time_t,
    pub finish_time: zsh_sys::time_t,
    pub histnum: c_long,
}

impl From<&zsh_sys::histent> for Entry {
    fn from(histent: &zsh_sys::histent) -> Self {
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

    fn end(&self) -> Self {
        let mut iter = self.clone();
        while let Some(ptr) = iter.next_ptr() {
            iter.ptr = Some(ptr);
        }
        iter
    }

    pub fn top(&self) -> Self {
        Self{ up: false, ptr: self.up().end().ptr }
    }

    pub fn bottom(&self) -> Self {
        Self{ up: true, ptr: self.down().end().ptr }
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

    fn next(&self) -> Option<Self> {
        self.next_ptr().map(|ptr| Self{ptr: Some(ptr), up: self.up})
    }

    pub fn iter(&self) -> impl Iterator<Item=&'static zsh_sys::histent> {
        let mut iter = self.clone();
        std::iter::from_fn(move || {
            let ptr = iter.ptr;
            iter.ptr = iter.next_ptr();
            ptr.map(|ptr| unsafe{ ptr.as_ref() })
        })
    }

    pub fn entries(&self) -> impl Iterator<Item=Entry> {
        self.iter().map(|e| e.into())
    }

    pub fn enumerate(&self) -> impl Iterator<Item=(c_long, &'static zsh_sys::histent)> {
        self.iter().map(|e| (e.histnum, e))
    }
}

pub fn get_history() -> EntryIter {
    EntryIter{ ptr: NonNull::new(unsafe{ zsh_sys::hist_ring }), up: true }
}

pub fn push_history(string: &BStr) -> EntryIter {
    let flags = 0; // TODO
    let string = unsafe{ CStr::from_ptr(super::metafy(string)) }.to_owned();

    let ptr = unsafe{ zsh_sys::prepnexthistent() };
    let hist = unsafe{ ptr.as_mut().unwrap() };
    hist.histnum = unsafe{ hist.up.as_ref() }.map(|h| h.histnum).unwrap_or(0) + 1;
    hist.node.nam = string.into_raw();
    hist.ftim = 0;
    hist.node.flags = flags;

    if flags & zsh_sys::HIST_TMPSTORE as i32 != 0 {
        // uuhhhh what is this for?
        unsafe{ zsh_sys::addhistnode(zsh_sys::histtab, hist.node.nam, ptr as _); }
    }

    EntryIter{ ptr: NonNull::new(ptr), up: true }
}
