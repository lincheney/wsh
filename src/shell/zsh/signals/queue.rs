use nix::sys::signal;
use std::ptr::{read_volatile, write_volatile};

pub struct QueuedSignalToken{
    used: bool,
}

impl Drop for QueuedSignalToken {
    fn drop(&mut self) {
        if !self.used {
            let _ = unqueue_signals_with_token(self);
        }
    }
}

pub fn queue_signal_level() -> i32 {
    unsafe {
        read_volatile(&raw const zsh_sys::queueing_enabled)
    }
}

pub fn queue_signals() -> QueuedSignalToken {
    unsafe {
        write_volatile(&raw mut zsh_sys::queueing_enabled, queue_signal_level() + 1);
        QueuedSignalToken{used: false}
    }
}

fn unqueue_signals_with_token(token: &mut QueuedSignalToken) -> nix::Result<()> {
    token.used = true;
    unsafe {
        let level = queue_signal_level() - 1;
        write_volatile(&raw mut zsh_sys::queueing_enabled, level);
        if level == 0 {
            run_queued_signals()?;
        }
    }
    Ok(())
}

pub fn unqueue_signals(mut token: QueuedSignalToken) -> nix::Result<()> {
    unqueue_signals_with_token(&mut token)
}

fn run_queued_signals() -> nix::Result<()> {
    const MAX_QUEUE_SIZE: i32 = 128;

    unsafe {
        loop {
            let queue_front = read_volatile(&raw const zsh_sys::queue_front);
            if queue_front == read_volatile(&raw const zsh_sys::queue_rear) { /* while signals in queue */
                break
            }
            let queue_front = queue_front + 1;
            write_volatile(&raw mut zsh_sys::queue_front, queue_front % MAX_QUEUE_SIZE);
            let sigset = zsh_sys::signal_mask_queue[queue_front as usize];
            let sigset = signal::SigSet::from_sigset_t_unchecked(std::mem::transmute::<zsh_sys::__sigset_t, nix::libc::sigset_t>(sigset));
            let mut oset = signal::SigSet::empty();
            signal::sigprocmask(signal::SigmaskHow::SIG_SETMASK, Some(&sigset), Some(&mut oset))?;
            zsh_sys::zhandler(zsh_sys::signal_queue[queue_front as usize]);
            signal::sigprocmask(signal::SigmaskHow::SIG_SETMASK, Some(&oset), None)?;
        }
    }
    Ok(())
}

pub fn dont_queue_signals() -> nix::Result<()> {
    unsafe {
        write_volatile(&raw mut zsh_sys::queueing_enabled, 0);
        run_queued_signals()
    }
}

pub fn restore_queue_signals(level: i32) {
    unsafe {
        write_volatile(&raw mut zsh_sys::queueing_enabled, level);
    }
}

pub fn with_queued_signals<T, F: FnOnce(&QueuedSignalToken) -> T>(func: F) -> (T, nix::Result<()>) {
    let token = queue_signals();
    let result = func(&token);
    (result, unqueue_signals(token))
}

#[derive(Default)]
pub struct SignalSafeWrapper<T>(T);

impl<T> SignalSafeWrapper<T> {
    pub const fn new(inner: T) -> Self {
        Self(inner)
    }

    pub fn get(&self) -> SignalSafeWrapperRef<'_, T> {
        let token = queue_signals();
        SignalSafeWrapperRef{ inner: &self.0, token }
    }

    pub fn get_with_token<'a>(&'a self, _token: &'a QueuedSignalToken) -> &'a T {
        &self.0
    }
}

pub struct SignalSafeWrapperRef<'a, T> {
    inner: &'a T,
    #[allow(dead_code)]
    token: QueuedSignalToken,
}
crate::impl_deref_helper!(self: SignalSafeWrapperRef<'a, T>, &self.inner => &'a T);
