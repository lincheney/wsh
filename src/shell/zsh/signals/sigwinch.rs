use std::cell::RefCell;
use std::os::raw::{c_int};
use nix::sys::signal;
use tokio::sync::{watch};
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    static RECEIVER: RefCell<Option<watch::Receiver<(u32, u32)>>> = const{ RefCell::new(None) };
}
static SIZE: AtomicU64 = AtomicU64::new(0);

pub(in crate::shell) fn fetch_term_size_from_zsh() {
    let _ = super::with_queued_signals(|_| unsafe {
        let cols = zsh_sys::zterm_columns.max(1).min(u32::MAX as _) as u64;
        let lines = zsh_sys::zterm_lines.max(1).min(u32::MAX as _) as u64;
        SIZE.store((cols << 16) | lines, Ordering::Release);
    });
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
    RECEIVER.with_borrow(|r| r.clone())
}

pub fn handle_sigwinch(sender: &watch::Sender<(u32, u32)>) {
    let _ = sender.send(get_term_size_from_zsh());
}

pub(super) fn sighandler(_trapped: bool) -> c_int {
    #[allow(static_mut_refs)]
    unsafe {
        zsh_sys::zhandler(signal::Signal::SIGWINCH as _);
        if super::write_to_self_pipe(super::SIGWINCH_BYTE) {
            fetch_term_size_from_zsh();
        }
        0
    }
}

pub(super) fn cleanup() {
    RECEIVER.with_borrow_mut(|r| {
        *r = None;
    });
}

pub(super) fn init(_ui: &crate::ui::Ui) -> watch::Sender<(u32, u32)> {
    fetch_term_size_from_zsh();
    let (sender, receiver) = watch::channel(get_term_size_from_zsh());
    RECEIVER.replace(Some(receiver));
    sender
}
