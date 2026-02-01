use tokio::sync::{oneshot};
use std::collections::HashMap;
use anyhow::Result;
use nix::sys::signal;
use std::os::fd::{IntoRawFd, BorrowedFd};
use std::os::raw::{c_int, c_void};
use std::sync::{Mutex, atomic::{AtomicI32, Ordering}};
mod pidset;

static SELF_PIPE: AtomicI32 = AtomicI32::new(-1);
pub static PID_MAP: Mutex<Option<HashMap<pidset::Pid, oneshot::Sender<i32>>>> = Mutex::new(None);
static mut THIS_JOB: i32 = 1;

fn jobtab_retain_iter<'a, F: FnMut(&'a zsh_sys::process) -> bool>(mut callback: F) -> impl Iterator<Item=&'a zsh_sys::process> {
    unsafe {
        let mut jobtab_iter = (1 ..= zsh_sys::maxjob)
            .flat_map(|i| {
                let jobtab = zsh_sys::jobtab.add(i as _);
                [&mut (*jobtab).procs, &mut (*jobtab).auxprocs]
            });
        let mut retain = false;
        let mut proc: *mut *mut zsh_sys::process = std::ptr::null_mut();

        std::iter::from_fn(move || {
            if !proc.is_null() {
                // goto to the next ptr
                if retain {
                    proc = &raw mut (**proc).next;
                } else {
                    // except if we delete, in which case assign the next pointer to prev
                    let old = *proc;
                    *proc = (**proc).next;
                    zsh_sys::zfree(old.cast(), std::mem::size_of::<zsh_sys::process>() as _);
                }
            }
            while proc.is_null() || (*proc).is_null() {
                proc = jobtab_iter.next()?;
            }
            retain = callback(&**proc);
            Some(&**proc)
        })
    }
}

fn jobtab_iter<'a>() -> impl Iterator<Item=&'a zsh_sys::process> {
    jobtab_retain_iter(|_| true)
}

pub fn register_pid(pid: pidset::Pid, add_to_jobtab: bool) -> oneshot::Receiver<i32> {
    let mut pid_map = PID_MAP.lock().unwrap();
    let pid_map = pid_map.get_or_insert_default();
    let (sender, receiver) = oneshot::channel();
    pid_map.insert(pid, sender);
    pidset::PID_TABLE.register_pid(pid, add_to_jobtab);
    receiver
}

pub(in crate::shell) fn check_pid_status(pid: pidset::Pid) -> Option<i32> {
    jobtab_iter().find(|proc| proc.pid == pid).map(|proc| proc.status)
}

pub(super) fn sighandler() -> c_int {
    #[allow(static_mut_refs)]
    unsafe {
        // register any pids we are interested in
        if let Some(pids) = pidset::PID_TABLE.get() && !pids.is_empty() {
            super::queue_signals();
            for (&pid, (status, add)) in pids.iter() {
                // register these pids
                if *add && status.load(Ordering::Relaxed) < 0 && !jobtab_iter().any(|proc| proc.pid == pid) {
                    super::add_pid(pid);
                }
            }
            super::unqueue_signals().unwrap();
        }

        // reset thisjob so it doesn't go off reporting job statuses for foreground jobs
        let thisjob = zsh_sys::thisjob;
        zsh_sys::thisjob = THIS_JOB;
        zsh_sys::zhandler(signal::Signal::SIGCHLD as _);
        zsh_sys::thisjob = thisjob;

        // check for our pids
        if let Some(pids) = pidset::PID_TABLE.get() && !pids.is_empty() {
            super::queue_signals();
            let mut found = false;
            jobtab_retain_iter(|proc| {
                // found one
                if proc.status >= 0 && let Some((status, added)) = pids.get(&proc.pid) {
                    found = true;
                    status.store(proc.status, std::sync::atomic::Ordering::Release);
                    // pop it off
                    if *added {
                        return false
                    }
                }
                true
            }).for_each(drop);
            super::unqueue_signals().unwrap();

            // notify that we found something
            if found {
                let pipe = SELF_PIPE.load(Ordering::Acquire);
                if pipe != -1 {
                    nix::unistd::write(BorrowedFd::borrow_raw(pipe), b"0").unwrap();
                }
            }

        }

        0
    }
}

extern "C" fn before_trap_hook(_hook: zsh_sys::Hookdef, _arg: *mut c_void) -> c_int {
    // traps make new jobs, so i need this hook to record what the original job is
    unsafe {
        // this is ok as it only ever runs in the main thread
        THIS_JOB = zsh_sys::thisjob;
    }
    0
}

pub(super) fn init() -> Result<()> {
    // spawn a reader task
    let writer = super::signals::self_pipe::<_, _, std::convert::Infallible>(|| {
        if let Some(pid_map) = &mut *PID_MAP.lock().unwrap() {
            // check for pids that are done
            pidset::PID_TABLE.extract_finished_pids(|pid, status| {
                if let Some(sender) = pid_map.remove(&pid) {
                    let _ = sender.send(status);
                }
            });
        }
        Ok(())
    })?;

    // set the writer for the handler to use
    SELF_PIPE.store(writer.into_raw_fd(), Ordering::Release);

    unsafe {
        zsh_sys::addhookfunc(c"before_trap".as_ptr().cast_mut(), Some(before_trap_hook));
    }

    super::signals::hook_signal(signal::Signal::SIGCHLD)?;

    Ok(())
}
