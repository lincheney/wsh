use std::ffi::{CStr};
use std::cmp::Ordering;
use std::os::raw::*;
use std::ptr::NonNull;
use std::marker::PhantomData;
use bstr::{BString, BStr};

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
pub struct EntryPtr<'a> {
    ptr: NonNull<zsh_sys::histent>,
    _marker: PhantomData<&'a ()>,
}

pub struct History<'a, 'b> {
    ring: Option<EntryPtr<'a>>,
    _shell: &'a mut crate::shell::ShellInner<'b>,
}

impl<'a, 'b> History<'a, 'b> {
    pub fn get(shell: &'a mut crate::shell::ShellInner<'b>) -> Self {
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

pub fn push_history<'a>(string: &BStr) -> EntryPtr<'a> {
    let flags = 0; // TODO
    let string = unsafe{ CStr::from_ptr(super::metafy(string)) }.to_owned();

    let ptr = unsafe{ zsh_sys::prepnexthistent() };
    let hist = unsafe{ ptr.as_mut().unwrap() };
    hist.histnum = unsafe{ hist.up.as_ref() }.map_or(0, |h| h.histnum) + 1;
    hist.node.nam = string.into_raw();
    hist.ftim = 0;
    hist.node.flags = flags;

    if flags & zsh_sys::HIST_TMPSTORE as i32 != 0 {
        // uuhhhh what is this for?
        unsafe{ zsh_sys::addhistnode(zsh_sys::histtab, hist.node.nam, ptr.cast()); }
    }

    EntryPtr::new(ptr).unwrap()
}
