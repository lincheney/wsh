use bstr::BString;
use anyhow::Result;
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

pub struct ZleWidget<'a, 'b> {
    ptr: NonNull<bindings::thingy>,
    pub shell: &'a mut crate::shell::ShellInner<'b>,
}

unsafe impl Send for ZleWidget<'_, '_> {}
unsafe impl Sync for ZleWidget<'_, '_> {}

pub struct WidgetArgs {
    times: u16,
    insert: bool,
}

impl Default for WidgetArgs {
    fn default() -> Self {
        Self {
            times: 1,
            insert: true,
        }
    }
}

impl<'a, 'b> ZleWidget<'a, 'b> {
    pub fn new(ptr: NonNull<bindings::thingy>, shell: &'a mut crate::shell::ShellInner<'b>) -> Self {
        let w = Self{ptr, shell};

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

        w
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
        Some(Widget(NonNull::new(unsafe{ self.ptr.as_ref() }.widget)?))
    }

    pub fn is_internal(&self) -> bool {
        unsafe{ (*self.ptr.as_ref().widget).flags & bindings::WidgetFlag::WIDGET_INT as i32 != 0 }
    }

    pub fn name(&self) -> &CStr {
        unsafe{ CStr::from_ptr(self.ptr.as_ref().nam) }
    }

    pub(crate) fn exec_with_ptr<'c, I: Iterator<Item=&'c BStr> + ExactSizeIterator>(
        ptr: NonNull<bindings::thingy>,
        opts: Option<WidgetArgs>,
        args: I,
    ) -> c_int {

        let opts = opts.unwrap_or_default();
        let mut null = std::ptr::null_mut();
        let args_ptr = if args.len() == 0 {
            &raw mut null
        } else {
            CStringArray::from_iter(args).ptr
        };

        unsafe {
            bindings::zmod.mult = opts.times.into();
            bindings::insmode = opts.insert.into();
            bindings::execzlefunc(ptr.as_ptr(), args_ptr, 0, 0)
        }
    }

    #[allow(dead_code)]
    pub(crate) fn exec<'c, I: Iterator<Item=&'c BStr> + ExactSizeIterator>(&self, opts: Option<WidgetArgs>, args: I) -> c_int {
        Self::exec_with_ptr(self.ptr, opts, args)
    }

    pub(crate) fn exec_and_get_output<'c, I: Iterator<Item=&'c BStr> + ExactSizeIterator>(&mut self, opts: Option<WidgetArgs>, args: I) -> Result<(BString, c_int)> {
        let ptr = self.ptr;
        self.shell.capture_shout(|_| Self::exec_with_ptr(ptr, opts, args))
    }

}
