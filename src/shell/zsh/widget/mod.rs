pub mod overrides;
use std::cell::Cell;
use bstr::{BString};
use std::os::raw::{c_int};
use std::ptr::NonNull;
use super::bindings;
use super::{MetaStr, MetaString, MetaArray};

#[derive(Eq, PartialEq, Clone, Copy)]
struct Widget(NonNull<bindings::widget>);

thread_local! {
    static SELF_INSERT: Cell<Option<Widget>> = Cell::new(None);
    static IMMORTAL_SELF_INSERT: Cell<Option<Widget>> = Cell::new(None);
    static UNDEFINED_KEY: Cell<Option<Widget>> = Cell::new(None);
    static IMMORTAL_UNDEFINED_KEY: Cell<Option<Widget>> = Cell::new(None);
    static ACCEPT_LINE: Cell<Option<Widget>> = Cell::new(None);
    static IMMORTAL_ACCEPT_LINE: Cell<Option<Widget>> = Cell::new(None);
}


pub struct ZleWidget<'a> {
    ptr: NonNull<bindings::thingy>,
    pub shell: &'a crate::shell::ShellInternal,
}

unsafe impl Send for ZleWidget<'_> {}
unsafe impl Sync for ZleWidget<'_> {}

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

fn insert_widget_cache(widget: &ZleWidget, cache: &'static std::thread::LocalKey<Cell<Option<Widget>>>, name: &MetaStr) -> bool {
    if cache.get().is_none() && widget.name() == name && let Some(widget) = widget.widget() {
        cache.set(Some(widget));
        true
    } else {
        false
    }
}

impl<'a> ZleWidget<'a> {
    pub fn new(ptr: NonNull<bindings::thingy>, shell: &'a crate::shell::ShellInternal) -> Self {
        let w = Self{ptr, shell};

        if w.is_internal() {
            // these are just caches
            let _ = insert_widget_cache(&w, &SELF_INSERT, meta_str!(c"self-insert"))
            || insert_widget_cache(&w, &IMMORTAL_SELF_INSERT, meta_str!(c".self-insert"))
            || insert_widget_cache(&w, &UNDEFINED_KEY, meta_str!(c"undefined-key"))
            || insert_widget_cache(&w, &IMMORTAL_UNDEFINED_KEY, meta_str!(c".undefined-key"))
            || insert_widget_cache(&w, &ACCEPT_LINE, meta_str!(c"accept-line"))
            || insert_widget_cache(&w, &IMMORTAL_ACCEPT_LINE, meta_str!(c".accept-line"));
        }

        w
    }

    pub fn is_self_insert(&self) -> bool {
        let widget = self.widget();
        widget.is_some() && (widget == SELF_INSERT.get() || widget == IMMORTAL_SELF_INSERT.get())
    }

    pub fn is_undefined_key(&self) -> bool {
        let widget = self.widget();
        widget.is_some() && (widget == UNDEFINED_KEY.get() || widget == IMMORTAL_UNDEFINED_KEY.get())
    }

    pub fn is_accept_line(&self) -> bool {
        let widget = self.widget();
        widget.is_some() && (widget == ACCEPT_LINE.get() || widget == IMMORTAL_ACCEPT_LINE.get())
    }

    fn widget(&self) -> Option<Widget> {
        Some(Widget(NonNull::new(unsafe{ self.ptr.as_ref() }.widget)?))
    }

    pub fn is_internal(&self) -> bool {
        unsafe{ (*self.ptr.as_ref().widget).flags & bindings::WidgetFlag::WIDGET_INT as i32 != 0 }
    }

    pub fn name(&self) -> &'a MetaStr {
        unsafe {
            MetaStr::from_ptr(self.ptr.as_ref().nam)
        }
    }

    pub(crate) fn exec_with_ptr<I: Iterator<Item=MetaString> + ExactSizeIterator>(
        ptr: NonNull<bindings::thingy>,
        opts: Option<WidgetArgs>,
        args: I,
    ) -> c_int {

        let opts = opts.unwrap_or_default();
        let args: MetaArray = args.collect();

        unsafe {
            bindings::zmod.mult = opts.times.into();
            bindings::insmode = opts.insert.into();
            bindings::execzlefunc(ptr.as_ptr(), args.as_ptr().cast_mut(), 0, 0)
        }
    }

    #[allow(dead_code)]
    pub(crate) fn exec<I: Iterator<Item=MetaString> + ExactSizeIterator>(&self, opts: Option<WidgetArgs>, args: I) -> c_int {
        Self::exec_with_ptr(self.ptr, opts, args)
    }

    pub(crate) fn exec_and_get_output<I: Iterator<Item=MetaString> + ExactSizeIterator>(&mut self, opts: Option<WidgetArgs>, args: I) -> (BString, c_int) {
        let sink = &mut *self.shell.sink.lock().unwrap();
        super::capture_shout(sink, || Self::exec_with_ptr(self.ptr, opts, args))
    }

}
