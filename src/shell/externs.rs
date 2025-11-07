mod fork;
use bstr::BString;
use crate::ui::{Ui, TrampolineIn};
use crate::c_string_array;
use std::os::fd::AsRawFd;
use std::os::raw::{c_char, c_int};
use std::ptr::null_mut;
use std::sync::{LazyLock, OnceLock, Mutex};
use std::ffi::CString;
use anyhow::Result;
use crate::shell::zsh;
use crate::fork_lock::{RawForkLock, ForkLock};

static FORK_LOCK: RawForkLock = RawForkLock::new();

type GlobalState = (Ui, TrampolineIn, tokio::runtime::Runtime);
static STATE: ForkLock<'static, Mutex<Option<GlobalState>>> = FORK_LOCK.wrap(Mutex::new(None));

fn do_with_ui<F: Fn(bool, &mut GlobalState) -> T, T>(f: F) -> Result<T> {

    let lock = STATE.read();
    let mut store = lock.lock().unwrap();
    let store = &mut *store;
    let new = store.is_none();

    let state = if let Some(state) = store {
        state
    } else {
        let log_file = Box::new(std::fs::File::create("/tmp/wish.log").expect("Can't create log file"));
        env_logger::Builder::from_default_env()
            .target(env_logger::Target::Pipe(log_file))
            .format_source_path(true)
            .format_timestamp_millis()
            .init();

        let runtime = tokio::runtime::Runtime::new()?;

        let result: Result<_> = runtime.block_on(async {
            let (events, event_ctrl) = crate::event_stream::EventStream::new();
            let (mut ui, trampoline) = Ui::new(&FORK_LOCK, event_ctrl).await?;
            ui.activate().await?;
            ui.start_cmd().await?;

            if !crate::IS_FORKED.load(std::sync::atomic::Ordering::Relaxed) {
                let ui = ui.clone();
                tokio::task::spawn(async move {
                    let tty = std::fs::File::open("/dev/tty").unwrap();
                    let raw_fd = tty.as_raw_fd();
                    // 3. Set non-blocking mode
                    let flags = nix::fcntl::fcntl(raw_fd, nix::fcntl::FcntlArg::F_GETFL).unwrap();
                    let new_flags = nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK;
                    nix::fcntl::fcntl(raw_fd, nix::fcntl::FcntlArg::F_SETFL(new_flags)).unwrap();
                    events.run(tty, ui).await.unwrap();
                });
            }

            Ok((ui, trampoline))
        });
        let (ui, trampoline) = result?;
        store.get_or_insert((ui, trampoline, runtime))
    };

    Ok(f(new, state))
}

fn main() -> Result<Option<BString>> {
    do_with_ui(|new, (_, trampoline, runtime)| {
        runtime.block_on(trampoline.jump_in(!new))
    })
}

unsafe extern "C" fn handlerfunc(_nam: *mut c_char, argv: *mut *mut c_char, _options: zsh_sys::Options, _func: c_int) -> c_int {

    let argv = c_string_array::CStrArray::from(argv).to_vec();
    match argv.first().map(|s| s.as_slice()) {
        Some(b"lua") => {
            let result: Result<()> = tokio::task::block_in_place(|| {
                do_with_ui(|_, (ui, _, runtime)| {
                    runtime.block_on(async {
                        ui.shell.with_tmp_permit(|| async {
                            ui.lua.load(argv.get(1).map_or(b"" as _, |s| s.as_slice())).exec_async().await
                        }).await
                    })
                })??;
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
                nam: CString::new("wsh").unwrap().into_raw(),
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

static mut EMPTY_STR: [u8; 1] = [0];

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
unsafe extern "C" fn zle_entry_ptr_override(cmd: c_int, ap: *mut zsh_sys::__va_list_tag) -> *mut c_char {
    // this is the real entrypoint
    #[allow(static_mut_refs)]
    unsafe {
        if cmd == zsh_sys::ZLE_CMD_READ as _ && let Ok(_lock) = IS_RUNNING.try_lock() {
            let mut keymap = [b'm', b'a', b'i', b'n', 0];
            zsh::done = 0;
            zsh::selectlocalmap(null_mut());
            zsh::selectkeymap(keymap.as_mut_ptr().cast(), 1);
            zsh::histline = zsh_sys::curhist as _;
            zsh::lpromptbuf = EMPTY_STR.as_mut_ptr().cast();
            zsh::rpromptbuf = EMPTY_STR.as_mut_ptr().cast();

            let result = main();

            // zsh will reset the tty settings to its saved values
            // but it may have saved it at a bad time!
            // e.g. when we were running a foreground process
            // so save it again now while we're good
            zsh::gettyinfo(&raw mut zsh::shttyinfo);

            match result {
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
        } else {
            ORIGINAL_ZLE_ENTRY_PTR.get().unwrap().unwrap()(cmd, ap)
        }
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
    zsh::completion::override_compadd();
    0
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn cleanup_(module: zsh_sys::Module) -> c_int {
    zsh::completion::restore_compadd();
    let module_features: *mut zsh_sys::features = unsafe{ &raw mut MODULE_FEATURES.0 };
    unsafe{ zsh_sys::setfeatureenables(module, module_features, null_mut()) }
}

#[unsafe(no_mangle)]
pub extern "C" fn finish_() -> c_int {
    0
}
