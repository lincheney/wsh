mod fork;
use bstr::BString;
use crate::ui::{Ui};
use std::os::raw::{c_char, c_int};
use std::rc::Rc;
use std::cell::RefCell;
use std::ptr::null_mut;
use std::sync::{LazyLock, OnceLock, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use anyhow::Result;
use crate::shell::{Shell, ShellMsg, zsh, MetaString, MetaArray};
use crate::fork_lock::{RawForkLock, ForkLock};

static FORK_LOCK: RawForkLock = RawForkLock::new();

pub(in crate::shell) struct GlobalState {
    pub(in crate::shell) ui: Ui,
    shell: Shell,
    pub runtime: tokio::runtime::Runtime,
    first_drawn: AtomicBool,
}

thread_local! {
    static STATE: RefCell<Option<ForkLock<'static, Rc<GlobalState>>>> = const{ RefCell::new(None) };
}

static ORIGINAL_ZLE_ENTRY_PTR: OnceLock<zsh_sys::ZleEntryPoint> = OnceLock::new();
static IS_RUNNING: Mutex<()> = Mutex::new(());

impl GlobalState {
    fn new() -> Result<Self> {
        crate::logging::init();
        fork::ForkState::init();

        let runtime = crate::async_runtime::init()?;
        let result: Result<_> = runtime.block_on(async {
            let (events, event_ctrl) = crate::event_stream::EventStream::new();
            let (shell, shell_client) = Shell::make();
            let ui = Ui::new(&FORK_LOCK, event_ctrl, shell_client)?;

            zsh::completion::override_compadd()?;
            zsh::widget::overrides::override_all()?;
            zsh::signals::init(&ui)?;

            if !crate::is_forked() {
                events.spawn(&ui);
                ui.init_lua()?;
                ui.get().inner.read().await.activate()?;
                zsh::bin_zle::override_zle();
                zsh::zle_watch_fds::init(&ui);

                unsafe {
                    let _ = ORIGINAL_ZLE_ENTRY_PTR.set(zsh_sys::zle_entry_ptr);
                    zsh_sys::zle_entry_ptr = Some(zle_entry_ptr_override);
                }
            }

            Ok((ui, shell))
        });
        let (ui, shell) = result?;

        Ok(Self {
            ui,
            shell,
            runtime,
            first_drawn: false.into(),
        })
    }

    pub fn with<T, F: FnOnce(&Rc<Self>) -> T>(f: F) -> Result<T> {
        STATE.with(|state| {
            if let Some(state) = &*state.borrow() {
                Ok(f(&state.read()))
            } else {
                anyhow::bail!("wish is not running")
            }
        })
    }

    fn get() -> Result<Rc<Self>> {
        Self::with(|state| state.clone())
    }

    fn shell_loop(&self) -> Result<Option<BString>> {
        self.shell_loop_internal(true)
    }

    fn shell_loop_oneshot<F: 'static + Send + Future<Output: Send>>(&self, future: F) -> Result<F::Output> {
        let ui = self.ui.clone();
        let handle = self.runtime.spawn(async move {
            let result = future.await;
            ui.shell.accept_line_trampoline(None).await?;
            Ok(result)
        });
        self.shell_loop_internal(false)?;
        self.runtime.block_on(handle)?
    }

    fn shell_loop_internal(&self, zle: bool) -> Result<Option<BString>> {
        loop {
            if zle && let Some(trampoline) = self.shell.trampoline.lock().unwrap().take() {
                let _ = trampoline.send(());
            }

            // allow sigwinch while we are waiting
            zsh::winch_unblock();
            let msg = self.shell.recv_from_queue()?;
            zsh::winch_block();

            match msg {
                Ok(ShellMsg::accept_line_trampoline{line, returnvalue}) => {
                    if zle {
                        *self.shell.trampoline.lock().unwrap() = Some(returnvalue);
                    } else {
                        returnvalue.send(()).unwrap();
                    }
                    return Ok(line)
                },
                Ok(msg) => self.shell.handle_one_message(msg),
                Err(_) => return Ok(None),
            }

            // sometimes zsh will trash zle without refreshing
            // redraw the ui
            if zle && self.runtime.block_on(self.ui.recover_from_unhandled_output(None)).unwrap() {
                // draw LATER
                self.ui.queue_draw();
            }
        }
    }

}

impl Drop for GlobalState {
    fn drop(&mut self) {
        zsh::completion::restore_compadd();
        zsh::widget::overrides::restore_all();
        zsh::bin_zle::restore_zle();
    }
}


pub fn run_with_shell<F: 'static + Send + Future<Output: Send>>(future: F) -> Result<F::Output> {
    GlobalState::get()?.shell_loop_oneshot(future)
}

unsafe extern "C" fn handlerfunc(_nam: *mut c_char, argv: *mut *mut c_char, _options: zsh_sys::Options, _func: c_int) -> c_int {

    let iter = unsafe{ MetaArray::iter_ptr(argv as _) };
    let mut iter = iter.map(|s| s.to_bytes());
    match iter.next() {
        Some(b"lua") => {
            let result: Result<()> = tokio::task::block_in_place(|| {

                let state = GlobalState::get()?;
                state.runtime.block_on(
                    state.ui.lua.load(iter.next().unwrap_or(b"" as _)).exec_async()
                )?;

                Ok(())
            });

            if let Err(e) = result {
                eprintln!("{e:?}");
                1
            } else {
                0
            }
        },

        Some(b".invoke-signal-handler") => {
            zsh::signals::invoke_signal_handler(iter.next())
        },

        Some(_) => {
            eprintln!("unknown arguments: {argv:?}");
            1
        },

        None => {
            0
        },
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
unsafe extern "C" fn zle_entry_ptr_override(cmd: c_int, ap: *mut zsh_sys::__va_list_tag) -> *mut c_char {
    // this is the real entrypoint

    if let Ok(state) = GlobalState::get() {
        #[allow(static_mut_refs)]
        unsafe {
            if cmd == zsh_sys::ZLE_CMD_READ as _ && let Ok(_lock) = IS_RUNNING.try_lock() {

                // lp = va_arg(ap, char **);
                // rp = va_arg(ap, char **);
                // flags = va_arg(ap, int);
                // context = va_arg(ap, int);
                // is these even right????
                let flags_ptr = (*ap).reg_save_area.add((*ap).gp_offset as usize + std::mem::size_of::<*mut *mut c_char>() * 2);

                let mut keymap = [b'm', b'a', b'i', b'n', 0];
                zsh::done = 0;
                zsh::lpromptbuf = crate::EMPTY_STR.as_ptr().cast_mut();
                zsh::rpromptbuf = crate::EMPTY_STR.as_ptr().cast_mut();
                zsh::free_prepostdisplay();
                zsh::zlereadflags = *(flags_ptr as *const c_int);
                zsh::histline = zsh_sys::curhist as _;
                zsh::selectkeymap(keymap.as_mut_ptr().cast(), 1);
                zsh::initundo();
                zsh::selectlocalmap(null_mut());
                zsh_sys::zleactive = 1;
                // window size may have changed since we last ran
                zsh_sys::adjustwinsize(0);
                zsh::signals::sigwinch::fetch_term_size_from_zsh();

                // this is the only thread we should ever run this func
                if !state.first_drawn.load(Ordering::Relaxed) {
                    crate::log_if_err::<_, anyhow::Error>(state.runtime.block_on(async {
                        if let Some(size) = zsh::signals::sigwinch::get_term_size() {
                            state.ui.handle_window_resize(size.0, size.1).await?;
                        }
                        state.ui.start_cmd().await?;
                        Ok(())
                    }));
                    state.first_drawn.store(true, Ordering::Relaxed);
                }

                let result = tokio::task::block_in_place(|| state.shell_loop());

                // zsh will reset the tty settings to its saved values
                // but it may have saved it at a bad time!
                // e.g. when we were running a foreground process
                // so save it again now while we're good
                zsh::gettyinfo(&raw mut zsh::shttyinfo);
                zsh::freeundo();
                zsh_sys::zleactive = 0;

                return match result {
                    Ok(Some(mut string)) => {
                        // MUST have a newline here
                        string.push(b'\n');
                        MetaString::from(string).into_raw()
                    },
                    Ok(None) => {
                        // TODO quit
                        null_mut()
                    },
                    Err(err) => {
                        log::error!("{:?}", err);
                        null_mut()
                    },
                }

            } else if cmd == zsh_sys::ZLE_CMD_TRASH as _ {
                // something is probably going to print (error msgs etc) to the terminal
                if let Ok(_lock) = state.ui.has_foreground_process.try_lock() {
                    state.ui.prepare_for_unhandled_output(None).unwrap();
                }
                return null_mut()

            } else if cmd == zsh_sys::ZLE_CMD_REFRESH as _ {
                // redraw the ui
                if state.runtime.block_on(state.ui.recover_from_unhandled_output(None)).unwrap() {
                    crate::log_if_err(state.runtime.block_on(state.ui.clone().draw()));
                }
                return null_mut()

            } else if cmd == zsh_sys::ZLE_CMD_RESET_PROMPT as _ {
                // redraw the prompt
                state.ui.get().borrow_mut().cmdline.prompt_dirty = true;
                state.ui.queue_draw();
                return null_mut()

            }
        }
    }

    if let Some(Some(func)) = ORIGINAL_ZLE_ENTRY_PTR.get() {
        unsafe { func(cmd, ap) }
    } else {
        // uhhhh wtf
        null_mut()
    }
}


#[derive(Debug)]
struct Features(zsh_sys::features);
unsafe impl Send for Features {}
unsafe impl Sync for Features {}

const DEFAULT_BUILTIN: zsh_sys::builtin = zsh_sys::builtin{
    node: zsh_sys::hashnode{ next: null_mut(), nam: null_mut(), flags: 0 },
    handlerfunc: None,
    minargs: 0,
    maxargs: -1,
    funcid: 0,
    optstr: null_mut(),
    defopts: null_mut(),
};

static mut MODULE_FEATURES: LazyLock<Features> = LazyLock::new(|| {
    let bn_list = Box::new([
        zsh_sys::builtin{
            node: zsh_sys::hashnode{
                next: null_mut(),
                nam: c"wsh".as_ptr().cast_mut(),
                flags: 0,
            },
            handlerfunc: Some(handlerfunc),
            ..DEFAULT_BUILTIN
        },
    ]);
    let bn_list = Box::leak(bn_list);

    Features(zsh_sys::features{
        // builtins
        bn_list: bn_list.as_mut_ptr(), bn_size: bn_list.len() as _,
        // conditions
        cd_list: null_mut(), cd_size: 0,
        // parameters
        pd_list: null_mut(), pd_size: 0,
        // math funcs
        mf_list: null_mut(), mf_size: 0,
        // abstract features
        n_abstract: 0,
    })
});

#[unsafe(no_mangle)]
pub extern "C" fn setup_() -> c_int {
    match GlobalState::new() {
        Ok(value) => {
            STATE.with(|state| {
                *state.borrow_mut() = Some(FORK_LOCK.wrap(Rc::new(value)));
            });
            0
        },
        Err(err) => {
            eprintln!("{err}");
            1
        },
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn features_(module: zsh_sys::Module, features: *mut *mut *mut c_char) -> c_int {
    unsafe {
        let module_features = &raw mut MODULE_FEATURES.0;
        *features = zsh_sys::featuresarray(module, module_features);
    }
    0
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn enables_(module: zsh_sys::Module, enables: *mut *mut c_int) -> c_int {
    unsafe {
        let module_features = &raw mut MODULE_FEATURES.0;
        zsh_sys::handlefeatures(module, module_features, enables)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn boot_() -> c_int {
    0
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn cleanup_(module: zsh_sys::Module) -> c_int {
    unsafe {
        let module_features = &raw mut MODULE_FEATURES.0;
        zsh_sys::setfeatureenables(module, module_features, null_mut())
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn finish_() -> c_int {
    STATE.with(|state| state.take());
    0
}
