use anyhow::Result;
use async_std::stream::StreamExt;
use futures::{select, future::FutureExt};
use std::ops::Deref;
use std::os::raw::{c_char, c_int};
use std::ptr::null_mut;
use std::sync::{OnceLock, LazyLock, Mutex};
use std::default::Default;
use std::ffi::CString;

mod shell;
mod zsh;
mod ui;
mod keybind;
mod buffer;
mod c_string_array;

async fn main() -> Result<()> {

    let ui = ui::Ui::new()?;
    ui.activate()?;
    ui.draw().await?;
    let mut events = crossterm::event::EventStream::new();

    loop {
        // let mut delay = std::pin::pin!(async_std::task::sleep(std::time::Duration::from_millis(1_000)).fuse());
        let mut events = events.next().fuse();

        select! {
            // _ = delay => { println!(".\r"); },
            event = events => {
                match event {
                    Some(Ok(event)) => {
                        if !ui.handle_event(event).await? {
                            break;
                        }
                    }
                    Some(Err(e)) => println!("Error: {:?}\r", e),
                    None => break,
                }
            }
        };
    }

    Ok(())
}


// bin_strftime(char *nam, char **argv, Options ops, int func)

unsafe extern "C" fn handlerfunc(nam: *mut c_char, argv: *mut *mut c_char, options: zsh_sys::Options, func: c_int) -> c_int {
    async_std::task::block_on(async {
        if let Err(x) = main().await {
            eprintln!("DEBUG(legman)\t{}\t= {:?}", stringify!(x), x);
        }
    });
    0
}

unsafe extern "C" fn compadd_handlerfunc(nam: *mut c_char, argv: *mut *mut c_char, options: zsh_sys::Options, func: c_int) -> c_int {
    eprintln!("DEBUG(bombay)\t{}\t= {:?}", stringify!(nam), nam);
    let argv = argv.into();
    ui::compadd(&argv, |argv| {
        let compadd = ORIGINAL_COMPADD.get().unwrap().lock().unwrap();
        (*compadd.0).handlerfunc.unwrap()(nam, argv.ptr, options, func)
    });
    // don't free argv?
    std::mem::forget(argv);
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
    maxargs: 0,
    funcid: 0,
    optstr: null_mut(),
    defopts: null_mut(),
};

static MODULE_FEATURES: LazyLock<Features> = LazyLock::new(|| {
    let bn_list = Box::new([
        zsh_sys::builtin{
            node: zsh_sys::hashnode{
                next: null_mut(),
                nam: CString::new("wash").unwrap().into_raw(),
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

#[derive(Debug)]
struct ShareablePointer<T>(*mut T);
unsafe impl<T> Send for ShareablePointer<T> {}

static ORIGINAL_COMPADD: OnceLock<Mutex<ShareablePointer<zsh_sys::builtin>>> = OnceLock::new();

fn override_compadd() {
    zsh::execstring("zmodload zsh/complete", Default::default());
    if zsh::get_return_code() == 0 {
        let mut compadd = ORIGINAL_COMPADD.get_or_init(|| Mutex::new(ShareablePointer(null_mut()))).lock().unwrap();
        *compadd = ShareablePointer(zsh::pop_builtin("compadd").unwrap());

        let builtin = zsh_sys::builtin{
            node: zsh_sys::hashnode{
                next: null_mut(),
                nam: CString::new("compadd").unwrap().into_raw(),
                flags: 0,
            },
            handlerfunc: Some(compadd_handlerfunc),
            ..DEFAULT_BUILTIN
        };
        zsh::add_builtin("compadd", Box::into_raw(Box::new(builtin)));
    }
}

fn restore_compadd() {
    if let Some(compadd) = ORIGINAL_COMPADD.get() {
        let mut compadd = compadd.lock().unwrap();
        if !compadd.0.is_null() {
            zsh::add_builtin("compadd", compadd.0);
            *compadd = ShareablePointer(null_mut());
        }
    }
}

#[no_mangle]
pub extern fn setup_() -> c_int {
    0
}

#[no_mangle]
pub extern fn features_(module: zsh_sys::Module, features: *mut *mut *mut c_char) -> c_int {
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ *features = zsh_sys::featuresarray(module, module_features as *const _ as *mut _); }
    0
}

#[no_mangle]
pub extern fn enables_(module: zsh_sys::Module, enables: *mut *mut c_int) -> c_int {
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ zsh_sys::handlefeatures(module, module_features as *const _ as *mut _, enables) }
}

#[no_mangle]
pub extern fn boot_() -> c_int {
    override_compadd();
    0
}

#[no_mangle]
pub extern fn cleanup_(module: zsh_sys::Module) -> c_int {
    restore_compadd();
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ zsh_sys::setfeatureenables(module, module_features as *const _ as *mut _, null_mut()) }
}

#[no_mangle]
pub extern fn finish_() -> c_int {
    0
}
