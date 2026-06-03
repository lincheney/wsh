use std::rc::{Rc, Weak};
use std::cell::RefCell;
use std::os::raw::{c_int};
use std::os::fd::{IntoRawFd, BorrowedFd};
use nix::sys::signal;
use anyhow::Result;
use tokio::sync::{Notify};
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

static SELF_PIPE: AtomicI32 = AtomicI32::new(-1);
thread_local! {
    static RECEIVER: RefCell<Option<Rc<Notify>>> = const{ RefCell::new(None) };
}
static LUA_PTR: AtomicPtr<mlua::ffi::lua_State> = AtomicPtr::new(std::ptr::null_mut());
const LUA_HOOK_MASK: c_int = mlua::ffi::LUA_MASKCALL | mlua::ffi::LUA_MASKRET | mlua::ffi::LUA_MASKLINE | mlua::ffi::LUA_MASKCOUNT;

pub fn get_subscriber() -> Option<Weak<Notify>> {
    RECEIVER.with(|r| r.borrow().as_ref().map(Rc::downgrade))
}

extern "C-unwind" fn lua_sigint_hook(lua: *mut mlua::ffi::lua_State, _ar: *mut mlua::ffi::lua_Debug) {
    unsafe {
        // keep interrupting lua so long as there is more
        if crate::shell::LUA_LEVEL.load(Ordering::Acquire) <= 1 {
            mlua::ffi::lua_sethook(lua, None, LUA_HOOK_MASK, 1);
        }
        mlua::ffi::lua_pushliteral(lua, c"interrupted");
        mlua::ffi::lua_error(lua);
    }
}

extern "C" fn sighandler(sig: c_int) {
    #[allow(static_mut_refs)]
    unsafe {

        // interrupt lua
        if crate::shell::LUA_LEVEL.load(Ordering::Acquire) > 0 {
            let lua = LUA_PTR.load(Ordering::Acquire);
            if !lua.is_null() {
                // this is safe
                // https://lua-l.lua.narkive.com/2F1sf9Vo/signal-safety-of-lua-sethook
                mlua::ffi::lua_sethook(lua, Some(lua_sigint_hook), LUA_HOOK_MASK, 1);
            }
        }

        zsh_sys::zhandler(sig);
        // bypass signal queueing
        let pipe = SELF_PIPE.load(Ordering::Acquire);
        if pipe != -1 {
            nix::unistd::write(BorrowedFd::borrow_raw(pipe), b"0").unwrap();
        }
    }
}

fn close_self_pipe() {
    let fd = SELF_PIPE.swap(-1, Ordering::AcqRel);
    if fd != -1 {
        let _ = nix::unistd::close(fd);
    }
}

pub(super) fn cleanup() {
    LUA_PTR.store(std::ptr::null_mut(), Ordering::Release);
    close_self_pipe();
    RECEIVER.with(|r| {
        *r.borrow_mut() = None;
    });
}

pub(in crate::shell) fn install_signal_handler() -> Result<()> {
    super::install_signal_handler(signal::Signal::SIGINT, false, Some(sighandler))
}

pub(super) fn init(ui: &crate::ui::Ui) -> Result<()> {
    let notify = Rc::new(Notify::new());
    RECEIVER.with(|r| {
        *r.borrow_mut() = Some(notify.clone());
    });

    // spawn a reader task
    let writer = super::self_pipe::<_, _, std::convert::Infallible>(ui, move || {
        notify.notify_waiters();
        Ok(())
    })?;

    // set the writer for the handler to use
    SELF_PIPE.store(writer.into_raw_fd(), Ordering::Release);

    unsafe {
        ui.lua.exec_raw::<()>((), |lua| {
            LUA_PTR.store(lua, Ordering::Release);
        }).unwrap();
    }

    super::hook_signal(signal::Signal::SIGINT)?;

    Ok(())
}
