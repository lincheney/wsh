mod shell;
mod ui;
mod tui;
mod event_stream;
mod lua;
mod keybind;
mod logging;
mod async_runtime;
mod canceller;
mod pauser;
mod interrupter;
mod print_lock;
#[macro_use]
mod utils;

pub use logging::log_if_err;
pub use async_runtime::spawn_and_log;

use std::sync::atomic::{AtomicBool, Ordering};
static IS_FORKED: AtomicBool = AtomicBool::new(false);
static EMPTY_STR: &std::ffi::CStr = c"";

use std::time::Duration;
pub const DEFAULT_DURATION: Duration = Duration::from_millis(1000);

fn is_forked() -> bool {
    IS_FORKED.load(Ordering::Relaxed)
}
