use std::os::raw::*;
use std::sync::Mutex;
use anyhow::{Result};
use super::super::bindings;
use super::super::MetaStr;
use crate::shell::externs::GlobalState;
use crate::lua::HasEventCallbacks;

/// Look up a thingy by name in thingytab and return its widget pointer.
unsafe fn get_widget(name: &MetaStr) -> Result<*mut bindings::widget> {
    unsafe {
        let getnode = (*bindings::thingytab).getnode.unwrap();
        let thingy = getnode(bindings::thingytab, name.as_ptr()).cast() as *mut bindings::thingy;
        if thingy.is_null() {
            anyhow::bail!("widget_override: could not find {name:?} in thingytab");
        }
        let widget = (*thingy).widget;
        if widget.is_null() {
            anyhow::bail!("widget_override: {name:?} thingy has null widget");
        }
        Ok(widget)
    }
}

/// Look up a widget by name in thingytab and swap its fn pointer.
fn override_widget(name: &MetaStr, new_fn: bindings::ZleIntFunc, state: &Mutex<Option<bindings::ZleIntFunc>>) -> Result<()> {
    unsafe {
        let widget = get_widget(name)?;
        let original_fn = (*widget).u.fn_;
        *state.lock().unwrap() = Some(original_fn);
        (*widget).u.fn_ = new_fn;
        Ok(())
    }
}

/// Restore a widget's original fn pointer.
fn restore_widget(name: &MetaStr, state: &Mutex<Option<bindings::ZleIntFunc>>) -> Result<()> {
    if let Some(original_fn) = state.lock().unwrap().take() {
        unsafe {
            (*get_widget(name)?).u.fn_ = original_fn;
        }
    }
    Ok(())
}

// --- undo widget override ---

static UNDO_NAME: &MetaStr = meta_str!(c".undo");
static UNDO_OVERRIDE: Mutex<Option<bindings::ZleIntFunc>> = Mutex::new(None);

unsafe extern "C" fn custom_undo(_args: *mut *mut c_char) -> c_int {
    let result = GlobalState::with(|state| {
        if state.ui.get().borrow_mut().buffer.move_in_history(false) {
            tokio::task::block_in_place(|| {
                state.runtime.block_on(state.ui.trigger_buffer_change_callbacks());
            });
        }
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            log::error!("custom_undo: {e}");
            1
        }
    }
}

fn override_undo() -> Result<()> {
    override_widget(UNDO_NAME, Some(custom_undo), &UNDO_OVERRIDE)
}

fn restore_undo() -> Result<()> {
    restore_widget(UNDO_NAME, &UNDO_OVERRIDE)
}

// --- redo widget override ---

static REDO_NAME: &MetaStr = meta_str!(c".redo");
static REDO_OVERRIDE: Mutex<Option<bindings::ZleIntFunc>> = Mutex::new(None);

unsafe extern "C" fn custom_redo(_args: *mut *mut c_char) -> c_int {
    let result = GlobalState::with(|state| {
        if state.ui.get().borrow_mut().buffer.move_in_history(true) {
            tokio::task::block_in_place(|| {
                state.runtime.block_on(state.ui.trigger_buffer_change_callbacks());
            });
        }
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            log::error!("custom_redo: {e}");
            1
        }
    }
}

fn override_redo() -> Result<()> {
    override_widget(REDO_NAME, Some(custom_redo), &REDO_OVERRIDE)
}

fn restore_redo() -> Result<()> {
    restore_widget(REDO_NAME, &REDO_OVERRIDE)
}

// --- split-undo widget override ---

static SPLIT_UNDO_NAME: &MetaStr = meta_str!(c".split-undo");
static SPLIT_UNDO_OVERRIDE: Mutex<Option<bindings::ZleIntFunc>> = Mutex::new(None);

unsafe extern "C" fn custom_split_undo(_args: *mut *mut c_char) -> c_int {
    // wish buffer doesn't have zsh's undo merging, so this is a no-op
    0
}

fn override_split_undo() -> Result<()> {
    override_widget(SPLIT_UNDO_NAME, Some(custom_split_undo), &SPLIT_UNDO_OVERRIDE)
}

fn restore_split_undo() -> Result<()> {
    restore_widget(SPLIT_UNDO_NAME, &SPLIT_UNDO_OVERRIDE)
}

// --- vi-undo-change widget override ---

static VI_UNDO_CHANGE_NAME: &MetaStr = meta_str!(c".vi-undo-change");
static VI_UNDO_CHANGE_OVERRIDE: Mutex<Option<bindings::ZleIntFunc>> = Mutex::new(None);

unsafe extern "C" fn custom_vi_undo_change(_args: *mut *mut c_char) -> c_int {
    // in zsh this toggles between end-of-undo-list and one step back;
    // for wish buffer, just do a regular undo
    let result = GlobalState::with(|state| {
        if state.ui.get().borrow_mut().buffer.move_in_history(false) {
            tokio::task::block_in_place(|| {
                state.runtime.block_on(state.ui.trigger_buffer_change_callbacks());
            });
        }
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            log::error!("custom_vi_undo_change: {e}");
            1
        }
    }
}

fn override_vi_undo_change() -> Result<()> {
    override_widget(VI_UNDO_CHANGE_NAME, Some(custom_vi_undo_change), &VI_UNDO_CHANGE_OVERRIDE)
}

fn restore_vi_undo_change() -> Result<()> {
    restore_widget(VI_UNDO_CHANGE_NAME, &VI_UNDO_CHANGE_OVERRIDE)
}

// --- public API ---

pub fn override_all() -> Result<()> {
    override_undo()?;
    override_redo()?;
    override_split_undo()?;
    override_vi_undo_change()?;
    Ok(())
}

pub fn restore_all() {
    crate::log_if_err(restore_undo());
    crate::log_if_err(restore_redo());
    crate::log_if_err(restore_split_undo());
    crate::log_if_err(restore_vi_undo_change());
}
