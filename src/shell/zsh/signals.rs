use tokio::sync::{oneshot};
use std::collections::HashMap;
use std::ptr::null_mut;
use anyhow::Result;
use nix::sys::signal;
use std::os::fd::{IntoRawFd, BorrowedFd};
use std::os::raw::{c_int};
use std::io::{PipeReader};
use std::sync::{Mutex, atomic::{AtomicI32, Ordering}};
use tokio::io::AsyncReadExt;
mod pidset;

const SIGTRAPPED_COUNT: usize = 1024;
const CUSTOM_SIGCHLD: usize = SIGTRAPPED_COUNT - 1;
static CHILD_PIPE: AtomicI32 = AtomicI32::new(-1);
pub static PID_MAP: Mutex<Option<HashMap<pidset::Pid, oneshot::Sender<i32>>>> = Mutex::new(None);

extern "C" fn sigchld_handler(_sig: c_int) {
    // this *should* run in the main thread
    #[allow(static_mut_refs)]
    unsafe {
        // allow our trap to run
        // is this safe? trap queuing is only used for pid waiting
        // we just run a builtin so should be ok
        let trap_queueing_enabled = zsh_sys::trap_queueing_enabled;
        zsh_sys::trap_queueing_enabled = 0;
        // this should call our trap
        zsh_sys::zhandler(CUSTOM_SIGCHLD as _);
        zsh_sys::trap_queueing_enabled = trap_queueing_enabled;
    }
}

fn jobtab_iter<'a>() -> impl Iterator<Item=(*mut zsh_sys::job, bool, &'a mut zsh_sys::process, *mut zsh_sys::process)> {
    unsafe {
        (1 ..= zsh_sys::maxjob)
            .flat_map(|i| {
                let jobtab = zsh_sys::jobtab.add(i as _);
                [(jobtab, false), (jobtab, true)]
            }).flat_map(|(jobtab, aux)| {
                let mut proc = if aux { (*jobtab).auxprocs } else { (*jobtab).procs };
                let mut prev: *mut zsh_sys::process = null_mut();
                std::iter::from_fn(move || {
                    let p = proc.as_mut()?;
                    let oldprev = prev;
                    prev = proc;
                    proc = p.next;
                    Some((jobtab, aux, p, oldprev))
                })
            })
    }
}

pub fn invoke_sigchld_handler() -> c_int {
    #[allow(static_mut_refs)]
    unsafe {
        // call it for real
        debug_assert_eq!(zsh_sys::queueing_enabled, 0);

        // register any pids we are interested in
        if let Some(pids) = pidset::PID_TABLE.get() && !pids.is_empty() {
            super::queue_signals();
            for (&pid, (status, _)) in pids.iter() {
                // register these pids
                if status.load(Ordering::Relaxed) < 0 && jobtab_iter().any(|(_, _, proc, _)| proc.pid == pid) {
                    super::add_pid(pid);
                }
            }
            super::unqueue_signals().unwrap();
        }


        // reset thisjob so it doesn't go off reporting job statuses for foreground jobs
        let thisjob = zsh_sys::thisjob;
        zsh_sys::thisjob = 1;
        zsh_sys::zhandler(signal::Signal::SIGCHLD as _);
        zsh_sys::thisjob = thisjob;

        // check for our pids
        if let Some(pids) = pidset::PID_TABLE.get() && !pids.is_empty() {
            super::queue_signals();
            let mut found = false;
            for (jobtab, aux, proc, prev) in jobtab_iter() {
                // found one
                if proc.status >= 0 && let Some((status, added)) = pids.get(&proc.pid) {
                    found = true;
                    status.store(proc.status, std::sync::atomic::Ordering::Release);
                    // pop it off
                    if *added {
                        if !prev.is_null() {
                            (*prev).next = proc.next;
                        } else if aux {
                            (*jobtab).auxprocs = proc.next;
                        } else {
                            (*jobtab).procs = proc.next;
                        }
                    }
                    zsh_sys::zfree(proc as *mut _ as *mut _, std::mem::size_of::<zsh_sys::process>() as _);
                }
            }
            super::unqueue_signals().unwrap();

            // notify that we found something
            if found {
                let pipe = CHILD_PIPE.load(Ordering::Acquire);
                if pipe != -1 {
                    nix::unistd::write(BorrowedFd::borrow_raw(pipe), b"0").unwrap();
                }
            }

        }

        0
    }
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
    for (_, _, proc, _) in jobtab_iter() {
        if proc.pid == pid {
            return Some(proc.status)
        }
    }
    None
}

async fn sigchld_safe_handler(reader: PipeReader) -> Result<()> {
    let mut reader = tokio::net::unix::pipe::Receiver::from_owned_fd(reader.into()).unwrap();
    let mut buf = [0];
    loop {
        reader.read_exact(&mut buf).await?;
        if let Some(pid_map) = &mut *PID_MAP.lock().unwrap() {
            // check for pids that are done
            pidset::PID_TABLE.extract_finished_pids(|pid, status| {
                if let Some(sender) = pid_map.remove(&pid) {
                    let _ = sender.send(status);
                }
            });
        }
    }
}


fn resize_array<T: Copy + Default>(dst: &mut *mut T, old_len: usize, new_len: usize) {
    let mut new = vec![T::default(); new_len];
    new[..old_len].copy_from_slice(unsafe{ std::slice::from_raw_parts(*dst, old_len) });
    *dst = Box::into_raw(new.into_boxed_slice()).cast();
}

pub fn init() -> Result<()> {
    let (reader, writer) = std::io::pipe()?;
    crate::utils::set_nonblocking_fd(&writer)?;
    crate::utils::set_nonblocking_fd(&reader)?;

    #[allow(static_mut_refs)]
    unsafe {
        let trapcount = (zsh_sys::SIGCOUNT + 3 + nix::libc::SIGRTMAX() as u32 - nix::libc::SIGRTMIN() as u32 + 1) as usize;

        // make extra space so we can stuff our own "custom" signals
        debug_assert!(SIGTRAPPED_COUNT > trapcount);
        resize_array(&mut super::sigtrapped, trapcount, SIGTRAPPED_COUNT);
        debug_assert!(SIGTRAPPED_COUNT > trapcount);
        resize_array(&mut super::siglists, trapcount, SIGTRAPPED_COUNT);

        // now set a trap for sigchld
        let script = b"\\builtin wsh .invoke-sigchld-handler";
        let func = super::functions::Function::new(script.into())?;
        let eprog = func.0.as_ref().funcdef;
        (&mut *eprog).nref += 1;
        zsh_sys::settrap(CUSTOM_SIGCHLD as _, eprog, zsh_sys::ZSIG_TRAPPED as _);

        // set the sighandler
        let handler = signal::SigHandler::Handler(sigchld_handler);
        let action = signal::SigAction::new(handler, signal::SaFlags::empty(), signal::SigSet::empty());
        signal::sigaction(signal::Signal::SIGCHLD, &action)?;
    }

    // set the writer for the handler to use
    CHILD_PIPE.store(writer.into_raw_fd(), Ordering::Release);
    // spawn a reader task
    crate::spawn_and_log(sigchld_safe_handler(reader));

    Ok(())
}
