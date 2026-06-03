use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::os::fd::RawFd;
use std::os::raw::*;
use crate::canceller;
use super::builtin::Builtin;
use super::meta_string::{MetaStr};
use super::zle_watch_fds::{FdChangeHook, SharedFdChangeHook};

struct ZleState {
    // original zle function
    original: Option<Builtin>,
    fd_mapping: HashMap<RawFd, (SharedFdChangeHook, canceller::Canceller)>,
}

thread_local! {
    static ZLE_STATE: RefCell<Option<ZleState>> = const{ RefCell::new(None) };
}

unsafe extern "C" fn zle_handlerfunc(nam: *mut c_char, argv: *mut *mut c_char, options: zsh_sys::Options, func: c_int) -> c_int {
    ZLE_STATE.with_borrow_mut(|zle| unsafe {
        let zle = zle.as_mut().unwrap();

        let fd_changed = if
            super::opt_isset(&*options, b'F')
            && (!super::opt_isset(&*options, b'L') || (*argv).is_null())
            && let Ok(fd) = std::str::from_utf8(&MetaStr::from_ptr(*argv).unmetafy())
            && let Ok(fd) = fd.parse()
        {
            Some((fd, !(*argv.add(1)).is_null()))
        } else {
            None
        };

        let result = zle.original.as_ref().as_ref().unwrap().handlerfunc.unwrap()(nam, argv, options, func);

        if result == 0 && let Some((fd, exists)) = fd_changed {

            // find the watch
            if exists && let Some(watch) = (0 .. super::nwatch)
                .map(|i| super::watch_fds.add(i as _))
                .find(|w| (**w).fd == fd)
            {

                let hook = Rc::new(FdChangeHook {
                    func: MetaStr::from_ptr((*watch).func).to_owned(),
                    widget: (*watch).widget != 0,
                });
                match zle.fd_mapping.entry(fd) {
                    Entry::Occupied(prev) => {
                        *prev.get().0.borrow_mut() = hook;
                    },
                    Entry::Vacant(entry) => {
                        let (canceller, cancellable) = canceller::new();
                        let hook = entry.insert((Rc::new(RefCell::new(hook)), canceller));
                        if let Err(err) = super::zle_watch_fds::register_fd(fd, &hook.0, cancellable) {
                            eprintln!("{err:?}");
                        }
                    },
                }

            } else {
                if exists {
                    log::error!("could not find watch for fd {fd}!");
                }

                // dropping the canceller will cause it to trigger
                drop(zle.fd_mapping.remove(&fd));
            };
        }

        result
    })
}

pub fn override_zle() {
    let original = Builtin::pop(meta_str!(c"zle")).unwrap();
    let mut zle = original.clone();

    ZLE_STATE.set(Some(ZleState{
        original: Some(original),
        fd_mapping: HashMap::default(),
    }));

    zle.handlerfunc = Some(zle_handlerfunc);
    zle.node.flags = 0;
    zle.add();
}

pub fn restore_zle() {
    if let Some(mut zle) = ZLE_STATE.take()
        && let Some(original) = zle.original.take()
    {
        original.add();
    }
}
