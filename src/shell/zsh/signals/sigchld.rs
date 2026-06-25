use tokio::sync::{oneshot};
use std::collections::HashMap;
use anyhow::Result;
use nix::sys::signal;
use std::os::raw::{c_int, c_void};
mod pidset;

pub type PidMap = HashMap<pidset::Pid, oneshot::Sender<i32>>;
// pub static PID_MAP: Mutex<Option<HashMap<pidset::Pid, oneshot::Sender<i32>>>> = Mutex::new(None);
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

pub fn clear_pids() {
    let _ = pidset::PidTable::clear();
}

pub fn register_pid(ui: &crate::ui::Ui, pid: pidset::Pid, add_to_jobtab: bool) -> Result<oneshot::Receiver<i32>> {
    let (sender, receiver) = oneshot::channel();
    let pid_map = &mut ui.try_borrow_mut()?.pid_map;
    pid_map.insert(pid, sender);
    let _ = pidset::PidTable::register_pid(pid, add_to_jobtab);
    Ok(receiver)
}

pub fn deregister_pid(ui: &crate::ui::Ui, pid: pidset::Pid) -> Result<()> {
    let pid_map = &mut ui.try_borrow_mut()?.pid_map;
    pid_map.remove(&pid);
    if matches!(pidset::PidTable::deregister_pid(pid), Ok(Some(true))) {
        jobtab_retain_iter(|proc| proc.pid != pid).for_each(|_| ());
    }
    Ok(())
}

pub(in crate::shell) fn check_pid_status(pid: pidset::Pid) -> Option<i32> {
    jobtab_iter().find(|proc| proc.pid == pid).map(|proc| proc.status)
}

pub(super) fn sighandler(trapped: bool) -> c_int {
    #[allow(static_mut_refs)]
    unsafe {
        // register any pids we are interested in
        let _ = pidset::PidTable::try_with(|pids| {
            for (&pid, (status, add)) in pids.iter() {
                // register these pids
                if *add && status.get() < 0 && !jobtab_iter().any(|proc| proc.pid == pid) {
                    crate::shell::zsh::add_pid(pid);
                }
            }
        });

        // reset thisjob so it doesn't go off reporting job statuses for foreground jobs
        if trapped {
            let thisjob = zsh_sys::thisjob;
            zsh_sys::thisjob = THIS_JOB;
            zsh_sys::zhandler(signal::Signal::SIGCHLD as _);
            zsh_sys::thisjob = thisjob;
        } else {
            zsh_sys::zhandler(signal::Signal::SIGCHLD as _);
        }

        // check for our pids
        let _ = pidset::PidTable::try_with(|pids| {
            let mut found = false;
            jobtab_retain_iter(|proc| {
                // found one
                if proc.status >= 0 && let Some((status, added)) = pids.get(&proc.pid) {
                    found = true;
                    status.set(proc.status);
                    // pop it off
                    if *added {
                        return false
                    }
                }
                true
            }).for_each(drop);

            // notify that we found something
            if found {
                super::write_to_self_pipe(super::SIGCHLD_BYTE);
            }
        });

        0
    }
}

pub fn handle_sigchld(ui: &crate::ui::Ui) -> Result<()> {
    let pid_map = &mut ui.try_borrow_mut()?.pid_map;
    if !pid_map.is_empty() {
        // check for pids that are done
        let _ = pidset::PidTable::extract_finished_pids(|pid, status| {
            if let Some(sender) = pid_map.remove(&pid) {
                let _ = sender.send(status);
            }
        });
    }
    Ok(())
}

extern "C" fn before_trap_hook(_hook: zsh_sys::Hookdef, _arg: *mut c_void) -> c_int {
    // traps make new jobs, so i need this hook to record what the original job is
    unsafe {
        // this is ok as it only ever runs in the main thread
        THIS_JOB = zsh_sys::thisjob;
    }
    0
}

pub(super) fn cleanup() {
    unsafe {
        zsh_sys::deletehookfunc(c"before_trap".as_ptr().cast_mut(), Some(before_trap_hook));
    }
    let _ = pidset::PidTable::clear();
}

pub(super) fn init(ui: &crate::ui::Ui) -> Result<crate::ui::Ui> {
    unsafe {
        zsh_sys::addhookfunc(c"before_trap".as_ptr().cast_mut(), Some(before_trap_hook));
    }
    Ok(ui.clone())
}
