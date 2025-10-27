use std::ffi::{CStr};
use bstr::BStr;
use crate::c_string_array::{CStringArray};
use std::os::raw::{c_int};
use std::sync::{OnceLock};
use std::ptr::NonNull;
use super::bindings;

#[derive(Eq, PartialEq)]
struct Widget(NonNull<bindings::widget>);
unsafe impl Send for Widget {}
unsafe impl Sync for Widget {}

static SELF_INSERT: OnceLock<Widget> = OnceLock::new();
static IMMORTAL_SELF_INSERT: OnceLock<Widget> = OnceLock::new();
static UNDEFINED_KEY: OnceLock<Widget> = OnceLock::new();
static IMMORTAL_UNDEFINED_KEY: OnceLock<Widget> = OnceLock::new();
static ACCEPT_LINE: OnceLock<Widget> = OnceLock::new();
static IMMORTAL_ACCEPT_LINE: OnceLock<Widget> = OnceLock::new();

pub struct ZleWidget(NonNull<bindings::thingy>);
unsafe impl Send for ZleWidget {}
unsafe impl Sync for ZleWidget {}

impl ZleWidget {
    pub fn new(ptr: NonNull<bindings::thingy>) -> Self {
        let w = Self(ptr);

        if w.is_internal() && let Some(widget) = w.widget() {
            // these are just caches
            if SELF_INSERT.get().is_none() && w.name() == c"self-insert" {
                let _ = SELF_INSERT.set(widget);
            } else if IMMORTAL_SELF_INSERT.get().is_none() && w.name() == c".self-insert" {
                let _ = IMMORTAL_SELF_INSERT.set(widget);
            } else if UNDEFINED_KEY.get().is_none() && w.name() == c"undefined-key" {
                let _ = UNDEFINED_KEY.set(widget);
            } else if IMMORTAL_UNDEFINED_KEY.get().is_none() && w.name() == c".undefined-key" {
                let _ = IMMORTAL_UNDEFINED_KEY.set(widget);
            } else if ACCEPT_LINE.get().is_none() && w.name() == c"accept-line" {
                let _ = ACCEPT_LINE.set(widget);
            } else if IMMORTAL_ACCEPT_LINE.get().is_none() && w.name() == c".accept-line" {
                let _ = IMMORTAL_ACCEPT_LINE.set(widget);
            }
        }

        return w
    }

    pub fn is_self_insert(&self) -> bool {
        let widget = self.widget();
        let widget = widget.as_ref();
        widget.is_some() && (widget == SELF_INSERT.get() || widget == IMMORTAL_SELF_INSERT.get())
    }

    pub fn is_undefined_key(&self) -> bool {
        let widget = self.widget();
        let widget = widget.as_ref();
        widget.is_some() && (widget == UNDEFINED_KEY.get() || widget == IMMORTAL_UNDEFINED_KEY.get())
    }

    pub fn is_accept_line(&self) -> bool {
        let widget = self.widget();
        let widget = widget.as_ref();
        widget.is_some() && (widget == ACCEPT_LINE.get() || widget == IMMORTAL_ACCEPT_LINE.get())
    }

    fn widget(&self) -> Option<Widget> {
        Some(Widget(NonNull::new(unsafe{ self.0.as_ref() }.widget)?))
    }

    pub fn is_internal(&self) -> bool {
        unsafe{ (*self.0.as_ref().widget).flags & bindings::WidgetFlag::WIDGET_INT as i32 != 0 }
    }

    pub fn name(&self) -> &CStr {
        unsafe{ CStr::from_ptr(self.0.as_ref().nam) }
    }

    pub(crate) fn exec<'a, I: Iterator<Item=&'a BStr> + ExactSizeIterator>(&self, args: I) -> c_int {
        let mut null = std::ptr::null_mut();
        let args_ptr;
        if args.len() == 0 {
            args_ptr = &raw mut null;
        } else {
            args_ptr = CStringArray::from_iter(args).ptr;
        }

        unsafe {
            bindings::execzlefunc(self.0.as_ptr(), args_ptr, 0, 0)
        }
    }

}
