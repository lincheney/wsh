mod shell;
mod fork_lock;
mod ui;
mod tui;
mod event_stream;
mod lua;
mod keybind;
mod unsafe_send;
mod timed_lock;
mod signals;
mod logging;
mod async_runtime;
mod canceller;
mod pauser;
#[macro_use]
mod utils;

pub use logging::log_if_err;
pub use async_runtime::spawn_and_log;

use std::sync::atomic::{AtomicBool, Ordering};
static IS_FORKED: AtomicBool = AtomicBool::new(false);
static EMPTY_STR: &std::ffi::CStr = c"";

fn is_forked() -> bool {
    IS_FORKED.load(Ordering::Relaxed)
}
