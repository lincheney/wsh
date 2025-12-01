mod shell;
mod fork_lock;
mod ui;
mod buffer;
mod c_string_array;
mod tui;
mod event_stream;
mod prompt;
mod lua;
mod keybind;
mod signals;
mod unsafe_send;
#[macro_use]
mod utils;

use std::sync::atomic::{AtomicBool, Ordering};
static IS_FORKED: AtomicBool = AtomicBool::new(false);
static mut EMPTY_STR: [u8; 1] = [0];

fn is_forked() -> bool {
    IS_FORKED.load(Ordering::Relaxed)
}
