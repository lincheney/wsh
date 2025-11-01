use std::os::fd::AsRawFd;
use std::ops::DerefMut;
use std::sync::{Mutex};
use anyhow::Result;
use std::ops::Deref;
use std::os::raw::{c_char, c_int};
use std::ptr::null_mut;
use std::sync::{LazyLock, OnceLock};
use std::ffi::CString;

mod shell;
mod zsh;
mod ui;
mod buffer;
mod c_string_array;
mod tui;
mod event_stream;
mod prompt;
mod lua;
mod keybind;
#[macro_use]
mod utils;

static STATE: OnceLock<ui::Ui> = OnceLock::new();
static UI: Mutex<Option<ui::Ui>> = Mutex::new(None);
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Runtime::new().unwrap()
});

async fn get_ui() -> Result<(ui::Ui, bool)> {
    let mut lock = UI.lock().unwrap();
    let store = lock.deref_mut();

    if let Some(ui) = store {
        Ok((ui.clone(), false))

    } else {
        let log_file = Box::new(std::fs::File::create("/tmp/wish.log").expect("Can't create log file"));
        env_logger::Builder::from_default_env()
            .target(env_logger::Target::Pipe(log_file))
            .format_source_path(true)
            .format_timestamp_millis()
            .init();

        let (events, event_ctrl) = event_stream::EventStream::new();

        let mut ui = ui::Ui::new(event_ctrl).await?;
        ui.activate().await?;
        ui.start_cmd().await?;
        *store = Some(ui.clone());

        let event_ui = ui.clone();
        tokio::task::spawn(async move {
            let tty = std::fs::File::open("/dev/tty").unwrap();
            let raw_fd = tty.as_raw_fd();
            // 3. Set non-blocking mode
            let flags = nix::fcntl::fcntl(raw_fd, nix::fcntl::FcntlArg::F_GETFL).unwrap();
            let new_flags = nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK;
            nix::fcntl::fcntl(raw_fd, nix::fcntl::FcntlArg::F_SETFL(new_flags)).unwrap();
            events.run(tty, event_ui).await.unwrap();
        });

        Ok((ui, true))
    }
}

async fn main() -> Result<i32> {
    let (ui, new) = get_ui().await?;
    ui.trampoline.jump_in(!new).await;
    Ok(0)
}


unsafe extern "C" fn handlerfunc(_nam: *mut c_char, argv: *mut *mut c_char, _options: zsh_sys::Options, _func: c_int) -> c_int {
    let argv = c_string_array::CStrArray::from(argv).to_vec();
    match argv.first().map(|s| s.as_slice()) {
        Some(b"lua") => {
            if let Some(ui) = STATE.get() {
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        ui.shell.with_tmp_permit(|| async {
                            ui.lua.load(argv.get(1).map(|s| s.as_slice()).unwrap_or(b"")).exec_async().await
                        }).await
                    })
                });
                if let Err(err) = result {
                    eprintln!("{:?}", err);
                    return 1;
                }
            }
        },
        Some(_) => {
            eprintln!("unknown arguments: {:?}", argv);
            return 1;
        },
        None => {
            return 0;
        },
    }

    0
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

static MODULE_FEATURES: LazyLock<Features> = LazyLock::new(|| {
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

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
unsafe extern "C" fn zle_entry_ptr_override(cmd: c_int, ap: *mut zsh_sys::__va_list_tag) -> *mut c_char {
    // this is the real entrypoint
    unsafe {
        if cmd == zsh_sys::ZLE_CMD_READ as _ && let Ok(_lock) = IS_RUNNING.try_lock() {
            zsh::done = 0;
            zsh::selectlocalmap(std::ptr::null_mut());
            zsh::selectkeymap(c"main".as_ptr() as _, 1);
            RUNTIME.block_on(async {
                match main().await {
                    // Ok(code) => code,
                    Ok(code) => code,
                    Err(err) => { log::error!("{:?}", err); 1 },
                }
            });
            zsh::zlegetline(std::ptr::null_mut(), std::ptr::null_mut())
        } else {
            ORIGINAL_ZLE_ENTRY_PTR.get().unwrap().unwrap()(cmd, ap)
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn setup_() -> c_int {
    unsafe{
        ORIGINAL_ZLE_ENTRY_PTR.get_or_init(|| zsh_sys::zle_entry_ptr);
        zsh_sys::zle_entry_ptr = Some(zle_entry_ptr_override);
    }
    0
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn features_(module: zsh_sys::Module, features: *mut *mut *mut c_char) -> c_int {
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ *features = zsh_sys::featuresarray(module, module_features as *const _ as *mut _); }
    0
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn enables_(module: zsh_sys::Module, enables: *mut *mut c_int) -> c_int {
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ zsh_sys::handlefeatures(module, module_features as *const _ as *mut _, enables) }
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
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ zsh_sys::setfeatureenables(module, module_features as *const _ as *mut _, null_mut()) }
}

#[unsafe(no_mangle)]
pub extern "C" fn finish_() -> c_int {
    0
}
