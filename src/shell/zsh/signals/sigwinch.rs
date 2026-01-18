use std::os::raw::{c_int};
use std::os::fd::{IntoRawFd, BorrowedFd};
use nix::sys::signal;
use anyhow::Result;
use std::sync::{RwLock};
use tokio::sync::{watch};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

static SELF_PIPE: AtomicI32 = AtomicI32::new(-1);
static RECEIVER: RwLock<Option<watch::Receiver<(u32, u32)>>> = RwLock::new(None);
static SIZE: AtomicU64 = AtomicU64::new(0);

pub(in crate::shell) fn fetch_term_size_from_zsh() {
    super::super::queue_signals();
    unsafe {
        let cols = zsh_sys::zterm_columns.max(1).min(u32::MAX as _) as u64;
        let lines = zsh_sys::zterm_lines.max(1).min(u32::MAX as _) as u64;
        SIZE.store((cols << 16) | lines, Ordering::Release);
    }
    let _ = super::super::unqueue_signals();
}

fn get_term_size_from_zsh() -> (u32, u32) {
    let size = SIZE.load(Ordering::Acquire);
    (
        (size >> 16) as _,
        (size & 0xffff) as _,
    )
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
            fetch_term_size_from_zsh();
            nix::unistd::write(BorrowedFd::borrow_raw(pipe), b"0").unwrap();
        }
        0
    }
}

pub(super) fn init() -> Result<()> {
    fetch_term_size_from_zsh();
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
