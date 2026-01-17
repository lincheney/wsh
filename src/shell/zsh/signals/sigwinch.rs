use std::os::raw::{c_int};
use std::os::fd::{IntoRawFd, BorrowedFd};
use nix::sys::signal;
use anyhow::Result;
use std::sync::{RwLock};
use tokio::sync::{watch};
use std::sync::atomic::{AtomicI32, Ordering};

static SELF_PIPE: AtomicI32 = AtomicI32::new(-1);
static RECEIVER: RwLock<Option<watch::Receiver<(u32, u32)>>> = RwLock::new(None);

fn get_term_size_from_zsh() -> (u32, u32) {
    unsafe {
        (zsh_sys::zterm_columns.max(1) as _, zsh_sys::zterm_lines.max(1) as _)
    }
}

pub fn get_term_size() -> Option<(u32, u32)> {
    get_subscriber().map(|x| *x.borrow())
}

pub fn get_subscriber() -> Option<watch::Receiver<(u32, u32)>> {
    RECEIVER.read().unwrap().clone()
}

pub(super) fn sighandler() -> c_int {
    #[allow(static_mut_refs)]
    unsafe {
        zsh_sys::zhandler(signal::Signal::SIGWINCH as _);
        let pipe = SELF_PIPE.load(Ordering::Acquire);
        if pipe != -1 {
            nix::unistd::write(BorrowedFd::borrow_raw(pipe), b"0").unwrap();
        }
        0
    }
}

pub(super) fn init() -> Result<()> {
    let (sender, receiver) = watch::channel(get_term_size_from_zsh());
    *RECEIVER.write().unwrap() = Some(receiver);

    // spawn a reader task
    let writer = super::self_pipe::<_, _, std::convert::Infallible>(move || {
        let _ = sender.send(get_term_size_from_zsh());
        Ok(())
    })?;

    // set the writer for the handler to use
    SELF_PIPE.store(writer.into_raw_fd(), Ordering::Release);

    super::hook_signal(signal::Signal::SIGWINCH)?;

    Ok(())
}
