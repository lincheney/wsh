use crate::lua::{HasEventCallbacks};
pub(in crate::shell) mod fork;
use crate::ui::{Ui};
use std::os::raw::{c_char, c_int};
use std::cell::RefCell;
use std::ptr::null_mut;
use std::sync::{LazyLock, OnceLock, Mutex, atomic::{Ordering, AtomicUsize, AtomicBool}};
use anyhow::Result;
use crate::shell::{Shell, zsh, MetaString, MetaSlice};


thread_local! {
    static STATE: RefCell<Option<Ui>> = const{ RefCell::new(None) };
}

static ORIGINAL_ZLE_ENTRY_PTR: OnceLock<zsh_sys::ZleEntryPoint> = OnceLock::new();
static IS_RUNNING: Mutex<()> = Mutex::new(());
static FIRST_DRAWN: AtomicBool = AtomicBool::new(false);
pub static LUA_LEVEL: AtomicUsize = AtomicUsize::new(0);

fn teardown() {
    STATE.take();
}

pub struct GlobalState;

impl GlobalState {
    fn init() -> Result<Ui> {
        crate::logging::init();
        fork::init();

        LUA_LEVEL.store(0, Ordering::Release);
        let runtime = crate::async_runtime::Runtime::new()?;

        // runtime.enter();
        let (events, event_ctrl) = crate::event_stream::EventStream::new();
        let shell = Shell::new();
        let mut ui = Ui::new(event_ctrl, shell, runtime)?;

        ui.clone().runtime.block_on(async move {
            zsh::completion::override_compadd()?;
            zsh::widget::overrides::override_all()?;
            zsh::signals::init(&ui)?;

            if !crate::is_forked() {
                events.spawn(&ui, teardown);
                ui.report_error(ui.init_lua());
                ui.borrow().activate()?;
                zsh::bin_zle::override_zle();

                unsafe {
                    let _ = ORIGINAL_ZLE_ENTRY_PTR.set(zsh_sys::zle_entry_ptr);
                    zsh_sys::zle_entry_ptr = Some(zle_entry_ptr_override);
                }
            }
            Ok(ui)
        })
    }

    pub fn with<T, F: FnOnce(&Ui) -> T>(f: F) -> Result<T> {
        STATE.with(|ui| {
            if let Some(ui) = &*ui.borrow() {
                Ok(f(ui))
            } else {
                anyhow::bail!("wish is not running")
            }
        })
    }

    fn get() -> Result<Ui> {
        Self::with(|state| state.clone())
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

// pub fn shell_loop<F: 'static + Future>(future: F) -> Result<F::Output> {
    // GlobalState::get().and_then(|state| state.shell_loop(future))
// }


unsafe extern "C" fn handlerfunc(_nam: *mut c_char, argv: *mut *mut c_char, _options: zsh_sys::Options, _func: c_int) -> c_int {

    let iter = unsafe{ MetaSlice::iter_ptr(argv as _) };
    let mut iter = iter.map(|s| s.to_bytes());
    match iter.next() {
        Some(b"lua") => {
            let result: Result<_> = (|| {
                let ui = GlobalState::get()?;
                ui.runtime.block_on(ui.lua.load(iter.next().unwrap_or(b"" as _)).exec_async())?;
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
            zsh::signals::invoke_signal_handler_entrypoint(iter.next())
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

    if let Ok(ui) = GlobalState::get() {
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
                    let ui = ui.clone();
                    let result = ui.clone().shell_loop(async move {
                        // sometimes zsh will trash zle without refreshing
                        // redraw the ui
                        let drawn = FIRST_DRAWN.swap(true, Ordering::Relaxed);
                        let result = crate::log_if_err(ui.zle_cmd_refresh().await);
                        if result == Some(true) && drawn {
                            // draw LATER
                            ui.queue_draw();
                        }

                        if !drawn {
                            crate::log_if_err::<_, anyhow::Error>(async {
                                if let Some(size) = zsh::signals::sigwinch::get_term_size() {
                                    ui.handle_window_resize(size.0, size.1).await?;
                                }
                                ui.start_cmd(None).await?;
                                Ok(())
                            }.await);
                            ui.trigger_init_callbacks().await;
                        }
                    });
                    crate::log_if_err(result);
                }

                // allow sigwinch while we are waiting
                // zsh::winch_unblock();

                let result = ui.shell_loop(ui.shell.wait_for_accept_line());

                ui.clone().runtime.block_on(async move {
                    // sometimes zsh will trash zle without refreshing
                    // redraw the ui
                    if ui.zle_cmd_refresh().await.unwrap() {
                        // draw LATER
                        ui.queue_draw();
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
                    Ok(Ok(Some(mut string))) => {
                        // MUST have a newline here
                        string.push(b'\n');
                        MetaString::from(string).into_raw()
                    },
                    Ok(Ok(None)) => {
                        // TODO quit
                        null_mut()
                    },
                    Ok(Err(err)) => {
                        log::error!("{err:?}");
                        null_mut()
                    },
                    Err(err) => {
                        log::error!("{err:?}");
                        null_mut()
                    },
                }

            } else if cmd == zsh_sys::ZLE_CMD_TRASH as _ {
                // something is probably going to print (error msgs etc) to the terminal
                if let Ok(_lock) = ui.has_foreground_process.try_lock() {
                    ui.zle_cmd_trash().unwrap();
                }
                return null_mut()

            } else if cmd == zsh_sys::ZLE_CMD_REFRESH as _ {
                // redraw the ui
                ui.clone().runtime.weak_block_on(async move {
                    if ui.zle_cmd_refresh().await.unwrap() {
                        crate::log_if_err(ui.draw().await);
                    }
                });
                return null_mut()

            } else if cmd == zsh_sys::ZLE_CMD_RESET_PROMPT as _ {
                // redraw the prompt
                ui.borrow_mut().cmdline.prompt_dirty = true;
                ui.queue_draw();
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
    match GlobalState::init() {
        Ok(ui) => {
            STATE.with(|state| {
                *state.borrow_mut() = Some(ui);
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
