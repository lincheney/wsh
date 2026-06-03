use std::sync::{atomic::{AtomicPtr, Ordering}};
use std::os::raw::*;
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
fn override_widget(name: &MetaStr, new_fn: bindings::ZleIntFunc, state: &AtomicPtr<c_void>) -> Result<()> {
    unsafe {
        let widget = get_widget(name)?;
        let original_fn = (*widget).u.fn_.map_or(std::ptr::null_mut(), |f| f as _);
        state.store(original_fn, Ordering::Release);
        (*widget).u.fn_ = new_fn;
        Ok(())
    }
}

/// Restore a widget's original fn pointer.
fn restore_widget(name: &MetaStr, state: &AtomicPtr<c_void>) -> Result<()> {
    let ptr = state.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !ptr.is_null() {
        unsafe {
            (*get_widget(name)?).u.fn_ = Some(std::mem::transmute(ptr));
        }
    }
    Ok(())
}

// --- undo widget override ---

static UNDO_NAME: &MetaStr = meta_str!(c".undo");
static UNDO_OVERRIDE: AtomicPtr<c_void> = const{ AtomicPtr::new(std::ptr::null_mut()) };

unsafe extern "C" fn custom_undo(_args: *mut *mut c_char) -> c_int {
    let result = GlobalState::with(|ui| {
        ui.borrow_mut().buffer.move_in_history(false);
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
static REDO_OVERRIDE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe extern "C" fn custom_redo(_args: *mut *mut c_char) -> c_int {
    let result = GlobalState::with(|ui| {
        ui.borrow_mut().buffer.move_in_history(true);
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
static SPLIT_UNDO_OVERRIDE: AtomicPtr<c_void> = const{ AtomicPtr::new(std::ptr::null_mut()) };

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
static VI_UNDO_CHANGE_OVERRIDE: AtomicPtr<c_void> = const{ AtomicPtr::new(std::ptr::null_mut()) };

unsafe extern "C" fn custom_vi_undo_change(_args: *mut *mut c_char) -> c_int {
    // in zsh this toggles between end-of-undo-list and one step back;
    // for wish buffer, just do a regular undo
    let result = GlobalState::with(|ui| {
        ui.borrow_mut().buffer.move_in_history(false);
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
