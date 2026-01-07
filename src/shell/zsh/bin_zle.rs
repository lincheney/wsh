use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io::{Write, Cursor};
use std::os::fd::RawFd;
use crate::unsafe_send::UnsafeSend;
use tokio::sync::{mpsc, oneshot};
use anyhow::Result;
use std::sync::{Arc, Mutex};
use std::ffi::{CString, CStr};
use std::os::raw::*;
use std::ptr::{null_mut};

static ZLE_FD_SOURCE: Mutex<Option<mpsc::UnboundedReceiver<FdChange>>> = Mutex::new(None);

struct ZleState {
    // original zle function
    original: UnsafeSend<zsh_sys::Builtin>,
    // sink to send fds
    fd_sink: Option<mpsc::UnboundedSender<FdChange>>,
    fd_mapping: HashMap<RawFd, (SyncFdChangeHook, oneshot::Sender<()>)>,
}

static ZLE_STATE: Mutex<Option<ZleState>> = Mutex::new(None);

#[derive(Debug)]
pub enum FdChange {
    Added(RawFd, SyncFdChangeHook, oneshot::Receiver<()>),
    Removed(RawFd),
}

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

    pub fn run(&self, _shell: &crate::shell::Shell, fd: RawFd, error: Option<std::io::Error>) {
        // this is way in excess of what we need
        let mut cursor = Cursor::new([0; 128]);
        write!(cursor, "{}", fd).unwrap();
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
                        let (sender, receiver) = oneshot::channel();
                        let hook = entry.insert((Arc::new(Mutex::new(hook)), sender));
                        Some(FdChange::Added(fd, hook.0.clone(), receiver))
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
    let original = super::pop_builtin("zle").unwrap();
    let (sender, receiver) = mpsc::unbounded_channel();

    *ZLE_STATE.lock().unwrap() = Some(ZleState{
        original: unsafe{ UnsafeSend::new(original) },
        fd_sink: Some(sender),
        fd_mapping: HashMap::default(),
    });
    *ZLE_FD_SOURCE.lock().unwrap() = Some(receiver);

    let mut zle = unsafe{ *original };
    zle.handlerfunc = Some(zle_handlerfunc);
    zle.node = zsh_sys::hashnode{
        next: null_mut(),
        nam: CString::new("zle").unwrap().into_raw(),
        flags: 0,
    };
    super::add_builtin("zle", Box::into_raw(Box::new(zle)));
    Ok(())
}

pub fn restore_zle() {
    if let Some(mut zle) = ZLE_STATE.lock().unwrap().take() {
        if !zle.original.as_ref().is_null() {
            super::add_builtin("zle", zle.original.into_inner());
            zle.original = unsafe{ UnsafeSend::new(null_mut()) };
        }
    }
}
