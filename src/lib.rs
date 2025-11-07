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
#[macro_use]
mod utils;

use std::sync::atomic::{AtomicBool, Ordering};
static IS_FORKED: AtomicBool = AtomicBool::new(false);

fn is_forked() -> bool {
    IS_FORKED.load(Ordering::Relaxed)
}
