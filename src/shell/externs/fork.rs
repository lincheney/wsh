/*
* prefork/postfork handlers
*/

use std::sync::{atomic::Ordering};

static ATFORK_INIT: std::sync::Once = std::sync::Once::new();

// extern "C" fn prefork() {
// }

extern "C" fn postfork_child() {
    crate::IS_FORKED.store(true, Ordering::Relaxed);

    super::STATE.with(|state| {
        // if the state is None, it is probably not running
        // but there is no way to unregister this callback?
        if let Some(state) = &*state.borrow() {
            // clear pid table
            // since we are now the child, we won't be able to wait for any of them
            // we shouldn't have to rush this, since we don't have any child processes
            // we shouldn't get any SIGCHLD yet
            crate::shell::zsh::process::clear_pids();
            state.ui.pid_map.borrow_mut().clear();
        }
    });
}

pub fn init() {
    ATFORK_INIT.call_once(|| unsafe {
        nix::libc::pthread_atfork(None, None, Some(postfork_child));
    });
}
