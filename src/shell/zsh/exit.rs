use std::os::raw::{c_int, c_void};
use crate::shell::externs::GlobalState;

pub extern "C" fn exit_hook(_hook: zsh_sys::Hookdef, _arg: *mut c_void) -> c_int {
    let exit_val = unsafe{ zsh_sys::exit_val };
    let _ = GlobalState::with(|ui| {
        crate::log_if_err(ui.shell_loop(false, async {
            crate::log_if_err(ui.event_callbacks.exit(ui, exit_val).await);
        }));
    });
    crate::shell::externs::teardown(true);
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
        if zsh_sys::shell_exiting == 0 {
            zsh_sys::deletehookfunc(c"exit".as_ptr().cast_mut(), Some(exit_hook));
        }
    }
}
