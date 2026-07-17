use std::ptr::null_mut;
use bstr::{BString};
use std::sync::Mutex;
use tokio::sync::{oneshot};
use std::collections::HashMap;
use anyhow::Result;
use std::os::raw::{c_int, c_void, c_char};
use std::collections::VecDeque;
use crate::utils::ConstHashMap;
use crate::shell::file_stream::Sink;
use super::queue::{SignalSafeWrapper, QueuedSignalToken};

pub type Pid = i32;
pub type PidMap = HashMap<Pid, oneshot::Sender<i32>>;
static mut THIS_JOB: i32 = 1;

enum Output {
    Status{pid: Pid, status: i32},
    Shout(BString),
}

struct State {
    sink: Option<Result<Sink, ()>>,
    pids: ConstHashMap<Pid, bool>,
    output: VecDeque<Output>,
}

unsafe impl Send for State {}
unsafe impl Sync for State {}

static STATE: SignalSafeWrapper<Mutex<State>> = const {
    SignalSafeWrapper::new(Mutex::new(State {
        sink: None,
        pids: ConstHashMap::new(),
        output: VecDeque::new(),
    }))
};

fn jobtab_retain_iter<'a, F: FnMut(&'a zsh_sys::process) -> bool>(_token: &QueuedSignalToken, mut callback: F) -> impl Iterator<Item=&'a zsh_sys::process> {
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

fn jobtab_iter<'a>(token: &QueuedSignalToken) -> impl Iterator<Item=&'a zsh_sys::process> {
    jobtab_retain_iter(token, |_| true)
}

pub fn clear_pids() {
    STATE.get().lock().unwrap().pids.clear();
}

pub fn register_pid(ui: &crate::ui::Ui, pid: Pid, add_to_jobtab: bool) -> Result<oneshot::Receiver<i32>> {
    let (sender, receiver) = oneshot::channel();
    ui.try_borrow_mut()?.pid_map.insert(pid, sender);
    STATE.get().lock().unwrap().pids.insert(pid, add_to_jobtab);
    Ok(receiver)
}

pub fn deregister_pid(ui: &crate::ui::Ui, pid: Pid) -> Result<()> {
    let _ = super::with_queued_signals(|token| {
        if matches!(STATE.get_with_token(token).lock().unwrap().pids.remove(&pid), Some(true)) {
            jobtab_retain_iter(token, |proc| proc.pid != pid).for_each(|_| ());
        }
    });
    ui.try_borrow_mut()?.pid_map.remove(&pid);
    Ok(())
}

pub(in crate::shell) fn check_pid_status(pid: Pid) -> Option<i32> {
    super::with_queued_signals(|token| {
        jobtab_iter(token).find(|proc| proc.pid == pid).map(|proc| proc.status)
    }).0
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
unsafe extern "C" fn sigchld_zle_entry_ptr_override(_: c_int, _: *mut zsh_sys::__va_list_tag) -> *mut c_char {
    // do nothing
    null_mut()
}

pub(super) fn sighandler(trapped: bool) -> c_int {

    unsafe {

        let _ = super::with_queued_signals(|token| {
            let mut notify = false;

            let state = &mut *STATE.get_with_token(token).lock().unwrap();

            // register any pids we are interested in
            for (&pid, &add) in state.pids.iter() {
                // register these pids
                if add && !jobtab_iter(token).any(|proc| proc.pid == pid) {
                    crate::shell::zsh::add_pid(pid);
                }
            }

            let sink = state.sink.get_or_insert_with(|| Sink::new().or(Err(())));
            let guard = sink.as_mut().map(|sink| {
                sink.clear();
                sink.override_shout(false, true)
            });
            // replace zle_entry_ptr to avoid deadlocks
            let old_zle_entry_ptr = zsh_sys::zle_entry_ptr;
            zsh_sys::zle_entry_ptr = Some(sigchld_zle_entry_ptr_override);
            // reset thisjob so it doesn't go off reporting job statuses for foreground jobs
            // call wait_for_processes instead of zhandler
            // zhandler only calls wait_for_processes and no traps anyway
            // but importantly we need to bypass queueing
            if trapped {
                let thisjob = zsh_sys::thisjob;
                zsh_sys::thisjob = THIS_JOB;
                zsh_sys::wait_for_processes();
                zsh_sys::thisjob = thisjob;
            } else {
                zsh_sys::wait_for_processes();
            }
            zsh_sys::zle_entry_ptr = old_zle_entry_ptr;
            drop(guard);
            let output = sink.as_mut().ok().and_then(|sink| sink.read());

            if let Some(output) = output {
                notify = true;
                state.output.push_back(Output::Shout(output));
            }

            jobtab_retain_iter(token, |proc| {
                // found one
                if proc.status >= 0 {
                    notify = true;
                    state.output.push_back(Output::Status{pid: proc.pid, status: proc.status});
                    // pop it off
                    if matches!(state.pids.remove(&proc.pid), Some(true)) {
                        return false
                    }
                }
                true
            }).for_each(drop);

            // notify that we found something
            if notify {
                super::write_to_self_pipe(super::SIGCHLD_BYTE);
            }

        });

        0
    }
}

pub fn handle_sigchld(ui: &crate::ui::Ui) -> Result<()> {
    let pid_map = &mut ui.try_borrow_mut()?.pid_map;
    for x in STATE.get().lock().unwrap().output.drain(..) {
        match x {
            Output::Status{pid, status} => if let Some(sender) = pid_map.remove(&pid) {
                let _ = sender.send(status);
            },
            Output::Shout(output) => {
                ui.handle_sigchld_shout(output);
            },
        }
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
    let mut state = STATE.get().lock().unwrap();
    state.pids.clear();
    state.output.clear();
}

pub(super) fn init(ui: &crate::ui::Ui) -> crate::ui::Ui {
    unsafe {
        zsh_sys::addhookfunc(c"before_trap".as_ptr().cast_mut(), Some(before_trap_hook));
    }
    ui.clone()
}
