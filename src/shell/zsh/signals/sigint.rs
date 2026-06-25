use std::rc::{Rc, Weak};
use std::cell::RefCell;
use std::os::raw::c_int;
use nix::sys::signal;
use anyhow::Result;
use tokio::sync::Notify;

thread_local! {
    static NOTIFY: RefCell<Option<Rc<Notify>>> = const{ RefCell::new(None) };
}

pub fn handle_sigint(notify: &Rc<Notify>) {
    notify.notify_waiters();
}

pub fn get_subscriber() -> Option<Weak<Notify>> {
    NOTIFY.with_borrow(|n| n.as_ref().map(Rc::downgrade))
}

extern "C" fn sighandler(sig: c_int) {
    #[allow(static_mut_refs)]
    unsafe {

        // interrupt lua
        crate::lua::set_sigint_hook();

        zsh_sys::zhandler(sig);
        // bypass signal queueing
        super::write_to_self_pipe(super::SIGINT_BYTE);
    }
}

pub(super) fn cleanup() {
}

pub(in crate::shell) fn install_signal_handler() -> Result<()> {
    super::install_signal_handler(signal::Signal::SIGINT, false, Some(sighandler))
}

pub(super) fn init(_ui: &crate::ui::Ui) -> Result<Rc<Notify>> {
    let notify = Rc::new(Notify::new());
    NOTIFY.with_borrow_mut(|r| {
        *r = Some(notify.clone());
    });
    Ok(notify)
}
