use std::cell::RefCell;
use std::io::{Write, Cursor};
use super::{MetaStr, MetaString};
use std::rc::Rc;
use std::os::fd::RawFd;
use anyhow::Result;
use tokio::io::unix::AsyncFd;

#[derive(Debug)]
pub struct FdChangeHook {
    pub func: MetaString,
    pub widget: bool,
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
// so there is an inner Rc to allow it to be cloned *out* of the mutex just for that one call
pub type SharedFdChangeHook = Rc<RefCell<Rc<FdChangeHook>>>;

fn run_hook(hook: &SharedFdChangeHook, fd: RawFd, error: Option<std::io::Error>) {
    // clone out of the refcell
    let hook = hook.borrow().clone();

    // this is way in excess of what we need
    let mut cursor = Cursor::new([0; 128]);
    write!(cursor, "{fd}").unwrap();
    let fdbuf = cursor.into_inner();
    let fdstr = MetaStr::from_bytes(&fdbuf);

    // what does this do
    let save_lbindk = unsafe{ super::refthingy(super::lbindk) };
    if hook.widget {
        unsafe {
            super::zlecallhook(hook.func.as_ptr().cast_mut(), fdstr.as_ptr().cast_mut());
        }
    } else {
        let args = [Some(fdstr), error.map(|_| MetaStr::new(c"err"))];
        super::call_hook_func(hook.func.as_ref(), args.into_iter().flatten());
    }
    unsafe {
        super::unrefthingy(super::lbindk);
        super::lbindk = save_lbindk;
    }
}

pub fn register_fd(fd: RawFd, hook: &SharedFdChangeHook, mut cancellable: crate::canceller::Cancellable) -> Result<()> {
    let reader = match AsyncFd::new(fd) {
        Ok(reader) => reader,
        Err(err) => {
            run_hook(hook, fd, Some(err));
            return Ok(())
        },
    };

    crate::shell::externs::GlobalState::with(|ui| {
        // spawn a task to wait on the fd
        let ui = ui.clone();
        let hook = hook.clone();
        ui.clone().runtime.spawn_local(async move {
            loop {
                let Some(_guard) = cancellable.run(reader.readable()).await
                    else { break };

                let hook = hook.clone();
                let ui_clone = ui.clone();
                let result = ui.clone().freeze_if(true, true, async move {
                    let hook = hook.clone();
                    ui_clone.shell.trampoline_out_callback(move |_ui, _token| {
                        run_hook(&hook, fd, None)
                    }).await
                }).await;

                let result = crate::log_if_err(result);
                let result = result.and_then(crate::log_if_err);

                if result.is_some() {
                    break
                }
            }
        });
    })
}
