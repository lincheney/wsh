use std::os::raw::{c_int};
use std::os::fd::{IntoRawFd, BorrowedFd};
use nix::sys::signal;
use anyhow::Result;
use std::sync::{RwLock, Arc, Weak};
use tokio::sync::{Notify};
use std::sync::atomic::{AtomicI32, Ordering};

static SELF_PIPE: AtomicI32 = AtomicI32::new(-1);
static RECEIVER: RwLock<Option<Arc<Notify>>> = RwLock::new(None);

pub fn get_subscriber() -> Option<Weak<Notify>> {
    RECEIVER.read().unwrap().as_ref().map(|x| Arc::downgrade(x))
}

pub(super) fn sighandler() -> c_int {
    #[allow(static_mut_refs)]
    unsafe {
        zsh_sys::zhandler(signal::Signal::SIGINT as _);
        let pipe = SELF_PIPE.load(Ordering::Acquire);
        if pipe != -1 {
            nix::unistd::write(BorrowedFd::borrow_raw(pipe), b"0").unwrap();
        }
        0
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
    *RECEIVER.write().unwrap() = None;
}

pub(in crate::shell) fn install_signal_handler() -> Result<()> {
    super::install_signal_handler(signal::Signal::SIGINT, false)
}

pub(super) fn init() -> Result<()> {
    let notify = Arc::new(Notify::new());
    *RECEIVER.write().unwrap() = Some(notify.clone());

    // spawn a reader task
    let writer = super::self_pipe::<_, _, std::convert::Infallible>(move || {
        notify.notify_waiters();
        Ok(())
    })?;

    // set the writer for the handler to use
    SELF_PIPE.store(writer.into_raw_fd(), Ordering::Release);

    super::hook_signal(signal::Signal::SIGINT)?;

    Ok(())
}
