use std::rc::{Rc, Weak};
use std::cell::RefCell;
use std::os::raw::{c_int};
use std::os::fd::{IntoRawFd, BorrowedFd};
use nix::sys::signal;
use anyhow::Result;
use tokio::sync::{Notify};
use std::sync::atomic::{AtomicI32, Ordering};

static SELF_PIPE: AtomicI32 = AtomicI32::new(-1);
thread_local! {
    static RECEIVER: RefCell<Option<Rc<Notify>>> = const{ RefCell::new(None) };
}

pub fn get_subscriber() -> Option<Weak<Notify>> {
    RECEIVER.with_borrow(|r| r.as_ref().map(Rc::downgrade))
}

extern "C" fn sighandler(sig: c_int) {
    #[allow(static_mut_refs)]
    unsafe {

        // interrupt lua
        crate::lua::set_sigint_hook();

        zsh_sys::zhandler(sig);
        // bypass signal queueing
        let pipe = SELF_PIPE.load(Ordering::Acquire);
        if pipe != -1 {
            nix::unistd::write(BorrowedFd::borrow_raw(pipe), b"0").unwrap();
        }
    }
}

fn close_self_pipe() {
    let fd = SELF_PIPE.swap(-1, Ordering::AcqRel);
    if fd != -1 {
        let _ = nix::unistd::close(fd);
    }
}

pub(super) fn cleanup() {
    close_self_pipe();
    RECEIVER.with(|r| {
        *r.borrow_mut() = None;
    });
}

pub(in crate::shell) fn install_signal_handler() -> Result<()> {
    super::install_signal_handler(signal::Signal::SIGINT, false, Some(sighandler))
}

pub(super) fn init(ui: &crate::ui::Ui) -> Result<()> {
    let notify = Rc::new(Notify::new());
    RECEIVER.with(|r| {
        *r.borrow_mut() = Some(notify.clone());
    });

    // spawn a reader task
    let writer = super::self_pipe::<_, _, std::convert::Infallible>(ui, move || {
        notify.notify_waiters();
        Ok(())
    })?;

    // set the writer for the handler to use
    SELF_PIPE.store(writer.into_raw_fd(), Ordering::Release);

    super::hook_signal(signal::Signal::SIGINT)?;

    Ok(())
}
