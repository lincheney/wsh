use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io::{Write, Cursor};
use std::os::fd::RawFd;
use crate::unsafe_send::UnsafeSend;
use tokio::sync::{mpsc};
use anyhow::Result;
use std::sync::{Arc, Mutex};
use std::ffi::{CString, CStr};
use std::os::raw::*;
use crate::canceller;
use super::builtin::Builtin;

static ZLE_FD_SOURCE: Mutex<Option<mpsc::UnboundedReceiver<FdChange>>> = Mutex::new(None);

struct ZleState {
    // original zle function
    original: UnsafeSend<Option<Builtin>>,
    // sink to send fds
    fd_sink: Option<mpsc::UnboundedSender<FdChange>>,
    fd_mapping: HashMap<RawFd, (SyncFdChangeHook, canceller::Canceller)>,
}

static ZLE_STATE: Mutex<Option<ZleState>> = Mutex::new(None);

pub enum FdChange {
    Added(RawFd, SyncFdChangeHook, canceller::Cancellable),
    Removed(RawFd),
}

// what on earth is this
// zle -F registration events get sent over a queue so its async
// however the hooks may modify itself (e.g. deregister itself)
// so there is a gap between when the hook is updated in zsh
// and the event is received over the queue where the hook
// may be incorrectly run
// instead we use this thing to have shared state
// we could just have an Arc<Mutex<Option<_>>> (the Option for when the fd is deregistered)
// but then when we call the hook and it modifies itself, we hit a deadlock on the mutex
// so there is an inner Arc to allow it to be cloned *out* of the mutex just for that one call
type SyncFdChangeHook = Arc<Mutex<Option<Arc<FdChangeHook>>>>;
#[derive(Debug)]
pub struct FdChangeHook {
    func: CString,
    widget: bool,
}

impl FdChangeHook {

    pub async fn run_locked(
        hook: &SyncFdChangeHook,
        shell: &crate::shell::ShellClient,
        fd: RawFd,
        error: Option<std::io::Error>,
    ) -> bool {
        let hook = hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            shell.run_watch_fd(hook, fd, error).await;
            true
        } else {
            false
        }
    }

    pub fn run(&self, _shell: &crate::shell::ShellInternal, fd: RawFd, error: Option<std::io::Error>) {
        // this is way in excess of what we need
        let mut cursor = Cursor::new([0; 128]);
        write!(cursor, "{fd}").unwrap();
        let fdbuf = cursor.into_inner();
        let fdstr = CStr::from_bytes_until_nul(&fdbuf).unwrap();

        // what does this do
        let save_lbindk = unsafe{ super::refthingy(super::lbindk) };
        if self.widget {
            unsafe {
                super::zlecallhook(self.func.as_ptr().cast_mut(), fdstr.as_ptr().cast_mut());
            }
        } else {
            let args = [Some(fdstr.to_bytes().into()), error.map(|_| b"err".into())];
            super::call_hook_func(self.func.clone(), args.into_iter().flatten());
        }
        unsafe {
            super::unrefthingy(super::lbindk);
            super::lbindk = save_lbindk;
        }
    }
}

unsafe extern "C" fn zle_handlerfunc(nam: *mut c_char, argv: *mut *mut c_char, options: zsh_sys::Options, func: c_int) -> c_int {
    unsafe {
        let mut zle = ZLE_STATE.lock().unwrap();
        let zle = zle.as_mut().unwrap();

        let fd_changed = if
            let Some(fd_sink) = &zle.fd_sink
            && super::opt_isset(&*options, b'F')
            && (!super::opt_isset(&*options, b'L') || (*argv).is_null())
            && let Some(fd) = CStr::from_ptr(*argv).to_str().ok().and_then(|s| s.parse::<RawFd>().ok())
        {
            Some((fd_sink, fd, !(*argv.add(1)).is_null()))
        } else {
            None
        };

        let result = zle.original.as_ref().as_ref().unwrap().handlerfunc.unwrap()(nam, argv, options, func);

        if result == 0 && let Some((fd_sink, fd, exists)) = fd_changed {

            // find the watch
            let payload = if exists && let Some(watch) = (0 .. super::nwatch)
                .map(|i| super::watch_fds.add(i as _))
                .find(|w| (**w).fd == fd)
            {

                let hook = Some(Arc::new(FdChangeHook {
                    func: CStr::from_ptr((*watch).func).to_owned(),
                    widget: (*watch).widget != 0,
                }));
                match zle.fd_mapping.entry(fd) {
                    Entry::Occupied(prev) => {
                        *prev.get().0.lock().unwrap() = hook;
                        None
                    },
                    Entry::Vacant(entry) => {
                        let (canceller, cancellable) = canceller::new();
                        let hook = entry.insert((Arc::new(Mutex::new(hook)), canceller));
                        Some(FdChange::Added(fd, hook.0.clone(), cancellable))
                    },
                }

            } else {
                if exists {
                    log::error!("could not find watch for fd {fd}!");
                }

                if let Some((prev, _canceller)) = zle.fd_mapping.remove(&fd) {
                    // dropping the canceller will cause it to trigger
                    prev.lock().unwrap().take();
                    Some(FdChange::Removed(fd))
                } else {
                    None
                }
            };

            if let Some(payload) = payload && fd_sink.send(payload).is_err() {
                // the receiver got dropped, so drop the sender too
                zle.fd_sink = None;
            }
        }

        result
    }
}

pub fn take_fd_change_source() -> Option<mpsc::UnboundedReceiver<FdChange>> {
    ZLE_FD_SOURCE.lock().unwrap().take()
}

pub fn override_zle() -> Result<()> {
    let original = Builtin::pop(c"zle").unwrap();
    let mut zle = original.clone();
    let (sender, receiver) = mpsc::unbounded_channel();

    *ZLE_STATE.lock().unwrap() = Some(ZleState{
        original: unsafe{ UnsafeSend::new(Some(original)) },
        fd_sink: Some(sender),
        fd_mapping: HashMap::default(),
    });
    *ZLE_FD_SOURCE.lock().unwrap() = Some(receiver);

    zle.handlerfunc = Some(zle_handlerfunc);
    zle.node.flags = 0;
    zle.add();
    Ok(())
}

pub fn restore_zle() {
    if let Some(mut zle) = ZLE_STATE.lock().unwrap().take() && let Some(original) = zle.original.as_mut().take() {
        original.add();
    }
}
