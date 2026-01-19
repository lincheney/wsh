use std::cmp::Ordering;
use std::os::raw::c_long;
use std::ptr::NonNull;
use std::marker::PhantomData;
use bstr::{BString};
use super::{MetaStr};

#[derive(Debug)]
pub struct Entry {
    pub text: BString,
    pub start_time: c_long,
    pub finish_time: c_long,
    pub histnum: c_long,
}

impl From<&zsh_sys::histent> for Entry {
    fn from(histent: &zsh_sys::histent) -> Self {
        let text_ptr = if histent.zle_text.is_null() {
            histent.node.nam
        } else {
            histent.zle_text
        };
        let text = unsafe{ MetaStr::from_ptr(text_ptr) }.unmetafy().into_owned();

        Self {
            text,
            start_time: histent.stim,
            finish_time: histent.ftim,
            histnum: histent.histnum,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct EntryPtr<'a> {
    ptr: NonNull<zsh_sys::histent>,
    _marker: PhantomData<&'a ()>,
}

#[derive(Debug, Copy, Clone)]
pub enum HistoryIndex {
    Absolute(i32),
    Relative(i32),
}

pub struct History<'a> {
    ring: Option<EntryPtr<'a>>,
    _shell: &'a crate::shell::ShellInternal,
}

impl<'a> History<'a> {
    pub fn goto(index: HistoryIndex, skipdups: bool) {
        // no idea what the return value means
        match index {
            HistoryIndex::Absolute(index) => unsafe{ super::zle_goto_hist(index, 0, skipdups.into()) },
            HistoryIndex::Relative(index) => unsafe{ super::zle_goto_hist(super::histline, index, skipdups.into()) },
        };
    }

    pub fn get(shell: &'a crate::shell::ShellInternal) -> Self {
        Self{
            ring: EntryPtr::new(unsafe{ zsh_sys::hist_ring }),
            _shell: shell,
        }
    }

    pub fn first(&self) -> Option<EntryPtr<'a>> {
        self.ring
    }

    pub fn iter(&self) -> impl Iterator<Item=EntryPtr<'a>> {
        self.ring.iter().flat_map(|r| r.up_iter())
    }

    pub fn closest_to(&self, histnum: c_long, cmp: Ordering) -> Option<EntryPtr<'a>> {
        for entry in self.iter() {
            let found = entry.histnum();
            if found == histnum {
                return Some(entry)
            } else if found < histnum {
                return match cmp {
                    Ordering::Equal => None,
                    Ordering::Less => Some(entry),
                    Ordering::Greater => entry.down(),
                }
            }
        }
        None
    }

    pub fn set_histline(&mut self, histline: c_long) -> Option<EntryPtr<'_>> {
        if let Some(entry) = self.closest_to(histline, std::cmp::Ordering::Greater) {
            // found a good enough match
            unsafe{ super::histline = entry.histnum() as _; }
            Some(entry)

        } else {
            // no history
            unsafe{ super::histline = 0; }
            None
        }
    }

}

impl EntryPtr<'_> {
    fn new(ptr: *mut zsh_sys::histent) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| EntryPtr{ ptr, _marker: PhantomData })
    }

    fn down(self) -> Option<Self> {
        Self::new(unsafe{ zsh_sys::down_histent(self.ptr.as_ptr()) })
    }

    fn up(self) -> Option<Self> {
        Self::new(unsafe{ zsh_sys::up_histent(self.ptr.as_ptr()) })
    }

    pub fn histnum(self) -> c_long {
        unsafe{ self.ptr.as_ref() }.histnum
    }

    pub fn as_entry(self) -> Entry {
        unsafe{ self.ptr.as_ref() }.into()
    }

    pub fn down_iter(self) -> impl Iterator<Item=Self> {
        let mut ptr = self;
        std::iter::from_fn(move || {
            ptr = ptr.down()?;
            Some(ptr)
        })
    }

    pub fn up_iter(self) -> impl Iterator<Item=Self> {
        let mut ptr = self;
        std::iter::from_fn(move || {
            ptr = ptr.up()?;
            Some(ptr)
        })
    }
}
