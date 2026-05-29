use std::ops::ControlFlow;
use std::cell::Cell;
use crate::lua::{ HasEventCallbacks};
pub(in crate::shell) mod fork;
use crate::ui::{Ui};
use std::os::raw::{c_char, c_int};
use std::rc::Rc;
use std::cell::RefCell;
use std::ptr::null_mut;
use std::sync::{LazyLock, OnceLock, Mutex};
use anyhow::Result;
use crate::shell::{Shell, zsh, MetaString, MetaSlice};


pub(in crate::shell) struct GlobalState {
    pub(in crate::shell) ui: Ui,
    pub runtime: tokio::runtime::Runtime,
    localset: tokio::task::LocalSet,
    first_drawn: Cell<bool>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<GlobalState>>> = const{ RefCell::new(None) };
}

static ORIGINAL_ZLE_ENTRY_PTR: OnceLock<zsh_sys::ZleEntryPoint> = OnceLock::new();
static IS_RUNNING: Mutex<()> = Mutex::new(());

fn teardown() {
    STATE.with(|state| state.take());
}

impl GlobalState {
    fn new() -> Result<Self> {
        crate::logging::init();
        fork::init();

        let runtime = crate::async_runtime::init()?;
        let localset = tokio::task::LocalSet::new();

        let result: Result<_> = localset.block_on(&runtime, async {
            let (events, event_ctrl) = crate::event_stream::EventStream::new();
            let shell = Shell::new();
            let mut ui = Ui::new(event_ctrl, shell)?;

            zsh::completion::override_compadd()?;
            zsh::widget::overrides::override_all()?;
            zsh::signals::init(&ui)?;

            if !crate::is_forked() {
                events.spawn(&ui, teardown);
                ui.report_error(ui.init_lua());
                ui.get().borrow().activate()?;
                zsh::bin_zle::override_zle();
                zsh::zle_watch_fds::init(&ui);

                unsafe {
                    let _ = ORIGINAL_ZLE_ENTRY_PTR.set(zsh_sys::zle_entry_ptr);
                    zsh_sys::zle_entry_ptr = Some(zle_entry_ptr_override);
                }
            }

            Ok(ui)
        });
        let ui = result?;

        Ok(Self {
            ui,
            runtime,
            localset,
            first_drawn: Cell::new(false),
        })
    }

    pub fn with<T, F: FnOnce(&Rc<Self>) -> T>(f: F) -> Result<T> {
        STATE.with(|state| {
            if let Some(state) = &*state.borrow() {
                Ok(f(state))
            } else {
                anyhow::bail!("wish is not running")
            }
        })
    }

    fn get() -> Result<Rc<Self>> {
        Self::with(|state| state.clone())
    }

    pub fn block_on<F: 'static + Future>(&self, future: F) -> F::Output {
        self.localset.block_on(&self.runtime, future)
    }

}

impl Drop for GlobalState {
    fn drop(&mut self) {
        zsh::signals::cleanup();
        zsh::completion::restore_compadd();
        zsh::widget::overrides::restore_all();
        zsh::bin_zle::restore_zle();
    }
}


pub fn block_on<F: 'static + Future>(future: F) -> Result<F::Output> {
    GlobalState::get().map(|state| state.block_on(future))
}

unsafe extern "C" fn handlerfunc(_nam: *mut c_char, argv: *mut *mut c_char, _options: zsh_sys::Options, _func: c_int) -> c_int {

    let iter = unsafe{ MetaSlice::iter_ptr(argv as _) };
    let mut iter = iter.map(|s| s.to_bytes());
    match iter.next() {
        Some(b"lua") => {
            let result: Result<_> = (|| {
                let state = GlobalState::get()?;
                state.block_on(state.ui.lua.load(iter.next().unwrap_or(b"" as _)).exec_async())?;
                Ok(())
            })();

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
                // need to do this or initundo() will fail
                zsh::zleline = zsh_sys::zalloc((256 + 2) * 4).cast(); // is this big enough?
                *zsh::zleline = 0;
                zsh::selectkeymap(keymap.as_mut_ptr().cast(), 1);
                zsh::initundo();
                zsh::selectlocalmap(null_mut());
                // need to do this all the time because zsh keeps resetting it
                crate::log_if_err(zsh::signals::sigint::install_signal_handler());
                zsh_sys::zleactive = 1;
                zsh_sys::errflag = 0;
                // window size may have changed since we last ran
                zsh_sys::adjustwinsize(0);
                zsh::signals::sigwinch::fetch_term_size_from_zsh();

                {
                    let state = state.clone();
                    state.clone().block_on(async move {
                        // sometimes zsh will trash zle without refreshing
                        // redraw the ui
                        let result = crate::log_if_err(state.ui.zle_cmd_refresh().await);
                        if result == Some(true) && state.first_drawn.get() {
                            // draw LATER
                            state.ui.queue_draw();
                        }

                        // this is the only thread we should ever run this func
                        if !state.first_drawn.get() {
                            crate::log_if_err::<_, anyhow::Error>(async {
                                if let Some(size) = zsh::signals::sigwinch::get_term_size() {
                                    state.ui.handle_window_resize(size.0, size.1).await?;
                                }
                                state.ui.start_cmd(None).await?;
                                Ok(())
                            }.await);
                            state.ui.trigger_init_callbacks().await;

                            state.first_drawn.set(true);
                        }
                    });
                }

                // allow sigwinch while we are waiting
                // zsh::winch_unblock();

                let result = loop {
                    ::log::debug!("DEBUG(casual)\t{}\t= {:?}", stringify!("loop"), "loop");
                    match state.block_on(state.ui.shell.trampoline_in()) {
                        Err(err) => break Err(err),
                        Ok(ControlFlow::Break(line)) => break Ok(line),
                        Ok(ControlFlow::Continue(callback)) => {
                            callback(state.ui.clone());
                        },
                    }
                };

                state.clone().block_on(async move {
                    // sometimes zsh will trash zle without refreshing
                    // redraw the ui
                    if state.ui.zle_cmd_refresh().await.unwrap() {
                        // draw LATER
                        state.ui.queue_draw();
                    }
                });

                // zsh::winch_block();

                zsh_sys::errflag = 0;

                // zsh will reset the tty settings to its saved values
                // but it may have saved it at a bad time!
                // e.g. when we were running a foreground process
                // so save it again now while we're good
                zsh::gettyinfo(&raw mut zsh::shttyinfo);
                zsh::freeundo();
                zsh_sys::free(zsh::zleline.cast());
                zsh::zleline = null_mut();
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
                    state.ui.zle_cmd_trash().unwrap();
                }
                return null_mut()

            } else if cmd == zsh_sys::ZLE_CMD_REFRESH as _ {
                // redraw the ui
                state.clone().block_on(async move {
                    if state.ui.zle_cmd_refresh().await.unwrap() {
                        crate::log_if_err(state.ui.draw().await);
                    }
                });
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
                *state.borrow_mut() = Some(Rc::new(value));
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
    teardown();
    0
}
