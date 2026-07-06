use crate::lua::{HasEventCallbacks};
use std::cell::Cell;
use std::os::raw::{c_char, c_int};
use anyhow::{Result};
use super::super::bindings;
use super::super::MetaStr;
use crate::shell::externs::GlobalState;

/// Look up a thingy by name in thingytab and return its widget pointer.
unsafe fn get_widget(name: &MetaStr) -> Result<*mut bindings::widget> {
    unsafe {
        let getnode = (*bindings::thingytab).getnode.unwrap();
        let thingy: *mut bindings::thingy = getnode(bindings::thingytab, name.as_ptr()).cast();
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
fn override_widget(name: &MetaStr, new_fn: bindings::ZleIntFunc, state: &Cell<bindings::ZleIntFunc>) -> Result<()> {
    unsafe {
        let widget = get_widget(name)?;
        let original_fn = (*widget).u.fn_;
        state.set(original_fn);
        (*widget).u.fn_ = new_fn;
        Ok(())
    }
}

/// Restore a widget's original fn pointer.
fn restore_widget(name: &MetaStr, state: &Cell<bindings::ZleIntFunc>) -> Result<()> {
    if let Some(state) = state.replace(None) {
        unsafe {
            (*get_widget(name)?).u.fn_ = Some(state);
        }
    }
    Ok(())
}

fn move_in_history(forward: bool) -> Result<()> {
    GlobalState::with(|ui| {
        if ui.try_borrow_mut()?.buffer.move_in_history(forward) {
            ui.shell_loop(false, async {
                ui.trigger_buffer_change_callbacks().await
            })??;
        }
        Ok(())
    })?
}

// --- undo widget override ---

static UNDO_NAME: &MetaStr = meta_str!(c".undo");
thread_local! {
    static UNDO_OVERRIDE: Cell<bindings::ZleIntFunc> = const{ Cell::new(None) };
}

unsafe extern "C" fn custom_undo(_args: *mut *mut c_char) -> c_int {
    match move_in_history(false) {
        Ok(()) => 0,
        Err(e) => {
            log::error!("custom_undo: {e}");
            1
        }
    }
}

fn override_undo() -> Result<()> {
    UNDO_OVERRIDE.with(|state| override_widget(UNDO_NAME, Some(custom_undo), state))
}

fn restore_undo() -> Result<()> {
    UNDO_OVERRIDE.with(|state| restore_widget(UNDO_NAME, state))
}

// --- redo widget override ---

static REDO_NAME: &MetaStr = meta_str!(c".redo");
thread_local! {
    static REDO_OVERRIDE: Cell<bindings::ZleIntFunc> = const{ Cell::new(None) };
}

unsafe extern "C" fn custom_redo(_args: *mut *mut c_char) -> c_int {
    match move_in_history(true) {
        Ok(()) => 0,
        Err(e) => {
            log::error!("custom_redo: {e}");
            1
        }
    }
}

fn override_redo() -> Result<()> {
    REDO_OVERRIDE.with(|state| override_widget(REDO_NAME, Some(custom_redo), state))
}

fn restore_redo() -> Result<()> {
    REDO_OVERRIDE.with(|state| restore_widget(REDO_NAME, state))
}

// --- split-undo widget override ---

static SPLIT_UNDO_NAME: &MetaStr = meta_str!(c".split-undo");
thread_local! {
    static SPLIT_UNDO_OVERRIDE: Cell<bindings::ZleIntFunc> = const{ Cell::new(None) };
}

unsafe extern "C" fn custom_split_undo(_args: *mut *mut c_char) -> c_int {
    // wish buffer doesn't have zsh's undo merging, so this is a no-op
    0
}

fn override_split_undo() -> Result<()> {
    SPLIT_UNDO_OVERRIDE.with(|state| override_widget(SPLIT_UNDO_NAME, Some(custom_split_undo), state))
}

fn restore_split_undo() -> Result<()> {
    SPLIT_UNDO_OVERRIDE.with(|state| restore_widget(SPLIT_UNDO_NAME, state))
}

// --- vi-undo-change widget override ---

static VI_UNDO_CHANGE_NAME: &MetaStr = meta_str!(c".vi-undo-change");
thread_local! {
    static VI_UNDO_CHANGE_OVERRIDE: Cell<bindings::ZleIntFunc> = const{ Cell::new(None) };
}

unsafe extern "C" fn custom_vi_undo_change(_args: *mut *mut c_char) -> c_int {
    // in zsh this toggles between end-of-undo-list and one step back;
    // for wish buffer, just do a regular undo
    match move_in_history(false) {
        Ok(()) => 0,
        Err(e) => {
            log::error!("custom_vi_undo_change: {e}");
            1
        }
    }
}

fn override_vi_undo_change() -> Result<()> {
    VI_UNDO_CHANGE_OVERRIDE.with(|state| override_widget(VI_UNDO_CHANGE_NAME, Some(custom_vi_undo_change), state))
}

fn restore_vi_undo_change() -> Result<()> {
    VI_UNDO_CHANGE_OVERRIDE.with(|state| restore_widget(VI_UNDO_CHANGE_NAME, state))
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
