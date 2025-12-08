use std::os::fd::{IntoRawFd, BorrowedFd};
use std::time::Duration;
use tokio::time::timeout;
use std::sync::atomic::{AtomicI32, Ordering};
use std::io::{PipeReader};
use std::os::raw::{c_int};
use std::sync::{LazyLock};
use tokio::sync::{watch};
use tokio::io::AsyncReadExt;
use anyhow::Result;
use nix::sys::signal;

static CHILD_WATCH: LazyLock<(watch::Sender<()>, watch::Receiver<()>)> = LazyLock::new(|| watch::channel(()));
static CHILD_PIPE: AtomicI32 = AtomicI32::new(-1);
static LAST_QUEUE_FRONT: AtomicI32 = AtomicI32::new(0);

pub fn disable_all_signals() -> nix::Result<()> {
    let mask = signal::SigSet::all();
    signal::sigprocmask(signal::SigmaskHow::SIG_SETMASK, Some(&mask), None)
}

extern "C" fn sigchld_handler(_sig: c_int) {
    // this *should* run in the main thread
    unsafe {
        zsh_sys::zhandler(signal::Signal::SIGCHLD as _);
        let pipe = CHILD_PIPE.load(Ordering::Acquire);
        if pipe != -1 {
            if zsh_sys::queueing_enabled > 0 {
                LAST_QUEUE_FRONT.store(zsh_sys::queue_front, Ordering::Release);
            }
            let data = [(zsh_sys::queueing_enabled > 0).into()];
            nix::unistd::write(BorrowedFd::borrow_raw(pipe), &data).unwrap();
        }
    }
}

async fn sigchld_safe_handler(reader: PipeReader) {
    let mut reader = tokio::net::unix::pipe::Receiver::from_owned_fd(reader.into()).unwrap();
    let mut buf = [0];
    let mut queueing_enabled = false;
    let mut queue_front = 0;
    loop {
        let fut = reader.read_exact(&mut buf);
        if queueing_enabled {
            // uhhhh i guess we poll
            // how long should we wait for
            if timeout(Duration::from_millis(100), fut).await.is_err() {
                // timed out, check the queue
                #[allow(static_mut_refs)]
                if queue_front == unsafe{ zsh_sys::queue_front } {
                    continue
                }
                // queue has been moved, the handler probably got called
                queueing_enabled = false;
            }
        } else {
            fut.await.unwrap();
        }

        if buf[0] == 1 {
            buf[0] = 0;
            queueing_enabled = true;
            queue_front = LAST_QUEUE_FRONT.load(Ordering::Acquire);
            continue
        }

        CHILD_WATCH.0.send(()).unwrap();
    }
}

pub async fn wait_for_pid(pid: i32, shell: &crate::shell::ShellClient) -> Option<c_int> {
    let mut receiver = CHILD_WATCH.1.clone();
    loop {
        let status = shell.find_process_status(pid, true).await?;
        if status >= 0 {
            return Some(status)
        }
        receiver.changed().await.unwrap();
    }
}

pub fn setup() -> Result<()> {
    let (reader, writer) = std::io::pipe()?;
    crate::utils::set_nonblocking_fd(&writer)?;
    crate::utils::set_nonblocking_fd(&reader)?;

    // set the writer for the handler to use
    CHILD_PIPE.store(writer.into_raw_fd(), Ordering::Release);
    // spawn a reader task
    tokio::task::spawn(sigchld_safe_handler(reader));

    unsafe {
        let handler = signal::SigHandler::Handler(sigchld_handler);
        let action = signal::SigAction::new(handler, signal::SaFlags::empty(), signal::SigSet::empty());
        signal::sigaction(signal::Signal::SIGCHLD, &action)?;
    }
    Ok(())
}
