use std::sync::atomic::{Ordering};
mod fork;
pub(super) mod signals;
use bstr::BString;
use crate::ui::{Ui};
use crate::c_string_array;
use std::os::raw::{c_char, c_int};
use std::ptr::null_mut;
use std::sync::{Arc, LazyLock, OnceLock, Mutex, mpsc};
use std::ffi::CString;
use anyhow::Result;
use crate::shell::{Shell, ShellClient, ShellMsg, zsh};
use crate::fork_lock::{RawForkLock, ForkLock};

static FORK_LOCK: RawForkLock = RawForkLock::new();

type GlobalState = (Ui, Shell, mpsc::Receiver<ShellMsg>, tokio::runtime::Runtime);
static STATE: ForkLock<'static, Mutex<Option<Arc<GlobalState>>>> = FORK_LOCK.wrap(Mutex::new(None));

pub fn with_runtime<T, F: FnMut(&tokio::runtime::Runtime) -> T>(mut f: F) -> Result<T> {
    let state = get_or_init_state()?;
    Ok(f(&state.3))
}

fn try_get_state() -> Option<Arc<GlobalState>> {
    let lock = STATE.read();
    let store = lock.lock().unwrap();
    store.clone()
}

fn get_or_init_state() -> Result<Arc<GlobalState>> {

    let lock = STATE.read();
    let mut store = lock.lock().unwrap();
    let store = &mut *store;

    let state = if let Some(state) = store {
        state
    } else {
        let log_file = Box::new(std::fs::File::create("/tmp/wish.log").expect("Can't create log file"));
        env_logger::Builder::from_default_env()
            .target(env_logger::Target::Pipe(log_file))
            .format_source_path(true)
            .format_timestamp_millis()
            .init();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .on_thread_start(|| {
                // try to make the main thread handle all signals
                if let Err(err) = signals::disable_all_signals() {
                    // mmmm pretty bad
                    log::error!("{:?}", err);
                }
            })
            .build()?;

        let shell = Shell::default();
        let result: Result<_> = runtime.block_on(async {
            let (events, event_ctrl) = crate::event_stream::EventStream::new();
            let (shell_client, shell_queue) = ShellClient::new(shell.clone());
            let mut ui = Ui::new(&FORK_LOCK, event_ctrl, shell_client)?;
            ui.get().inner.read().await.activate()?;
            ui.start_cmd().await?;

            if !crate::IS_FORKED.load(Ordering::Relaxed) {
                // spawn a task to take care of keyboard input
                {
                    let ui = ui.clone();
                    tokio::task::spawn(async move {
                        let tty = std::fs::File::open("/dev/tty").unwrap();
                        crate::utils::set_nonblocking_fd(&tty).unwrap();
                        events.run(tty, ui).await.unwrap();
                    });
                }

                // spawn a task to take care of watched fd
                if let Some(mut fd_source) = zsh::bin_zle::take_fd_change_source() {
                    let ui = ui.clone();
                    tokio::task::spawn(async move {
                        while let Some(change) = fd_source.recv().await {
                            crate::zle_watch_fds::handle_fd_change(&ui, change).await;
                        }
                    });
                }

                // spawn tasks to take care of signals
                signals::setup()?;
            }

            Ok((ui, shell, shell_queue))
        });
        let (ui, shell, shell_queue) = result?;
        store.get_or_insert(Arc::new((ui, shell, shell_queue, runtime)))
    };

    Ok(state.clone())
}

fn main() -> Result<Option<BString>> {
    let state = get_or_init_state()?;
    let (ui, shell, queue, runtime) = &*state;

    loop {
        if let Some(trampoline) = shell.trampoline.lock().unwrap().take() {
            let _ = trampoline.send(());
        }
        // let signals run while we are waiting for the next cmd
        shell.unqueue_signals()?;
        let msg = queue.recv();
        shell.queue_signals();

        match msg {
            Ok(ShellMsg::accept_line_trampoline{line, returnvalue}) => {
                *shell.trampoline.lock().unwrap() = Some(returnvalue);
                return Ok(line)
            },
            Ok(msg) => shell.handle_one_message(msg),
            Err(_) => return Ok(None),
        }

        // sometimes zsh will trash zle without refreshing
        // redraw the ui
        if runtime.block_on(ui.recover_from_unhandled_output()).unwrap() {
            // draw LATER
            // drawing may use shell, so we need to run it later when the shell is running the loop
            let mut ui = ui.clone();
            runtime.spawn(async move { ui.try_draw().await });
        }
    }
}

pub fn weak_main<F: 'static + Send + Future>(future: F) -> Result<F::Output> where F::Output: Send {
    let state = get_or_init_state()?;
    let (ui, shell, queue, runtime) = &*state;

    let ui = ui.clone();
    let handle = runtime.spawn(async move {
        let result = future.await;
        ui.shell.accept_line_trampoline(None).await;
        result
    });

    loop {
        match queue.recv() {
            Ok(ShellMsg::accept_line_trampoline{returnvalue, ..}) => {
                returnvalue.send(()).unwrap();
                break;
            },
            Ok(msg) => shell.handle_one_message(msg),
            Err(_) => break,
        }
    }
    Ok(runtime.block_on(handle)?)
}

unsafe extern "C" fn handlerfunc(_nam: *mut c_char, argv: *mut *mut c_char, _options: zsh_sys::Options, _func: c_int) -> c_int {

    let argv = c_string_array::CStrArray::from(argv).to_vec();
    match argv.first().map(|s| s.as_slice()) {
        Some(b"lua") => {
            let result: Result<()> = tokio::task::block_in_place(|| {

                let state = get_or_init_state()?;
                let ui = state.0.clone();
                let runtime = &state.3;

                runtime.block_on(async move {
                    ui.lua.load(argv.get(1).map_or(b"" as _, |s| s.as_slice())).exec_async().await
                })?;

                Ok(())
            });

            if let Err(e) = result {
                eprintln!("{e:?}");
                1
            } else {
                0
            }
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
                nam: c"wsh".as_ptr() as _,
                // CString::new("wsh").unwrap().into_raw(),
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

static ORIGINAL_ZLE_ENTRY_PTR: OnceLock<zsh_sys::ZleEntryPoint> = OnceLock::new();
static IS_RUNNING: Mutex<()> = Mutex::new(());

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
unsafe extern "C" fn zle_entry_ptr_override(cmd: c_int, ap: *mut zsh_sys::__va_list_tag) -> *mut c_char {
    // this is the real entrypoint
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
            zsh::zlereadflags = *(flags_ptr as *const c_int);
            zsh::histline = zsh_sys::curhist as _;
            zsh::selectkeymap(keymap.as_mut_ptr().cast(), 1);
            zsh::initundo();
            zsh::selectlocalmap(null_mut());
            zsh_sys::zleactive = 1;

            let result = tokio::task::block_in_place(main);

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
                    CString::new(string).unwrap().into_raw()
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

        } else if cmd == zsh_sys::ZLE_CMD_TRASH as _ && let Some(state) = try_get_state() {
            // something is probably going to print (error msgs etc) to the terminal
            let (ui, _, _, runtime) = &*state;
            if let Ok(_lock) = ui.has_foreground_process.try_lock() {
                runtime.block_on(ui.prepare_for_unhandled_output()).unwrap();
            }
            return null_mut()

        } else if cmd == zsh_sys::ZLE_CMD_REFRESH as _ && let Some(state) = try_get_state() {
            // redraw the ui
            let mut ui = state.0.clone();
            let runtime = &state.3;
            if runtime.block_on(ui.recover_from_unhandled_output()).unwrap() {
                runtime.block_on(ui.try_draw());
            }
            return null_mut()

        }

        ORIGINAL_ZLE_ENTRY_PTR.get().unwrap().unwrap()(cmd, ap)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn setup_() -> c_int {
    unsafe{
        fork::ForkState::setup();
        ORIGINAL_ZLE_ENTRY_PTR.get_or_init(|| zsh_sys::zle_entry_ptr);
        zsh_sys::zle_entry_ptr = Some(zle_entry_ptr_override);
    }
    0
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn features_(module: zsh_sys::Module, features: *mut *mut *mut c_char) -> c_int {
    let module_features: *mut zsh_sys::features = unsafe{ &raw mut MODULE_FEATURES.0 };
    unsafe{ *features = zsh_sys::featuresarray(module, module_features); }
    0
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn enables_(module: zsh_sys::Module, enables: *mut *mut c_int) -> c_int {
    let module_features: *mut zsh_sys::features = unsafe{ &raw mut MODULE_FEATURES.0 };
    unsafe{ zsh_sys::handlefeatures(module, module_features, enables) }
}

#[unsafe(no_mangle)]
pub extern "C" fn boot_() -> c_int {
    if let Err(err) = zsh::completion::override_compadd().and(zsh::bin_zle::override_zle()) {
        eprintln!("{err}");
        1
    } else {
        0
    }
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn cleanup_(module: zsh_sys::Module) -> c_int {
    zsh::completion::restore_compadd();
    zsh::bin_zle::restore_zle();
    let module_features: *mut zsh_sys::features = unsafe{ &raw mut MODULE_FEATURES.0 };
    unsafe{ zsh_sys::setfeatureenables(module, module_features, null_mut()) }
}

#[unsafe(no_mangle)]
pub extern "C" fn finish_() -> c_int {
    0
}
