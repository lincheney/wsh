use std::os::raw::{c_int, c_void};
use crate::shell::externs::GlobalState;
use crate::lua::HasEventCallbacks;

pub extern "C" fn exit_hook(_hook: zsh_sys::Hookdef, _arg: *mut c_void) -> c_int {
    let _ = GlobalState::with(|ui| {
        let ui = ui.clone();
        crate::log_if_err(ui.clone().shell_loop(false, async move {
            crate::log_if_err(ui.trigger_exit_callbacks().await);
        }));
    });
    crate::shell::externs::teardown();
    0
}

pub fn init() {
    unsafe {
        zsh_sys::addhookfunc(c"exit".as_ptr().cast_mut(), Some(exit_hook));
    }
}

pub fn cleanup() {
    unsafe {
        // do NOT deletehookfunc while in the middle of exiting, will cause use after free
        if zsh_sys::exit_pending == 0 {
            zsh_sys::deletehookfunc(c"exit".as_ptr().cast_mut(), Some(exit_hook));
        }
    }
}
