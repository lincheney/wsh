pub mod overrides;
use std::cell::Cell;
use bstr::{BString};
use std::os::raw::{c_int};
use std::ptr::{null_mut, NonNull};
use super::bindings;
use super::{MetaStr, MetaString, MetaArray};
use crossterm::execute;

#[derive(Eq, PartialEq, Clone, Copy)]
struct Widget(NonNull<bindings::widget>);

thread_local! {
    static SELF_INSERT: Cell<Option<Widget>> = const{ Cell::new(None) };
    static IMMORTAL_SELF_INSERT: Cell<Option<Widget>> = const{ Cell::new(None) };
    static UNDEFINED_KEY: Cell<Option<Widget>> = const{ Cell::new(None) };
    static IMMORTAL_UNDEFINED_KEY: Cell<Option<Widget>> = const{ Cell::new(None) };
    static ACCEPT_LINE: Cell<Option<Widget>> = const{ Cell::new(None) };
    static IMMORTAL_ACCEPT_LINE: Cell<Option<Widget>> = const{ Cell::new(None) };
}


pub struct ZleWidget {
    ptr: NonNull<bindings::thingy>,
}

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

impl ZleWidget {
    pub fn new(ptr: NonNull<bindings::thingy>) -> Self {
        let w = Self{ptr};

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

    pub fn name(&self) -> &MetaStr {
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

    pub(crate) fn exec_and_recover<I: Iterator<Item=MetaString> + ExactSizeIterator>(
        &self,
        _token: crate::shell::TrampolineToken,
        stdout: &mut std::io::Stdout,
        shell: &crate::shell::Shell,
        opts: Option<WidgetArgs>,
        args: I,
    ) -> (c_int, bool, Option<BString>) {
        unsafe {
            // we detect if it is refreshed by setting trashedzle to 1 then checking if it is reset to 0
            super::trashedzle = 1;
            let code = Self::exec_with_ptr(self.ptr, opts, args);

            let refreshed = super::trashedzle == 0;
            if refreshed {
                // move back up $BUFFERLINES
                super::start_zle_scope();
                let lines = super::Variable::get(meta_str!(c"BUFFERLINES")).unwrap().try_as_int().unwrap_or(Some(0)).unwrap_or(0);
                super::end_zle_scope();
                if lines > 0 {
                    crate::log_if_err(execute!(stdout, crate::tui::MoveUp(lines as u16 - 1)));
                }
            }

            // match lists are output in zrefresh() not inside the widget, so we have to grab it separately
            let output = if let Some(hookdef) = NonNull::new(zsh_sys::gethookdef(c"list_matches".as_ptr().cast_mut())) {
                let sink = &mut *shell.sink.borrow_mut();
                let (output, _code) = super::capture_shout(sink, || zsh_sys::runhookdef(hookdef.as_ptr(), null_mut()));
                Some(output)
            } else {
                None
            };

            (code, refreshed, output)
        }
    }

    #[allow(dead_code)]
    pub(crate) fn exec_and_get_output<I: Iterator<Item=MetaString> + ExactSizeIterator>(
        &mut self,
        _token: crate::shell::TrampolineToken,
        shell: &crate::shell::Shell,
        opts: Option<WidgetArgs>,
        args: I,
    ) -> (BString, c_int) {
        let sink = &mut *shell.sink.borrow_mut();
        super::capture_shout(sink, || Self::exec_with_ptr(self.ptr, opts, args))
    }

}
