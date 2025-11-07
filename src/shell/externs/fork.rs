/*
* prefork/postfork handlers
*/

use std::sync::{Mutex, Arc, atomic::{Ordering}};
use std::mem::transmute;
use parking_lot::lock_api::RawMutex;

#[allow(dead_code)]
pub struct ForkState {
    pid: u32,
    fork_lock: Option<crate::fork_lock::RawForkLockWriteGuard<'static, 'static>>,
    lua: Option<(mlua::AppDataRef<'static, ()>, Arc<mlua::Lua>)>,
}
unsafe impl Sync for ForkState {}
unsafe impl Send for ForkState {}

static FORK_STATE: Mutex<Option<ForkState>> = Mutex::new(None);

extern "C" fn prefork() {
    *FORK_STATE.lock().unwrap() = ForkState::new();
}

extern "C" fn postfork() {
    FORK_STATE.lock().unwrap().take();
}

impl ForkState {
    pub fn setup() {
        unsafe {
            nix::libc::pthread_atfork(Some(prefork), Some(postfork), Some(postfork));
        }
    }

    fn new() -> Option<Self> {
        // this adds a lot of overhead
        // is there some easy way to tell that zsh is just going to exec
        // straight afterwards and we don't have to worry about this stuff?

        // this is the big global fork lock
        let fork_lock = super::FORK_LOCK.write();

        let lua = {
            let ui = super::STATE.read_with_lock(&fork_lock);
            if let Some(state) = ui.lock().unwrap().as_ref() {
                let (ui, _, _) = &**state;
                if !ui.shell.is_locked() {
                    // shell is not locked == we are forking for some unknown reason
                    return None
                }

                // i can take a lock on lua by acquiring a ref to the app data
                ui.lua.set_app_data(());
                // ui.lua.gc_stop();
                Some((unsafe{ transmute(ui.lua.app_data_ref::<()>().unwrap()) }, ui.lua.clone()))
            } else {
                None
            }
        };

        Some(Self {
            pid: std::process::id(),
            fork_lock: Some(fork_lock),
            lua,
        })
    }

    fn is_parent(&self) -> bool {
        self.pid == std::process::id()
    }
}

impl Drop for ForkState {
    fn drop(&mut self) {
        if !self.is_parent() {
            crate::IS_FORKED.store(true, Ordering::Relaxed);

            if let Some(fork_lock) = self.fork_lock.take() {
                // reset the lock if in the child
                // we need to do this since all the other threads waiting on the lock
                // are now gone
                fork_lock.reset();
            }
        }

        if let Some((lock, lua)) = self.lua.take() {
            drop(lock);

            if !self.is_parent() {
                // what the heck is going on here
                // i can't do the same thing as the other mutexes pre/post fork
                // by taking the lock prefork and dropping the lock post fork
                // (which should work because it is held by the same thread)
                //
                // this is because mlua::Lua uses a parking_lot::ReentrantMutex,
                // if another thread waits on the lock even while we hold it,
                // the parking bit is set on the lock and as soon as we relase our lock
                // (as we do right above) it will attempt to hand the lock over to that thread
                // except that in the child this thread doesn't exist anymore so we get deadlock anyway
                // we need to unlock it manually instead, which should be ok
                // given that the "thread" that it locked on doesn't exist anymore
                //
                // i guess what we should do is actually fork mlua and make the lock accessible
                // instead of this transmute() stuff, but i'm lazy right now
                struct FakeLua {
                    raw: std::sync::Arc<parking_lot::ReentrantMutex<()>>,
                    // Controls whether garbage collection should be run on drop
                    _collect_garbage: bool,
                }
                let lua: Arc<FakeLua> = unsafe{ transmute(lua.clone()) };
                while lua.raw.is_locked() {
                    unsafe{
                        lua.raw.raw().unlock();
                    }
                }
            }

            // lua.gc_restart();
        }
    }
}
